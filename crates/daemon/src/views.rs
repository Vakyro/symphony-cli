//! Consultas de solo lectura para las vistas de la TUI (FLOW §5–§7, §8, §16).
//! La TUI nunca abre la base: todo lo que muestra sale de aquí por IPC.

use rusqlite::{Connection, params};
use serde_json::{Value, json};

/// Modelos exactos para el Model Picker (vista 13). `available` = proveedor
/// `READY` y habilitado, y modelo habilitado.
pub fn models(conn: &Connection) -> rusqlite::Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.display_name, p.id, p.display_name, p.setup_state,
                p.enabled = 1 AND m.enabled = 1 AND p.setup_state = 'READY'
         FROM models m JOIN providers p ON p.id = m.provider_id
         ORDER BY p.display_name, m.display_name",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(json!({
            "id": r.get::<_, String>(0)?,
            "display_name": r.get::<_, String>(1)?,
            "provider_id": r.get::<_, String>(2)?,
            "provider": r.get::<_, String>(3)?,
            "setup_state": r.get::<_, String>(4)?,
            "available": r.get::<_, bool>(5)?,
        }))
    })?;
    Ok(Value::Array(rows.collect::<Result<_, _>>()?))
}

/// Timeline de la vista Activity (08): tool calls y checkpoints, lo más nuevo primero.
pub fn activity(conn: &Connection, agent: &str, limit: u32) -> rusqlite::Result<Value> {
    let mut items: Vec<(i64, Value)> = Vec::new();
    let mut tools = conn.prepare(
        "SELECT tool_name, command, status, exit_code, requested_at, finished_at
         FROM tool_calls WHERE agent_id = ?1 ORDER BY requested_at DESC LIMIT ?2",
    )?;
    for row in tools.query_map(params![agent, limit], |r| {
        let at: i64 = r.get(4)?;
        Ok((
            at,
            json!({
                "kind": "tool",
                "at": at,
                "tool": r.get::<_, String>(0)?,
                "command": r.get::<_, Option<String>>(1)?,
                "status": r.get::<_, String>(2)?,
                "exit_code": r.get::<_, Option<i64>>(3)?,
                "finished_at": r.get::<_, Option<i64>>(5)?,
            }),
        ))
    })? {
        items.push(row?);
    }
    let mut cps = conn.prepare(
        "SELECT seq, created_at, current_step, next_step
         FROM checkpoints WHERE agent_id = ?1 ORDER BY seq DESC LIMIT ?2",
    )?;
    for row in cps.query_map(params![agent, limit], |r| {
        let at: i64 = r.get(1)?;
        Ok((
            at,
            json!({
                "kind": "checkpoint",
                "at": at,
                "seq": r.get::<_, i64>(0)?,
                "current_step": r.get::<_, Option<String>>(2)?,
                "next_step": r.get::<_, Option<String>>(3)?,
            }),
        ))
    })? {
        items.push(row?);
    }
    items.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    items.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    Ok(Value::Array(items.into_iter().map(|(_, v)| v).collect()))
}

/// Vista History (12): runs del agente y cambios de executor con su motivo.
pub fn history(conn: &Connection, agent: &str) -> rusqlite::Result<Value> {
    let mut runs = conn.prepare(
        "SELECT seq, provider_id, model_id, status, end_reason, started_at, ended_at
         FROM agent_runs WHERE agent_id = ?1 ORDER BY seq",
    )?;
    let runs: Vec<Value> = runs
        .query_map([agent], |r| {
            Ok(json!({
                "seq": r.get::<_, i64>(0)?,
                "provider_id": r.get::<_, String>(1)?,
                "model_id": r.get::<_, String>(2)?,
                "status": r.get::<_, String>(3)?,
                "end_reason": r.get::<_, Option<String>>(4)?,
                "started_at": r.get::<_, i64>(5)?,
                "ended_at": r.get::<_, Option<i64>>(6)?,
            }))
        })?
        .collect::<Result<_, _>>()?;
    let mut changes = conn.prepare(
        "SELECT ec.reason, ec.occurred_at, ec.checkpoint_age_ms, fr.model_id, tr.model_id, pf.failure_type
         FROM executor_changes ec
         JOIN agent_runs fr ON fr.id = ec.from_run_id
         LEFT JOIN agent_runs tr ON tr.id = ec.to_run_id
         LEFT JOIN provider_failures pf ON pf.id = ec.failure_id
         WHERE ec.agent_id = ?1 ORDER BY ec.occurred_at",
    )?;
    let changes: Vec<Value> = changes
        .query_map([agent], |r| {
            Ok(json!({
                "reason": r.get::<_, String>(0)?,
                "at": r.get::<_, i64>(1)?,
                "checkpoint_age_ms": r.get::<_, Option<i64>>(2)?,
                "from_model": r.get::<_, String>(3)?,
                "to_model": r.get::<_, Option<String>>(4)?,
                "failure": r.get::<_, Option<String>>(5)?,
            }))
        })?
        .collect::<Result<_, _>>()?;
    Ok(json!({ "runs": runs, "changes": changes }))
}

/// Recovery Center (30): problemas abiertos, del proyecto o de todos.
pub fn recovery(conn: &Connection, project: Option<&str>) -> rusqlite::Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.kind, r.detail, r.created_at, r.agent_id, a.number, a.state
         FROM recovery_items r LEFT JOIN agents a ON a.id = r.agent_id
         WHERE r.status = 'OPEN' AND (?1 IS NULL OR r.project_id = ?1)
         ORDER BY r.created_at DESC",
    )?;
    let rows = stmt.query_map([project], |r| {
        Ok(json!({
            "id": r.get::<_, String>(0)?,
            "kind": r.get::<_, String>(1)?,
            "detail": r.get::<_, String>(2)?,
            "created_at": r.get::<_, i64>(3)?,
            "agent_id": r.get::<_, Option<String>>(4)?,
            "agent_number": r.get::<_, Option<i64>>(5)?,
            "agent_state": r.get::<_, Option<String>>(6)?,
        }))
    })?;
    Ok(Value::Array(rows.collect::<Result<_, _>>()?))
}

/// `(agent_id, kind)` de un item abierto del Recovery Center.
pub fn recovery_item(
    conn: &Connection,
    id: &str,
) -> rusqlite::Result<Option<(Option<String>, String)>> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT agent_id, kind FROM recovery_items WHERE id = ?1 AND status = 'OPEN'",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}
