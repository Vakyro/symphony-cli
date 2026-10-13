//! Sugerencias de modelo en el daemon (P14.S2, FLOW §8.5). **Nunca cambia el modelo por sí
//! solo**: detecta, propone y espera al usuario (`[Switch] [Keep] [Don't suggest again]`).
//!
//! Se evalúa tras cada checkpoint del agente (una vez por checkpoint, solo si el agente no tiene
//! ya una sugerencia pendiente ni la apagó; CONSTRAINTS A11) con datos que ya están en la base.

use rusqlite::Connection;
use serde_json::{Value, json};
use symphony_core::{AgentId, AgentState, SuggestionStatus};
use symphony_router::suggest::{better_model, change_reason, classify};
use symphony_store::repo;
use symphony_store::suggestions::{self as store, NewSuggestion};
use tokio_util::sync::CancellationToken;

use crate::routing::Choose;
use crate::runtime::{Runtime, now_ms};

/// Profile con el que se compara «mejor para esta tarea».
const COMPARE_PROFILE: &str = "@code";
/// Cuántos archivos distintos, de los primeros que toca el agente, definen «cómo empezó».
const START_FILES: usize = 3;

/// Arranca el evaluador. Termina con `shutdown`.
pub fn spawn(runtime: Runtime, shutdown: CancellationToken) -> tokio::task::JoinHandle<()> {
    let mut taken = runtime.bus.checkpoints_taken();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                got = taken.recv() => match got {
                    Ok(agent) => {
                        if let Err(e) = runtime.evaluate_suggestion(agent).await {
                            tracing::debug!(agent = %agent, error = %e, "sugerencia: no se pudo evaluar");
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    })
}

/// Archivos del `summary_json` de un checkpoint.
fn files_of(summary: &str) -> Vec<String> {
    serde_json::from_str::<Value>(summary)
        .ok()
        .and_then(|v| {
            v["files_touched"].as_array().map(|a| {
                a.iter()
                    .filter_map(|f| f["path"].as_str().map(str::to_string))
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// Los primeros archivos que tocó el agente, de los eventos (que no se podan, a diferencia de
/// los checkpoints): de ahí sale «cómo empezó».
fn first_files(conn: &Connection, agent: AgentId) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT json_extract(payload_json, '$.path') FROM events
         WHERE agent_id = ?1 AND type = 'FileModified' ORDER BY id ASC LIMIT 40",
    )?;
    let rows = stmt.query_map([agent.to_string()], |r| r.get::<_, Option<String>>(0))?;
    let mut out: Vec<String> = Vec::new();
    for path in rows.flatten().flatten() {
        if !out.contains(&path) {
            out.push(path);
        }
        if out.len() == START_FILES {
            break;
        }
    }
    Ok(out)
}

/// Lo que decidió el evaluador (calculado sin esperar a la base).
struct Plan {
    /// Una pendiente que ya no aplica (el agente cambió de modelo o terminó).
    close_stale: Option<String>,
    new: Option<NewSuggestion>,
}

impl Runtime {
    /// Evalúa si conviene sugerir otro modelo a `agent` y, si sí, guarda la sugerencia pendiente.
    /// `true` si quedó una nueva sugerencia. Barato: lee la base y puntúa con el router.
    pub async fn evaluate_suggestion(&self, agent_id: AgentId) -> Result<bool, String> {
        let plan = self.plan_suggestion(agent_id)?;
        if plan.close_stale.is_none() && plan.new.is_none() {
            return Ok(false);
        }
        let (stale, new) = (plan.close_stale, plan.new);
        let slot: std::sync::Arc<std::sync::Mutex<bool>> = std::sync::Arc::default();
        let out = slot.clone();
        self.writer
            .write(Box::new(move |t| {
                let now = now_ms();
                if let Some(id) = &stale {
                    // Cerrada sin acción del usuario: ya no aplica.
                    store::resolve(t, id, SuggestionStatus::Kept, now)?;
                }
                if let Some(n) = &new {
                    let inserted = store::insert(t, n, now)?;
                    if let Ok(mut o) = out.lock() {
                        *o = inserted;
                    }
                }
                Ok(())
            }))
            .await
            .map_err(|e| e.to_string())?;
        Ok(slot.lock().map(|b| *b).unwrap_or(false))
    }

    fn plan_suggestion(&self, agent_id: AgentId) -> Result<Plan, String> {
        let none = Plan {
            close_stale: None,
            new: None,
        };
        let conn = self
            .reader
            .lock()
            .map_err(|_| "lector de la base no disponible".to_string())?;
        let agent = repo::get_agent(&conn, agent_id).map_err(|e| e.to_string())?;
        let alive = matches!(
            agent.state,
            AgentState::Running | AgentState::Testing | AgentState::Ready
        );
        let current = repo::runs_of(&conn, agent_id)
            .map_err(|e| e.to_string())?
            .pop()
            .map(|r| r.model_id);
        // Una pendiente que dejó de aplicar (el agente cambió de modelo o ya no está vivo) se
        // cierra; una que sigue vigente se respeta.
        let mut close_stale = None;
        if let Some(p) = store::pending_of(&conn, agent_id).map_err(|e| e.to_string())? {
            let still = alive && current.as_deref() == Some(p.current_model_id.as_str());
            if still {
                return Ok(none);
            }
            close_stale = Some(p.id);
        }
        let stale_only = |close_stale: Option<String>| Plan {
            close_stale,
            new: None,
        };
        let Some(current) = current.filter(|_| alive) else {
            return Ok(stale_only(close_stale));
        };
        if store::is_muted(&conn, agent_id).map_err(|e| e.to_string())? {
            return Ok(stale_only(close_stale));
        }
        // Cómo empezó (los primeros archivos que tocó) y cómo va (el último checkpoint con
        // archivos).
        let summaries: Vec<String> = conn
            .prepare(
                "SELECT summary_json FROM checkpoints WHERE agent_id = ?1
                 AND summary_json IS NOT NULL ORDER BY seq ASC",
            )
            .and_then(|mut s| {
                s.query_map([agent_id.to_string()], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(|e| e.to_string())?;
        let now = summaries.iter().rev().find_map(|s| classify(&files_of(s)));
        // Los eventos `FileModified` solo llegan por hooks y no se podan; sin ellos, el
        // checkpoint más viejo que aún tenga archivos.
        let mut first = first_files(&conn, agent_id).map_err(|e| e.to_string())?;
        if first.is_empty() {
            first = summaries
                .iter()
                .map(|s| files_of(s))
                .find(|f| !f.is_empty())
                .unwrap_or_default();
        }
        let start = classify(&first);
        let (Some(start), Some(now)) = (start, now) else {
            return Ok(stale_only(close_stale));
        };
        let Some(reason) = change_reason(start, now) else {
            return Ok(stale_only(close_stale));
        };
        // ¿Hay un modelo claramente mejor para código en varios archivos?
        let decision = self
            .evaluate(
                &conn,
                &Choose {
                    profile: COMPARE_PROFILE.into(),
                    ..Default::default()
                },
            )
            .map_err(|e| e.to_string())?;
        let Some((suggested, gain, confidence)) = better_model(&current, &decision.evaluated)
        else {
            return Ok(stale_only(close_stale));
        };
        // Lo que el usuario ya dijo «mantener» no se vuelve a proponer.
        if store::was_kept(&conn, agent_id, &suggested).map_err(|e| e.to_string())? {
            return Ok(stale_only(close_stale));
        }
        Ok(Plan {
            close_stale,
            new: Some(NewSuggestion {
                id: symphony_core::ToolCallId::new().to_string(),
                agent_id,
                current_model_id: current,
                suggested_model_id: suggested,
                reason: format!("{reason} (ventaja de puntaje +{gain:.2})"),
                confidence,
            }),
        })
    }

    /// La sugerencia pendiente de un agente como JSON (o `null`). Una que ya no aplica (el
    /// agente cambió de modelo) no se muestra aunque siga en la base hasta el próximo checkpoint.
    pub(crate) fn pending_suggestion_json(&self, agent: AgentId) -> Value {
        let Ok(c) = self.reader.lock() else {
            return Value::Null;
        };
        let pending = store::pending_of(&c, agent).ok().flatten();
        let current = repo::runs_of(&c, agent)
            .ok()
            .and_then(|mut r| r.pop())
            .map(|r| r.model_id);
        match (pending, current) {
            (Some(s), Some(cur)) if s.current_model_id == cur => suggestion_json(&s),
            _ => Value::Null,
        }
    }

    /// Las pendientes de un proyecto.
    pub fn suggestion_list(&self, project: symphony_core::ProjectId) -> Result<Value, String> {
        let c = self
            .reader
            .lock()
            .map_err(|_| "lector de la base no disponible".to_string())?;
        let rows = store::pending_of_project(&c, project).map_err(|e| e.to_string())?;
        Ok(json!({ "suggestions": rows.iter().map(suggestion_json).collect::<Vec<_>>() }))
    }

    /// Resuelve una sugerencia: `switch` cambia al modelo sugerido (y la marca `ACCEPTED`),
    /// `keep` la descarta y `mute` apaga las próximas de ese agente.
    pub async fn suggestion_resolve(&self, id: &str, action: &str) -> Result<Value, String> {
        let (s, current) = {
            let c = self
                .reader
                .lock()
                .map_err(|_| "lector de la base no disponible".to_string())?;
            let s = store::get(&c, id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("no existe la sugerencia `{id}`"))?;
            let current = repo::runs_of(&c, s.agent_id)
                .map_err(|e| e.to_string())?
                .pop()
                .map(|r| r.model_id);
            (s, current)
        };
        if s.status != SuggestionStatus::Pending {
            return Err(format!("la sugerencia ya está {}", s.status.as_str()));
        }
        let status = match action {
            "switch" => SuggestionStatus::Accepted,
            "keep" => SuggestionStatus::Kept,
            "mute" => SuggestionStatus::Muted,
            other => {
                return Err(format!(
                    "acción desconocida `{other}` (switch, keep o mute)"
                ));
            }
        };
        // Cambiar desde un modelo que ya no es el actual no tiene sentido: se cierra y se dice
        // por qué.
        if status == SuggestionStatus::Accepted
            && current.as_deref() != Some(s.current_model_id.as_str())
        {
            let sid = id.to_string();
            let _ = self
                .writer
                .write(Box::new(move |t| {
                    store::resolve(t, &sid, SuggestionStatus::Kept, now_ms())?;
                    Ok(())
                }))
                .await;
            return Err(
                "la sugerencia ya no aplica: el agente cambió de modelo desde que se propuso"
                    .into(),
            );
        }
        // Primero el cambio (puede fallar); solo si se hizo se marca aceptada.
        let mut switched: Option<String> = None;
        if status == SuggestionStatus::Accepted {
            let run = self
                .switch(s.agent_id, &s.suggested_model_id)
                .await
                .map_err(|e| e.0)?;
            switched = Some(run.to_string());
        }
        let sid = id.to_string();
        self.writer
            .write(Box::new(move |t| {
                store::resolve(t, &sid, status, now_ms())?;
                Ok(())
            }))
            .await
            .map_err(|e| e.to_string())?;
        Ok(json!({ "id": id, "status": status.as_str(), "run_id": switched }))
    }
}

fn suggestion_json(s: &store::Suggestion) -> Value {
    json!({
        "id": s.id,
        "agent_id": s.agent_id.to_string(),
        "current_model_id": s.current_model_id,
        "suggested_model_id": s.suggested_model_id,
        "reason": s.reason,
        "confidence": s.confidence,
        "status": s.status.as_str(),
        "created_at": s.created_at,
    })
}
