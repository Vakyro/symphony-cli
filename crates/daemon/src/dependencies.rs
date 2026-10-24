//! Dependencias entre tareas y milestones en el daemon (P08.S5, FLOW §9.3–§9.4).
//!
//! Un reactor escucha los cambios de estado de los agentes: cuando uno termina, reevalúa a
//! quienes lo esperaban (los libera a `READY` y, si ya tienen modelo, los arranca; o los
//! bloquea con una decisión pendiente) y revisa el gate de su milestone.

use std::sync::{Arc, Mutex};

use symphony_core::{AgentId, AgentState, ExecutionMode, MilestoneStatus, TaskId, TaskStatus};
use symphony_store::deps::{self, Finished, Reaction};
use symphony_store::repo;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use crate::runtime::{Runtime, now_ms};

/// Lo que el usuario decide ante una dependencia fallida (FLOW §9.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// Se quita la dependencia y la tarea sigue sin esperarla.
    Ignore,
    /// Igual que `Ignore`, pero la razón deja constancia de que la tarea se replanifica.
    Replan,
    /// La tarea dependiente se cancela.
    Cancel,
}

impl std::str::FromStr for Resolution {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "ignore" | "ignorar" => Ok(Self::Ignore),
            "replan" | "replanificar" => Ok(Self::Replan),
            "cancel" | "cancelar" => Ok(Self::Cancel),
            other => Err(format!(
                "acción desconocida `{other}` (ignore, replan o cancel)"
            )),
        }
    }
}

/// Arranca el reactor. Termina con `shutdown`.
pub fn spawn(runtime: Runtime, shutdown: CancellationToken) -> tokio::task::JoinHandle<()> {
    let mut changes = runtime.writer.state_changes();
    // La cuota también cambia sin que un agente cambie de estado: se revisa cada tanto, en otra
    // tarea para no retrasar la lectura de los cambios de estado.
    {
        let (rt, stop) = (runtime.clone(), shutdown.clone());
        tokio::spawn(async move {
            let mut refill = tokio::time::interval(std::time::Duration::from_secs(3));
            refill.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    () = stop.cancelled() => break,
                    _ = refill.tick() => rt.fill_batches().await,
                }
            }
        });
    }
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                change = changes.recv() => match change {
                    Ok(c) => {
                        let outcome = match c.to {
                            AgentState::Completed => Finished::Done,
                            AgentState::Failed => Finished::Failed,
                            AgentState::Cancelled => Finished::Cancelled,
                            _ => continue,
                        };
                        if let Ok(agent) = c.agent_id.parse::<AgentId>() {
                            react(&runtime, agent, outcome).await;
                            // Terminó uno: puede haber cupo para quien esperaba.
                            let rt = runtime.clone();
                            tokio::spawn(async move { rt.fill_batches().await });
                        }
                    }
                    // Se perdieron cambios de estado: nadie sabe cuáles, se revisa a todos.
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(perdidos = n, "el reactor de dependencias se atrasó");
                        recheck_waiting(&runtime).await;
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        }
    })
}

/// Reevalúa a quienes esperaban a la tarea de `agent` y arranca lo que quedó listo.
async fn react(rt: &Runtime, agent: AgentId, outcome: Finished) {
    // Si era un revisor, su veredicto se guarda (P08.S7).
    rt.on_agent_finished_review(agent, outcome != Finished::Done)
        .await;
    let task = rt
        .reader
        .lock()
        .ok()
        .and_then(|c| repo::get_agent(&c, agent).ok())
        .map(|a| a.task_id);
    let Some(task) = task else { return };
    reconcile_and_start(rt, task, outcome).await;
}

/// Un agente que nació esperando puede haber perdido la carrera contra el fin de su dependencia
/// (lo leyó pendiente y, antes de que se confirmara su creación, la dependencia terminó y el
/// reactor ya había pasado): queda `WAITING_DEPENDENCY` para siempre. Se reevalúa con las
/// dependencias que ya están hechas. Es idempotente.
pub async fn recheck_new(rt: &Runtime, task: TaskId) {
    let finished: Vec<(TaskId, Finished)> = rt
        .reader
        .lock()
        .ok()
        .and_then(|c| deps::dependencies_of(&c, task).ok())
        .map(|rows| {
            rows.into_iter()
                .filter_map(|d| {
                    let outcome = match d.status {
                        TaskStatus::Done => Finished::Done,
                        TaskStatus::Failed => Finished::Failed,
                        TaskStatus::Cancelled => Finished::Cancelled,
                        _ => return None,
                    };
                    Some((d.task_id, outcome))
                })
                .collect()
        })
        .unwrap_or_default();
    for (dep, outcome) in finished {
        reconcile_and_start(rt, dep, outcome).await;
    }
}

