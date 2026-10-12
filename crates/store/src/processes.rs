//! Árbol de procesos observado en la capa OS (DB §3.C `processes`, P08.S4). El daemon
//! toma una foto cada pocos segundos; aquí se reconcilia contra lo ya guardado.

use std::collections::HashMap;

use rusqlite::{Connection, params};
use symphony_core::{ProcessId, RunId};

use crate::repo::RepoError;

/// Un proceso visto en una foto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcSample {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub command: String,
    pub rss_bytes: u64,
    pub cpu_time_ms: u64,
}

/// Fila de `processes`, para las vistas y los tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRow {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub command: Option<String>,
    pub peak_rss_bytes: i64,
    pub cpu_time_ms: i64,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

fn clamp(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// Reconcilia la foto con las filas abiertas del run:
/// - un pid nuevo (o con otro comando: el pid se reutilizó) abre una fila;
/// - uno conocido sube su pico de memoria y su tiempo de CPU;
/// - uno que ya no está cierra su fila (`ended_at`).
///
/// Si exactamente una `tool_call` del run está `RUNNING`, los procesos nuevos se le atribuyen.
pub fn sync_processes(
    conn: &Connection,
    run: RunId,
    snapshot: &[ProcSample],
    now: i64,
) -> Result<(), RepoError> {
    let run_s = run.to_string();
    let mut open: HashMap<u32, (String, Option<String>)> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, pid, command FROM processes WHERE run_id = ?1 AND ended_at IS NULL",
        )?;
        let rows = stmt.query_map([&run_s], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get(2)?))
        })?;
        for row in rows {
            let (id, pid, command) = row?;
            if let Ok(pid) = u32::try_from(pid) {
                open.insert(pid, (id, command));
            }
        }
    }
    let attributed: Option<String> = conn
        .query_row(
            "SELECT CASE WHEN COUNT(*) = 1 THEN MIN(id) END FROM tool_calls
             WHERE run_id = ?1 AND status = 'RUNNING'",
            [&run_s],
            |r| r.get(0),
        )
        .unwrap_or(None);

    let mut ids: HashMap<u32, String> = HashMap::new();
    let mut seen: Vec<u32> = Vec::new();
    for p in snapshot {
        let known = open
            .get(&p.pid)
            .filter(|(_, cmd)| cmd.as_deref() == Some(p.command.as_str()))
            .map(|(id, _)| id.clone());
        let id = match known {
            Some(id) => {
                conn.execute(
                    "UPDATE processes SET peak_rss_bytes = MAX(COALESCE(peak_rss_bytes, 0), ?2),
                                          cpu_time_ms = MAX(COALESCE(cpu_time_ms, 0), ?3)
                     WHERE id = ?1",
                    params![id, clamp(p.rss_bytes), clamp(p.cpu_time_ms)],
                )?;
                id
            }
            None => {
                // El pid se reutilizó con otro comando: la fila vieja se cierra.
                if let Some((old, _)) = open.get(&p.pid) {
                    conn.execute(
                        "UPDATE processes SET ended_at = ?2 WHERE id = ?1",
                        params![old, now],
                    )?;
                }
                let id = ProcessId::new().to_string();
                conn.execute(
                    "INSERT INTO processes (id, run_id, tool_call_id, pid, parent_process_id, command,
                                            peak_rss_bytes, cpu_time_ms, started_at)
                     VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8)",
                    params![
                        id,
                        run_s,
                        attributed,
                        p.pid,
                        p.command,
                        clamp(p.rss_bytes),
                        clamp(p.cpu_time_ms),
                        now
                    ],
                )?;
                id
            }
        };
        ids.insert(p.pid, id);
        seen.push(p.pid);
    }
    // Los padres se enlazan al final: en la foto pueden venir después que sus hijos.
    for p in snapshot {
        if let (Some(id), Some(parent)) =
            (ids.get(&p.pid), p.parent_pid.and_then(|pp| ids.get(&pp)))
        {
            conn.execute(
                "UPDATE processes SET parent_process_id = ?2 WHERE id = ?1 AND parent_process_id IS NULL",
                params![id, parent],
            )?;
        }
    }
    for (pid, (id, _)) in &open {
        if !seen.contains(pid) {
            conn.execute(
                "UPDATE processes SET ended_at = ?2 WHERE id = ?1 AND ended_at IS NULL",
                params![id, now],
            )?;
        }
    }
    Ok(())
}

/// El run terminó: todo lo que seguía abierto se cierra.
pub fn close_processes(conn: &Connection, run: RunId, now: i64) -> Result<(), RepoError> {
    conn.execute(
        "UPDATE processes SET ended_at = ?2 WHERE run_id = ?1 AND ended_at IS NULL",
        params![run.to_string(), now],
    )?;
    Ok(())
}

pub fn processes_of(conn: &Connection, run: RunId) -> Result<Vec<ProcessRow>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT c.pid, p.pid, c.command, COALESCE(c.peak_rss_bytes, 0), COALESCE(c.cpu_time_ms, 0),
                c.started_at, c.ended_at
         FROM processes c LEFT JOIN processes p ON p.id = c.parent_process_id
         WHERE c.run_id = ?1 ORDER BY c.started_at, c.pid",
    )?;
    let rows = stmt.query_map([run.to_string()], |r| {
        Ok(ProcessRow {
            pid: r.get::<_, i64>(0).map(|p| u32::try_from(p).unwrap_or(0))?,
            parent_pid: r
                .get::<_, Option<i64>>(1)?
                .and_then(|p| u32::try_from(p).ok()),
            command: r.get(2)?,
            peak_rss_bytes: r.get(3)?,
            cpu_time_ms: r.get(4)?,
            started_at: r.get(5)?,
            ended_at: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
