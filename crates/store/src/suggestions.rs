//! Sugerencias de modelo y desempeño aprendido (P14, DB §3.E).
//!
//! Una sugerencia nunca cambia nada por sí sola: es una fila `PENDING` que el usuario resuelve
//! (`ACCEPTED`, `KEPT`, `MUTED`). Hay a lo sumo una pendiente por agente, y `MUTED` apaga las
//! siguientes de ese agente.

use rusqlite::{Connection, OptionalExtension, params};
use symphony_core::{AgentId, SuggestionStatus, learning};

use crate::repo::RepoError;

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub id: String,
    pub agent_id: AgentId,
    pub current_model_id: String,
    pub suggested_model_id: String,
    pub reason: String,
    pub confidence: Option<f64>,
    pub status: SuggestionStatus,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
}

const COLS: &str = "id, agent_id, current_model_id, suggested_model_id, reason, confidence, status, created_at, resolved_at";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Suggestion> {
    let conv = |i: usize, e: Box<dyn std::error::Error + Send + Sync>| {
        rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, e)
    };
    let (agent, status): (String, String) = (r.get(1)?, r.get(6)?);
    Ok(Suggestion {
        id: r.get(0)?,
        agent_id: agent.parse().map_err(|e| conv(1, Box::new(e)))?,
        current_model_id: r.get(2)?,
        suggested_model_id: r.get(3)?,
        reason: r.get(4)?,
        confidence: r.get(5)?,
        status: status.parse().map_err(|e| conv(6, Box::new(e)))?,
        created_at: r.get(7)?,
        resolved_at: r.get(8)?,
    })
}

#[derive(Debug, Clone)]
pub struct NewSuggestion {
    pub id: String,
    pub agent_id: AgentId,
    pub current_model_id: String,
    pub suggested_model_id: String,
    pub reason: String,
    pub confidence: f64,
}

/// Registra una sugerencia pendiente. `false` si no entró: ya hay una pendiente para el agente
/// o el usuario apagó las sugerencias de ese agente.
pub fn insert(conn: &Connection, s: &NewSuggestion, now: i64) -> Result<bool, RepoError> {
    if is_muted(conn, s.agent_id)? {
        return Ok(false);
    }
    let n = conn.execute(
        "INSERT OR IGNORE INTO model_suggestions
           (id, agent_id, current_model_id, suggested_model_id, reason, confidence, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'PENDING', ?7)",
        params![
            s.id,
            s.agent_id.to_string(),
            s.current_model_id,
            s.suggested_model_id,
            s.reason,
            s.confidence.clamp(0.0, 1.0),
            now
        ],
    )?;
    Ok(n == 1)
}

/// ¿El usuario pidió no recibir más sugerencias para este agente?
pub fn is_muted(conn: &Connection, agent: AgentId) -> Result<bool, RepoError> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM model_suggestions WHERE agent_id = ?1 AND status = 'MUTED')",
        [agent.to_string()],
        |r| r.get(0),
    )?)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Suggestion>, RepoError> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} FROM model_suggestions WHERE id = ?1"),
            [id],
            row,
        )
        .optional()?)
}

/// La sugerencia pendiente de un agente, si hay.
pub fn pending_of(conn: &Connection, agent: AgentId) -> Result<Option<Suggestion>, RepoError> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {COLS} FROM model_suggestions WHERE agent_id = ?1 AND status = 'PENDING'"
            ),
            [agent.to_string()],
            row,
        )
        .optional()?)
}

/// Las pendientes de un proyecto (más nuevas primero).
pub fn pending_of_project(
    conn: &Connection,
    project: symphony_core::ProjectId,
) -> Result<Vec<Suggestion>, RepoError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM model_suggestions s JOIN agents a ON a.id = s.agent_id
         WHERE a.project_id = ?1 AND s.status = 'PENDING' ORDER BY s.created_at DESC, s.rowid DESC",
        COLS.split(", ")
            .map(|c| format!("s.{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    ))?;
    let rows = stmt.query_map([project.to_string()], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Resuelve una sugerencia pendiente. `false` si no estaba pendiente.
pub fn resolve(
    conn: &Connection,
    id: &str,
    status: SuggestionStatus,
    now: i64,
) -> Result<bool, RepoError> {
    if status == SuggestionStatus::Pending {
        return Ok(false);
    }
    let n = conn.execute(
        "UPDATE model_suggestions SET status = ?2, resolved_at = ?3 WHERE id = ?1 AND status = 'PENDING'",
        params![id, status.as_str(), now],
    )?;
    Ok(n == 1)
}

/// Anota el resultado de un run bajo `profile`: suma una muestra y recalcula el ajuste aprendido
/// (que solo existe con las muestras mínimas; antes queda `NULL`).
pub fn record_outcome(
    conn: &Connection,
    profile: &str,
    model: &str,
    outcome: learning::Outcome,
) -> Result<(), RepoError> {
    conn.execute(
        "UPDATE profile_models
         SET sample_count = sample_count + 1, outcome_score_sum = outcome_score_sum + ?3
         WHERE profile_id = ?1 AND model_id = ?2",
        params![profile, model, outcome.score()],
    )?;
    let counts: Option<(i64, f64)> = conn
        .query_row(
            "SELECT sample_count, outcome_score_sum FROM profile_models WHERE profile_id = ?1 AND model_id = ?2",
            params![profile, model],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((n, sum)) = counts {
        conn.execute(
            "UPDATE profile_models SET learned_adjustment = ?3 WHERE profile_id = ?1 AND model_id = ?2",
            params![profile, model, learning::learned_adjustment(n, sum)],
        )?;
    }
    Ok(())
}
