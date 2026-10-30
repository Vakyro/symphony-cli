//! Consultas de solo lectura para las vistas de la TUI (FLOW §5–§7, §8, §16).
//! La TUI nunca abre la base: todo lo que muestra sale de aquí por IPC.

use rusqlite::{Connection, params};
use serde_json::{Value, json};

/// Modelos exactos para el Model Picker (vista 13). `available` = proveedor
/// `READY` y habilitado, y modelo habilitado.
/// Modelos para el Model Picker (FLOW §8.1), con la salud vigente de su proveedor.
pub fn models(conn: &Connection, now: i64) -> rusqlite::Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.display_name, p.id, p.display_name, p.setup_state,
                p.enabled = 1 AND m.enabled = 1 AND p.setup_state = 'READY'
         FROM models m JOIN providers p ON p.id = m.provider_id
         ORDER BY p.display_name, m.display_name",
    )?;
    let rows: Vec<(String, Value)> = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(2)?,
                json!({
                    "id": r.get::<_, String>(0)?,
                    "display_name": r.get::<_, String>(1)?,
                    "provider_id": r.get::<_, String>(2)?,
                    "provider": r.get::<_, String>(3)?,
                    "setup_state": r.get::<_, String>(4)?,
                    "available": r.get::<_, bool>(5)?,
                }),
            ))
        })?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::with_capacity(rows.len());
    for (provider, mut m) in rows {
        // Disponible ≠ sano: un modelo con la cuota baja se puede elegir a mano, pero se ve.
        m["health"] = json!(
            symphony_store::health::get_health(conn, &provider, None)
                .map_err(rusqlite::Error::from)?
                .map_or("UNKNOWN", |h| h.effective_state(now).as_str())
        );
        out.push(m);
    }
    Ok(Value::Array(out))
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

/// Comandos del agente con salida, derivados de `tool_calls` (P17.S5).
pub fn processes(
    conn: &Connection,
    agent: &str,
    objects: &symphony_object_store::ObjectStore,
) -> rusqlite::Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT tc.id, tc.run_id, tc.tool_name, tc.command, tc.status, tc.exit_code,
                tc.requested_at, tc.started_at, tc.finished_at, co.blob_hash
         FROM tool_calls tc LEFT JOIN context_objects co ON co.id = tc.output_object_id
         WHERE tc.agent_id = ?1 AND lower(tc.tool_name) IN ('bash','exec_command','shell','run_command') ORDER BY tc.requested_at",
    )?;
    let rows = stmt.query_map([agent], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<i64>>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, Option<i64>>(7)?,
            r.get::<_, Option<i64>>(8)?,
            r.get::<_, Option<String>>(9)?,
        ))
    })?;
    rows.map(|row| {
        let (
            id,
            run_id,
            tool,
            command,
            status,
            exit_code,
            requested_at,
            started_at,
            finished_at,
            hash,
        ) = row?;
        let output = hash
            .map(|h| objects.get(&h))
            .transpose()
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
            .map(|b| String::from_utf8_lossy(&b).into_owned());
        Ok(
            json!({"id":id,"run_id":run_id,"tool":tool,"command":command,"status":status,
                  "exit_code":exit_code,"requested_at":requested_at,"started_at":started_at,
                  "finished_at":finished_at,"output":output}),
        )
    })
    .collect()
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

/// Task List (15): las tareas del proyecto con su agente, lo que esperan y su integración.
pub fn tasks(conn: &Connection, project: &str) -> rusqlite::Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.code, t.title, t.kind, t.status, t.status_reason, t.priority,
                t.milestone_id, m.name,
                a.id, a.number, a.state,
                (SELECT mr.status FROM merge_requests mr WHERE mr.agent_id = a.id
                 ORDER BY mr.created_at DESC, mr.rowid DESC LIMIT 1)
         FROM tasks t
         LEFT JOIN milestones m ON m.id = t.milestone_id
         LEFT JOIN agents a ON a.id = (
             SELECT id FROM agents WHERE task_id = t.id ORDER BY created_at DESC, rowid DESC LIMIT 1)
         WHERE t.project_id = ?1 AND t.code <> 'CHAT'
         ORDER BY t.rowid",
    )?;
    let rows = stmt.query_map([project], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, Option<String>>(7)?,
            r.get::<_, Option<String>>(8)?,
            r.get::<_, Option<String>>(9)?,
            r.get::<_, Option<i64>>(10)?,
            r.get::<_, Option<String>>(11)?,
            r.get::<_, Option<String>>(12)?,
        ))
    })?;
    let mut unmet = conn.prepare(
        "SELECT d.code FROM task_dependencies td JOIN tasks d ON d.id = td.depends_on_task_id
         WHERE td.task_id = ?1 AND d.status <> 'DONE' ORDER BY d.code",
    )?;
    let mut out = Vec::new();
    for row in rows {
        let (
            id,
            code,
            title,
            kind,
            status,
            reason,
            priority,
            ms_id,
            ms_name,
            agent,
            number,
            state,
            merge,
        ) = row?;
        let waits: Vec<String> = unmet
            .query_map([&id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        out.push(json!({
            "task_id": id,
            "code": code,
            "title": title,
            "kind": kind,
            "status": status,
            "status_reason": reason,
            "priority": priority,
            "milestone_id": ms_id,
            "milestone": ms_name,
            "agent_id": agent,
            "agent_number": number,
            "agent_state": state,
            "merge_status": merge,
            "waiting_for": waits,
        }));
    }
    Ok(json!({ "tasks": out }))
}
