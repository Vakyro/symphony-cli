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
                        }
                    }
                    Err(RecvError::Lagged(_)) => {}
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
    let agent = rt
        .reader
        .lock()
        .ok()
        .and_then(|c| repo::get_agent(&c, agent_id).ok());
    let Some(agent) = agent else { return };
    if agent.state != AgentState::Ready {
        return;
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
        _ => return,
    };
    if let Err(e) = started {
        tracing::warn!(agent = %agent_id, error = %e.0, "no se pudo arrancar el agente liberado");
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
