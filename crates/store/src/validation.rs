//! Resultados de validación (DB §3.H, P08.S6): una `validation_run` por pasada de un tier y un
//! `validation_check` por comando.

use rusqlite::{Connection, params};
use symphony_core::{AgentId, CheckName, CheckStatus, ValidationStatus, ValidationTrigger};

use crate::repo::RepoError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewValidationRun {
    pub id: String,
    pub agent_id: AgentId,
    pub run_id: Option<symphony_core::RunId>,
    pub tier: u8,
    pub trigger: ValidationTrigger,
}

pub fn insert_run(conn: &Connection, r: &NewValidationRun, now: i64) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO validation_runs (id, agent_id, run_id, tier, trigger, status, started_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'RUNNING', ?6)",
        params![
            r.id,
            r.agent_id.to_string(),
            r.run_id.map(|x| x.to_string()),
            r.tier,
            r.trigger.as_str(),
            now
        ],
    )?;
    Ok(())
}

pub fn finish_run(
    conn: &Connection,
    id: &str,
    status: ValidationStatus,
    now: i64,
) -> Result<(), RepoError> {
    conn.execute(
        "UPDATE validation_runs SET status = ?2, finished_at = ?3 WHERE id = ?1",
        params![id, status.as_str(), now],
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCheck {
    pub id: String,
    pub validation_run_id: String,
    pub name: CheckName,
    pub status: CheckStatus,
    pub passed: Option<u32>,
    pub failed: Option<u32>,
    pub summary: Option<String>,
    pub output_object_id: Option<symphony_core::ContextObjectId>,
}

pub fn insert_check(conn: &Connection, c: &NewCheck) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO validation_checks
            (id, validation_run_id, name, status, passed_count, failed_count, summary, output_object_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            c.id,
            c.validation_run_id,
            c.name.as_str(),
            c.status.as_str(),
            c.passed,
            c.failed,
            c.summary,
            c.output_object_id.map(|o| o.to_string())
        ],
    )?;
    Ok(())
}

/// Validaciones fallidas del agente (para el límite de intentos).
pub fn failed_runs(conn: &Connection, agent: AgentId) -> Result<i64, RepoError> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM validation_runs WHERE agent_id = ?1 AND status = 'FAIL'",
        [agent.to_string()],
        |r| r.get(0),
    )?)
}

/// Un check ya resuelto, para vistas e IPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRow {
    pub name: String,
    pub status: String,
    pub passed: Option<i64>,
    pub failed: Option<i64>,
    pub summary: Option<String>,
    pub output_object_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    pub id: String,
    pub tier: i64,
    pub trigger: String,
    pub status: String,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub checks: Vec<CheckRow>,
}

/// Las pasadas más recientes del agente (la más nueva primero), con sus checks.
pub fn runs_of(conn: &Connection, agent: AgentId, limit: u32) -> Result<Vec<RunRow>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT id, tier, trigger, status, started_at, finished_at FROM validation_runs
         WHERE agent_id = ?1 ORDER BY started_at DESC, rowid DESC LIMIT ?2",
    )?;
    let mut runs: Vec<RunRow> = stmt
        .query_map(params![agent.to_string(), limit], |r| {
            Ok(RunRow {
                id: r.get(0)?,
                tier: r.get(1)?,
                trigger: r.get(2)?,
                status: r.get(3)?,
                started_at: r.get(4)?,
                finished_at: r.get(5)?,
                checks: Vec::new(),
            })
        })?
        .collect::<Result<_, _>>()?;
    let mut checks = conn.prepare(
        "SELECT name, status, passed_count, failed_count, summary, output_object_id
         FROM validation_checks WHERE validation_run_id = ?1 ORDER BY rowid",
    )?;
    for run in &mut runs {
        run.checks = checks
            .query_map([&run.id], |r| {
                Ok(CheckRow {
                    name: r.get(0)?,
                    status: r.get(1)?,
                    passed: r.get(2)?,
                    failed: r.get(3)?,
                    summary: r.get(4)?,
                    output_object_id: r.get(5)?,
                })
            })?
            .collect::<Result<_, _>>()?;
    }
    Ok(runs)
}