/// Reevalúa a todos los agentes que esperan una dependencia. Lo usa el reactor cuando se atrasa
/// en el canal de cambios de estado (`Lagged`): sin esto, una tarea terminada en ese hueco dejaba
/// a sus dependientes esperando para siempre. Es idempotente.
pub async fn recheck_waiting(rt: &Runtime) {
    let waiting: Vec<TaskId> = rt
        .reader
        .lock()
        .ok()
        .and_then(|c| {
            let mut stmt = c
                .prepare("SELECT task_id FROM agents WHERE state = 'WAITING_DEPENDENCY'")
                .ok()?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0)).ok()?;
            Some(
                rows.flatten()
                    .filter_map(|id| id.parse::<TaskId>().ok())
                    .collect(),
            )
        })
        .unwrap_or_default();
    for task in waiting {
        recheck_new(rt, task).await;
    }
}

/// Reevalúa a los dependientes de `task` (que terminó con `outcome`) y arranca a los liberados.
async fn reconcile_and_start(rt: &Runtime, task: TaskId, outcome: Finished) {
    let reactions: Arc<Mutex<Vec<Reaction>>> = Arc::default();
    let sink = reactions.clone();
    let written = rt
        .writer
        .write(Box::new(move |t| {
            let now = now_ms();
            let found = deps::reconcile_dependents(t, task, outcome, now)?;
            if outcome == Finished::Done
                && let Some(m) = deps::milestone_of_task(t, task)?
                && let Some(status) = deps::evaluate_milestone(t, m, now)?
            {
                tracing::info!(milestone = %m, status = status.as_str(), "milestone evaluado");
            }
            if let Ok(mut out) = sink.lock() {
                *out = found;
            }
            Ok(())
        }))
        .await;
    if let Err(e) = written {
        tracing::warn!(task = %task, error = %e, "no se pudieron reevaluar las dependencias");
        return;
    }
    let found = reactions.lock().map(|r| r.clone()).unwrap_or_default();
    for r in found {
        match r {
            Reaction::Released { agent, task } => {
                tracing::info!(agent = %agent, task = %task, "dependencias cumplidas");
                start_released(rt, agent).await;
            }
            Reaction::Blocked { agent, because, .. } => {
                tracing::info!(agent = %agent, "bloqueado por una dependencia: {because}");
            }
        }
    }
}

/// Un agente liberado arranca solo si ya sabe con qué modelo (exacto o profile); si quedó
/// «decidir después», espera en `READY` a que el usuario elija.
pub async fn start_released(rt: &Runtime, agent_id: AgentId) {
    // Mismo candado que `fill_batches`: sin él, el reactor y el repaso periódico calculan el mismo
    // cupo a la vez y arrancan más agentes de los que caben (carrera de P11.5.S5 vista con carga).
    let _one_at_a_time = rt.batch_fill.lock().await;
    // Un agente de un lote espera si la cuota de ahora no deja cupo; `fill_batches` lo arranca.
    if !rt.batch_has_room_for(agent_id) {
        rt.hold_for_batch(agent_id).await;
        return;
    }
    start_now(rt, agent_id).await;
}

/// Arranca a un agente `READY` sin mirar el cupo del lote (lo mira quien llama).
/// `true` si lo arrancó (los «decidir después» y los fallos no gastan cupo).
pub(crate) async fn start_now(rt: &Runtime, agent_id: AgentId) -> bool {
    let agent = rt
        .reader
        .lock()
        .ok()
        .and_then(|c| repo::get_agent(&c, agent_id).ok());
    let Some(agent) = agent else { return false };
    if agent.state != AgentState::Ready {
        return false;
    }
    let started = match (
        agent.execution_mode,
        &agent.requested_model_id,
        &agent.requested_profile_id,
    ) {
        (ExecutionMode::Exact, Some(model), _) => rt.switch(agent_id, model).await.map(|_| ()),
        (ExecutionMode::Profile, _, Some(profile)) => rt
            .switch_to_profile(agent_id, profile, None)
            .await
            .map(|_| ()),
        _ => return false,
    };
    match started {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(agent = %agent_id, error = %e.0, "no se pudo arrancar el agente liberado");
            false
        }
    }
}

/// Resultado de resolver una dependencia fallida.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub agent: AgentId,
    pub released: bool,
}

