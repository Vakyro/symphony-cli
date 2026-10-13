//! Modo delegado del chat (ADR-0013 F1, F3, F4, F11, F13): «haz X con N agentes y estos
//! modelos». Un lote es un plan (el mismo formato de ADR-0011) que se vuelve un agente por
//! tarea, con sus dependencias, bajo un milestone que lo agrupa para el seguimiento.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{Value, json};
use symphony_core::{AgentId, AgentState, ContextMode, FailoverPolicy, MilestoneId, TaskStatus};
use symphony_scheduler::{Plan, agent_concurrency};
use symphony_store::{deps, merge as store, repo};

use crate::runtime::{CreateAgent, Execution, Runtime, now_ms};

/// Tope por defecto de agentes simultáneos de un lote.
pub const DEFAULT_MAX_PARALLEL: usize = 3;

#[derive(Debug, Clone)]
pub struct BatchRequest {
    pub project_root: PathBuf,
    pub title: String,
    /// El plan en el formato de `scheduler::plan` (`- [ ] [id] texto (needs: a, b)`).
    pub plan: String,
    pub execution: Execution,
    pub max_parallel: Option<usize>,
    pub failover: FailoverPolicy,
    pub context_mode: ContextMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchAgent {
    pub id: String,
    pub agent_id: AgentId,
    pub task_code: String,
    pub state: AgentState,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchCreated {
    pub milestone: MilestoneId,
    /// Agentes simultáneos que se permiten (tope pedido recortado por la cuota conocida).
    pub parallel: usize,
    pub agents: Vec<BatchAgent>,
}

/// Una tarea del lote con su id (el del plan o uno sintético).
struct Node {
    id: String,
    text: String,
    phase: String,
    verify: Option<String>,
    needs: Vec<String>,
}

fn nodes_of(plan: &Plan) -> Vec<Node> {
    let mut out = Vec::new();
    for (pi, phase) in plan.phases.iter().enumerate() {
        for (ti, t) in phase.tasks.iter().enumerate() {
            out.push(Node {
                id: t
                    .id
                    .clone()
                    .unwrap_or_else(|| format!("f{}t{}", pi + 1, ti + 1)),
                text: t.text.clone(),
                phase: phase.title.clone(),
                verify: phase.verify.clone(),
                needs: t.needs.clone(),
            });
        }
    }
    out
}

impl Runtime {
    /// Fracciones de cuota conocidas (`KNOWN`) de los proveedores con cuota informada.
    fn known_quota(&self) -> Vec<f64> {
        self.reader
            .lock()
            .ok()
            .and_then(|c| symphony_store::health::list_health(&c).ok())
            .map(|rows| {
                rows.into_iter()
                    .filter(|h| h.health.certainty == symphony_core::QuotaCertainty::Known)
                    .filter_map(|h| h.health.remaining)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Cupos libres del lote ahora: el tope pedido, recortado por la cuota conocida, menos los
    /// agentes que ya ocupan uno. `None` si el lote no está registrado.
    fn batch_room(&self, milestone: MilestoneId) -> Option<usize> {
        let wanted = *self.batches.lock().ok()?.get(&milestone)?;
        let allowed = agent_concurrency(wanted, &self.known_quota());
        let in_flight: i64 = self
            .reader
            .lock()
            .ok()?
            .query_row(
                "SELECT COUNT(*) FROM agents a JOIN tasks t ON t.id = a.task_id
                 WHERE t.milestone_id = ?1
                   AND a.state IN ('RUNNING','TESTING','WAITING_PROVIDER','WAITING_RESOURCE')",
                [milestone.to_string()],
                |r| r.get(0),
            )
            .ok()?;
        Some(allowed.saturating_sub(usize::try_from(in_flight).unwrap_or(0)))
    }

    /// ¿Hay cupo para arrancar `agent`? Fuera de un lote, siempre. Con un lote, mira la cuota
    /// de ahora, no la de cuando se creó (P11.5.S5): si bajó, el agente espera; nunca se mata a
    /// uno en curso.
    pub(crate) fn batch_has_room_for(&self, agent: AgentId) -> bool {
        let milestone = self.reader.lock().ok().and_then(|c| {
            let task = repo::get_agent(&c, agent).ok()?.task_id;
            deps::milestone_of_task(&c, task).ok().flatten()
        });
        match milestone.and_then(|m| self.batch_room(m)) {
            Some(room) => room > 0,
            None => true,
        }
    }

    /// Deja constancia de por qué un agente liberado todavía no arranca.
    pub(crate) async fn hold_for_batch(&self, agent: AgentId) {
        let id = agent.to_string();
        let _ = self
            .writer
            .write(Box::new(move |t| {
                t.execute(
                    "UPDATE agents SET state_reason = ?1, updated_at = ?2 WHERE id = ?3 AND state = 'READY'",
                    rusqlite::params![
                        "esperando cupo del lote (la cuota conocida limita los simultáneos)",
                        now_ms(),
                        id
                    ],
                )?;
                Ok(())
            }))
            .await;
    }

    /// Arranca, mientras haya cupo, a los agentes de cada lote que quedaron esperándolo, en el
    /// orden del plan. Olvida los lotes sin agentes vivos. Lo llama el reactor de dependencias.
    pub async fn fill_batches(&self) {
        // Una pasada a la vez: dos en paralelo calcularían el mismo cupo y arrancarían de más.
        let _one_at_a_time = self.batch_fill.lock().await;
        let milestones: Vec<MilestoneId> = match self.batches.lock() {
            Ok(b) if !b.is_empty() => b.keys().copied().collect(),
            _ => return,
        };
        for m in milestones {
            let (held, alive): (Vec<AgentId>, i64) = {
                let Ok(c) = self.reader.lock() else { continue };
                let held = c
                    .prepare(
                        "SELECT a.id FROM agents a JOIN tasks t ON t.id = a.task_id
                         WHERE t.milestone_id = ?1 AND a.state = 'READY'
                           AND NOT EXISTS (SELECT 1 FROM agent_runs r WHERE r.agent_id = a.id)
                         ORDER BY t.rowid",
                    )
                    .and_then(|mut s| {
                        s.query_map([m.to_string()], |r| r.get::<_, String>(0))?
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|id| id.parse().ok())
                    .collect();
                let alive = c
                    .query_row(
                        "SELECT COUNT(*) FROM agents a JOIN tasks t ON t.id = a.task_id
                         WHERE t.milestone_id = ?1
                           AND a.state NOT IN ('COMPLETED','FAILED','CANCELLED')",
                        [m.to_string()],
                        |r| r.get(0),
                    )
                    .unwrap_or(1);
                (held, alive)
            };
            if alive == 0 {
                if let Ok(mut b) = self.batches.lock() {
                    b.remove(&m);
                }
                continue;
            }
            let Some(mut room) = self.batch_room(m) else {
                continue;
            };
            for agent in held {
                if room == 0 {
                    break;
                }
                if crate::dependencies::start_now(self, agent).await {
                    room -= 1;
                }
            }
        }
    }

    /// Crea los agentes de un lote. Los agentes que dependen de otros nacen esperando
    /// (`WAITING_DEPENDENCY`) y arrancan solos al cumplirse; el tope de simultáneos se aplica
    /// con dependencias entre tareas de una misma ola.
    pub async fn batch_create(&self, req: BatchRequest) -> Result<BatchCreated, String> {
        let plan = Plan::parse(&req.plan);
        // Un plan sin ids también es válido: solo se ordena por fases.
        let mut nodes = nodes_of(&plan);
        if nodes.is_empty() {
            return Err("el plan del lote no tiene tareas (`- [ ] texto`)".into());
        }
        let ids: std::collections::HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
        if ids.len() != nodes.len() {
            return Err("el plan del lote repite ids de tarea".into());
        }
        for n in &nodes {
            if let Some(bad) = n.needs.iter().find(|d| !ids.contains(d.as_str())) {
                return Err(format!(
                    "`{}` depende de `{bad}`, que no existe en el plan",
                    n.id
                ));
            }
        }
        let edges: Vec<(String, String)> = nodes
            .iter()
            .flat_map(|n| n.needs.iter().map(|d| (n.id.clone(), d.clone())))
            .collect();
        let waves = symphony_scheduler::topo_layers(nodes.iter().map(|n| n.id.clone()), &edges)
            .map_err(|c| format!("el plan del lote tiene un ciclo: {}", c.0.join(" → ")))?;

        // Tope de simultáneos: el pedido, recortado por la cuota que se sabe que queda.
        let wanted = req.max_parallel.unwrap_or(DEFAULT_MAX_PARALLEL).max(1);
        let parallel = agent_concurrency(wanted, &self.known_quota());
        if parallel == 0 {
            return Err(
                "la cuota conocida de un proveedor está por debajo del 10 %: espera a que se reinicie la ventana antes de lanzar un lote"
                    .into(),
            );
        }
        // Dependencias extra por tope: dentro de una ola, la tarea `i` espera a la `i - parallel`.
        let mut extra: HashMap<String, Vec<String>> = HashMap::new();
        for wave in &waves {
            for (i, id) in wave.iter().enumerate() {
                if i >= parallel {
                    extra
                        .entry(id.clone())
                        .or_default()
                        .push(wave[i - parallel].clone());
                }
            }
        }
        // Se crean en orden topológico (cada una después de sus dependencias).
        let order: Vec<String> = waves.into_iter().flatten().collect();
        nodes.sort_by_key(|n| order.iter().position(|o| *o == n.id).unwrap_or(usize::MAX));

        let mut codes: HashMap<String, String> = HashMap::new();
        let mut agents: Vec<BatchAgent> = Vec::new();
        let mut milestone: Option<MilestoneId> = None;
        for n in &nodes {
            let mut deps_ids: Vec<String> = n.needs.clone();
            deps_ids.extend(extra.get(&n.id).cloned().unwrap_or_default());
            deps_ids.sort();
            deps_ids.dedup();
            let depends_on: Vec<String> = deps_ids
                .iter()
                .filter_map(|d| codes.get(d).cloned())
                .collect();
            let mut description = format!("Tarea del lote «{}», fase «{}».", req.title, n.phase);
            if let Some(v) = &n.verify {
                description.push_str(&format!("\nVerifica con: {v}"));
            }
            let created = self
                .create_agent(CreateAgent {
                    project_root: req.project_root.clone(),
                    title: n.text.clone(),
                    description: Some(description),
                    execution: req.execution.clone(),
                    failover: req.failover,
                    context_mode: req.context_mode,
                    priority: 0,
                    chat: false,
                    depends_on: depends_on.clone(),
                    review: false,
                })
                .await
                .map_err(|e| format!("no se pudo crear `{}`: {e}", n.id))?;
            codes.insert(n.id.clone(), created.task_code.clone());
            // El milestone del lote se crea con el primer agente (ahí existe el proyecto).
            if milestone.is_none() {
                let project = self
                    .reader
                    .lock()
                    .ok()
                    .and_then(|c| repo::get_agent(&c, created.agent_id).ok())
                    .map(|a| a.project_id)
                    .ok_or_else(|| "no se encontró el proyecto del lote".to_string())?;
                let (title, slot) = (
                    req.title.clone(),
                    std::sync::Arc::new(std::sync::Mutex::new(None)),
                );
                let out = slot.clone();
                self.writer
                    .write(Box::new(move |t| {
                        let id = deps::create_milestone(t, project, &title, false)?;
                        if let Ok(mut s) = out.lock() {
                            *s = Some(id);
                        }
                        Ok(())
                    }))
                    .await
                    .map_err(|e| e.to_string())?;
                milestone = slot.lock().ok().and_then(|s| *s);
            }
            if let Some(m) = milestone {
                if let Ok(mut b) = self.batches.lock() {
                    b.insert(m, wanted);
                }
                let task = created.task_id;
                self.writer
                    .write(Box::new(move |t| {
                        deps::assign_task(t, task, Some(m), now_ms())?;
                        Ok(())
                    }))
                    .await
                    .map_err(|e| e.to_string())?;
            }
            agents.push(BatchAgent {
                id: n.id.clone(),
                agent_id: created.agent_id,
                task_code: created.task_code,
                state: created.state,
                depends_on,
            });
        }
        let milestone = milestone.ok_or_else(|| "el lote no creó ningún agente".to_string())?;
        // El chat sabe lo que delegó (F13): queda en su conversación.
        self.chat_note(
            agents.first().map(|a| a.agent_id),
            &format!(
                "Delegado «{}»: {} agentes ({}), hasta {} a la vez.",
                req.title,
                agents.len(),
                agents
                    .iter()
                    .map(|a| a.task_code.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                parallel
            ),
        )
        .await;
        Ok(BatchCreated {
            milestone,
            parallel,
            agents,
        })
    }

    /// Anota algo en la conversación del chat del proyecto de `agent` (si hay chat).
    pub(crate) async fn chat_note(&self, agent: Option<AgentId>, text: &str) {
        let Some(agent) = agent else { return };
        let chat = self.reader.lock().ok().and_then(|c| {
            let project = repo::get_agent(&c, agent).ok()?.project_id;
            repo::chat_agent(&c, project).ok()?
        });
        let Some(chat) = chat else { return };
        let (chat_id, text) = (chat.id, text.to_string());
        let _ = self
            .writer
            .write(Box::new(move |t| {
                repo::insert_message(
                    t,
                    &repo::NewMessage {
                        id: symphony_core::MessageId::new(),
                        agent_id: chat_id,
                        run_id: None,
                        role: repo::MessageRole::System,
                        content: Some(text),
                        content_object_id: None,
                    },
                    now_ms(),
                )?;
                Ok(())
            }))
            .await;
    }

    /// Estado de un lote: cada tarea con su agente y su integración (F11: «pendiente de
    /// integrar» = terminó y todavía no entró a la rama del chat).
    pub fn batch_status(&self, milestone: MilestoneId) -> Result<Value, String> {
        let c = self
            .reader
            .lock()
            .map_err(|_| "lector de la base no disponible".to_string())?;
        let m = deps::get_milestone(&c, milestone).map_err(|e| e.to_string())?;
        let mut stmt = c
            .prepare(
                "SELECT t.id, t.code, t.title, t.status, a.id, a.state, a.state_reason
                 FROM tasks t LEFT JOIN agents a ON a.task_id = t.id
                 WHERE t.milestone_id = ?1 ORDER BY t.rowid",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([milestone.to_string()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut tasks = Vec::new();
        let (mut done, mut pending_integration, mut merged) = (0, 0, 0);
        for row in rows.flatten() {
            let (task_id, code, title, status, agent, state, reason) = row;
            let mr = agent
                .as_deref()
                .and_then(|a| a.parse::<AgentId>().ok())
                .and_then(|a| store::latest_mr(&c, a).ok().flatten());
            let is_done = status == TaskStatus::Done.as_str();
            let is_merged = mr
                .as_ref()
                .is_some_and(|m| m.status == symphony_core::MergeStatus::Merged);
            if is_done {
                done += 1;
            }
            if is_merged {
                merged += 1;
            }
            let pending = is_done && !is_merged;
            if pending {
                pending_integration += 1;
            }
            tasks.push(json!({
                "task_id": task_id,
                "code": code,
                "title": title,
                "status": status,
                "agent_id": agent,
                "agent_state": state,
                "state_reason": reason,
                "merge_status": mr.as_ref().map(|m| m.status.as_str()),
                "pending_integration": pending,
            }));
        }
        Ok(json!({
            "milestone": {
                "id": m.id.to_string(),
                "name": m.name,
                "status": m.status.as_str(),
                "tasks_total": m.tasks_total,
                "tasks_done": m.tasks_done,
            },
            "done": done,
            "merged": merged,
            "pending_integration": pending_integration,
            "tasks": tasks,
        }))
    }

    /// Integra a la rama del chat lo que terminó del lote. `approve`: el usuario aprueba de
    /// una vez todo lo que el preflight deje en verde (si no, queda a la espera de `merge.approve`).
    pub async fn batch_integrate(
        &self,
        milestone: MilestoneId,
        approve: bool,
    ) -> Result<Value, String> {
        let agents: Vec<(AgentId, String)> = {
            let c = self
                .reader
                .lock()
                .map_err(|_| "lector de la base no disponible".to_string())?;
            let mut stmt = c
                .prepare(
                    "SELECT a.id, t.code FROM tasks t JOIN agents a ON a.task_id = t.id
                     WHERE t.milestone_id = ?1 AND t.status = 'DONE' ORDER BY t.rowid",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([milestone.to_string()], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(|e| e.to_string())?;
            rows.flatten()
                .filter_map(|(id, code)| Some((id.parse().ok()?, code)))
                .collect()
        };
        let mut results = Vec::new();
        for (agent, code) in agents {
            let already = self
                .reader
                .lock()
                .ok()
                .and_then(|c| store::latest_mr(&c, agent).ok().flatten())
                .is_some_and(|m| m.status == symphony_core::MergeStatus::Merged);
            if already {
                results.push(json!({"task": code, "result": "ya integrada"}));
                continue;
            }
            let pre = match self.merge_prepare(agent, true).await {
                Ok(p) => p,
                Err(e) => {
                    results.push(json!({"task": code, "result": "error", "detail": e}));
                    continue;
                }
            };
            if !pre.ready() {
                let why = pre
                    .checks
                    .iter()
                    .filter(|c| c.status == "FAIL")
                    .map(|c| format!("{}: {}", c.name, c.detail))
                    .collect::<Vec<_>>()
                    .join("; ");
                results.push(json!({"task": code, "result": "bloqueada", "detail": why}));
                continue;
            }
            if approve && let Err(e) = self.merge_approve(&pre.merge_request).await {
                results.push(json!({"task": code, "result": "error", "detail": e}));
                continue;
            }
            match self.merge_integrate(&pre.merge_request).await {
                Ok(commit) => {
                    results.push(json!({"task": code, "result": "integrada", "commit": commit}))
                }
                Err(e) => results.push(json!({"task": code, "result": "pendiente", "detail": e})),
            }
        }
        Ok(json!({ "results": results }))
    }
}
