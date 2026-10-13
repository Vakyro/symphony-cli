//! Sugerencias de modelo en el daemon (P14.S2, FLOW §8.5). **Nunca cambia el modelo por sí
//! solo**: detecta, propone y espera al usuario (`[Switch] [Keep] [Don't suggest again]`).
//!
//! Se evalúa tras cada checkpoint del agente (una vez por checkpoint, solo si el agente no tiene
//! ya una sugerencia pendiente ni la apagó; CONSTRAINTS A11) con datos que ya están en la base.

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

/// Arranca el evaluador. Termina con `shutdown`.
pub fn spawn(runtime: Runtime, shutdown: CancellationToken) -> tokio::task::JoinHandle<()> {
    let mut taken = runtime.bus.checkpoints_taken();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                got = taken.recv() => match got {
                    Ok(agent) => {
                        if let Err(e) = runtime.evaluate_suggestion(agent) {
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

impl Runtime {
    /// Evalúa si conviene sugerir otro modelo a `agent` y, si sí, guarda la sugerencia pendiente.
    /// Síncrono y barato: lee la base y puntúa con el router (microsegundos).
    pub fn evaluate_suggestion(&self, agent_id: AgentId) -> Result<bool, String> {
        let conn = self
            .reader
            .lock()
            .map_err(|_| "lector de la base no disponible".to_string())?;
        let agent = repo::get_agent(&conn, agent_id).map_err(|e| e.to_string())?;
        if !matches!(
            agent.state,
            AgentState::Running | AgentState::Testing | AgentState::Ready
        ) {
            return Ok(false);
        }
        if store::is_muted(&conn, agent_id).map_err(|e| e.to_string())?
            || store::pending_of(&conn, agent_id)
                .map_err(|e| e.to_string())?
                .is_some()
        {
            return Ok(false);
        }
        let Some(current) = repo::runs_of(&conn, agent_id)
            .map_err(|e| e.to_string())?
            .pop()
            .map(|r| r.model_id)
        else {
            return Ok(false);
        };
        // Cómo empezó (el checkpoint más viejo con archivos) y cómo va (el último).
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
        let start = summaries
            .iter()
            .map(|s| classify(&files_of(s)))
            .find(Option::is_some)
            .flatten();
        // «Cómo va»: el último checkpoint que lleva archivos (tras el commit del turno puede haber
        // uno vacío).
        let now = summaries.iter().rev().find_map(|s| classify(&files_of(s)));
        let (Some(start), Some(now)) = (start, now) else {
            return Ok(false);
        };
        let Some(reason) = change_reason(start, now) else {
            return Ok(false);
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
            return Ok(false);
        };
        let new = NewSuggestion {
            id: symphony_core::ToolCallId::new().to_string(),
            agent_id,
            current_model_id: current,
            suggested_model_id: suggested,
            reason: format!("{reason} (ventaja de puntaje +{gain:.2})"),
            confidence,
        };
        drop(conn);
        // La escritura va por el writer único; el resultado se lee al refrescar la vista.
        let writer = self.writer.clone();
        tokio::spawn(async move {
            let _ = writer
                .write(Box::new(move |t| {
                    store::insert(t, &new, now_ms())?;
                    Ok(())
                }))
                .await;
        });
        Ok(true)
    }

    /// La sugerencia pendiente de un agente como JSON (o `null`).
    pub(crate) fn pending_suggestion_json(&self, agent: AgentId) -> Value {
        self.reader
            .lock()
            .ok()
            .and_then(|c| store::pending_of(&c, agent).ok().flatten())
            .map_or(Value::Null, |s| suggestion_json(&s))
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
        let s = self
            .reader
            .lock()
            .map_err(|_| "lector de la base no disponible".to_string())
            .and_then(|c| {
                store::get(&c, id)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("no existe la sugerencia `{id}`"))
            })?;
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