/// Aplica la decisión del usuario sobre una tarea bloqueada por una dependencia (FLOW §9.3).
pub async fn resolve(
    rt: &Runtime,
    task: TaskId,
    dependency: TaskId,
    action: Resolution,
) -> Result<Resolved, String> {
    let slot: Arc<Mutex<Option<Result<Resolved, String>>>> = Arc::default();
    let out = slot.clone();
    rt.writer
        .write(Box::new(move |t| {
            let now = now_ms();
            let result = (|| -> Result<Resolved, String> {
                let agent = t
                    .query_row(
                        "SELECT id, state FROM agents WHERE task_id = ?1
                         AND state NOT IN ('COMPLETED','FAILED','CANCELLED') ORDER BY created_at DESC LIMIT 1",
                        [task.to_string()],
                        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                    )
                    .map_err(|_| "la tarea no tiene un agente vivo".to_string())?;
                let id: AgentId = agent.0.parse().map_err(|_| "id de agente inválido".to_string())?;
                if agent.1 != AgentState::Blocked.as_str() {
                    return Err(format!(
                        "el agente está {}; solo se resuelve una dependencia con el agente BLOCKED",
                        agent.1
                    ));
                }
                match action {
                    Resolution::Cancel => {
                        repo::set_agent_state(t, id, AgentState::Cancelled, None, now)
                            .map_err(|e| e.to_string())?;
                        repo::set_task_status(t, task, TaskStatus::Cancelled, None, now)
                            .map_err(|e| e.to_string())?;
                        Ok(Resolved {
                            agent: id,
                            released: false,
                        })
                    }
                    Resolution::Ignore | Resolution::Replan => {
                        deps::remove_dependency(t, task, dependency).map_err(|e| e.to_string())?;
                        let still = deps::unmet_dependencies(t, task).map_err(|e| e.to_string())?;
                        // BLOCKED → READY es la salida manual de un bloqueo; si quedan otras
                        // dependencias sin cumplir, vuelve a esperar (READY → WAITING_DEPENDENCY).
                        repo::set_agent_state(t, id, AgentState::Ready, None, now)
                            .map_err(|e| e.to_string())?;
                        if !still.is_empty() {
                            let phrase = deps::waiting_phrase(&still);
                            repo::set_agent_state(
                                t,
                                id,
                                AgentState::WaitingDependency,
                                Some(&phrase),
                                now,
                            )
                            .map_err(|e| e.to_string())?;
                        }
                        let status = repo::get_task(t, task).map_err(|e| e.to_string())?.status;
                        if status == TaskStatus::Blocked {
                            let (next, note) = if still.is_empty() {
                                (TaskStatus::Ready, None)
                            } else {
                                (TaskStatus::Waiting, Some(deps::waiting_phrase(&still)))
                            };
                            let note = match (action, note) {
                                (Resolution::Replan, None) => {
                                    Some("replanificada sin la dependencia".to_string())
                                }
                                (_, n) => n,
                            };
                            repo::set_task_status(t, task, next, note.as_deref(), now)
                                .map_err(|e| e.to_string())?;
                        }
                        Ok(Resolved {
                            agent: id,
                            released: still.is_empty(),
                        })
                    }
                }
            })();
            if let Ok(mut o) = out.lock() {
                *o = Some(result);
            }
            Ok(())
        }))
        .await
        .map_err(|e| e.to_string())?;
    let result = slot
        .lock()
        .map_err(|_| "estado interno envenenado".to_string())?
        .take()
        .unwrap_or_else(|| Err("la escritura no devolvió resultado".into()))?;
    if result.released {
        start_released(rt, result.agent).await;
    }
    Ok(result)
}

/// Decisión del usuario ante el gate de un milestone (FLOW §9.4).
pub async fn decide_milestone(
    rt: &Runtime,
    milestone: symphony_core::MilestoneId,
    to: MilestoneStatus,
) -> Result<(), String> {
    let slot: Arc<Mutex<Option<Result<(), String>>>> = Arc::default();
    let out = slot.clone();
    rt.writer
        .write(Box::new(move |t| {
            let r = deps::decide_milestone(t, milestone, to, now_ms()).map_err(|e| e.to_string());
            if let Ok(mut o) = out.lock() {
                *o = Some(r);
            }
            Ok(())
        }))
        .await
        .map_err(|e| e.to_string())?;
    slot.lock()
        .map_err(|_| "estado interno envenenado".to_string())?
        .take()
        .unwrap_or_else(|| Err("la escritura no devolvió resultado".into()))
}
