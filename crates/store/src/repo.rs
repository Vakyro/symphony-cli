//! Repositorios de Fase 1: SQL escrito a mano, sin ORM (STACK §43).
//! Las lecturas reciben una `&Connection`; las escrituras se hacen dentro de
//! `WriterHandle::write` (una `Transaction` es una `Connection`).
//!
//! AGENT ≠ MODEL: el modelo de un agente se consulta en su run abierto
//! (`current_run`), nunca en `agents`.

use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, Row, params};
use symphony_core::{
    AgentId, AgentState, ContextMode, ExecutionMode, FailoverPolicy, InvalidTransition, ProjectId,
    RunEndReason, RunId, RunStatus, SessionId, TaskId, TaskStatus, WorktreeId,
};

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("no existe {entity} `{id}`")]
    NotFound { entity: &'static str, id: String },
    #[error(transparent)]
    Transition(#[from] InvalidTransition),
    #[error("el estado {state} necesita una razón legible (FLOW §7)")]
    MissingReason { state: &'static str },
}

impl From<RepoError> for rusqlite::Error {
    /// Para poder usar `?` dentro de `WriterHandle::write`.
    fn from(e: RepoError) -> Self {
        match e {
            RepoError::Sqlite(e) => e,
            other => rusqlite::Error::ToSqlConversionFailure(Box::new(other)),
        }
    }
}

/// Lee una columna TEXT y la convierte con `FromStr` (enums e IDs de `core`).
fn col<T>(row: &Row<'_>, idx: usize) -> rusqlite::Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let text: String = row.get(idx)?;
    text.parse().map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn opt_col<T>(row: &Row<'_>, idx: usize) -> rusqlite::Result<Option<T>>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match row.get::<_, Option<String>>(idx)? {
        None => Ok(None),
        Some(text) => text.parse().map(Some).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
        }),
    }
}

fn not_found(entity: &'static str, id: impl ToString) -> impl FnOnce() -> RepoError {
    move || RepoError::NotFound {
        entity,
        id: id.to_string(),
    }
}

// --- projects ---------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub root_path: String,
    pub default_branch: String,
    pub created_at: i64,
}

pub fn insert_project(conn: &Connection, p: &Project) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![p.id.to_string(), p.name, p.root_path, p.default_branch, p.created_at],
    )?;
    Ok(())
}

fn project_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: col(row, 0)?,
        name: row.get(1)?,
        root_path: row.get(2)?,
        default_branch: row.get(3)?,
        created_at: row.get(4)?,
    })
}

pub fn project_by_root(conn: &Connection, root_path: &str) -> Result<Option<Project>, RepoError> {
    Ok(conn
        .query_row(
            "SELECT id, name, root_path, default_branch, created_at FROM projects WHERE root_path = ?1 AND archived_at IS NULL",
            [root_path],
            project_row,
        )
        .optional()?)
}

pub fn get_project(conn: &Connection, id: ProjectId) -> Result<Project, RepoError> {
    conn.query_row(
        "SELECT id, name, root_path, default_branch, created_at FROM projects WHERE id = ?1",
        [id.to_string()],
        project_row,
    )
    .optional()?
    .ok_or_else(not_found("project", id))
}

// --- sessions ---------------------------------------------------------------

/// Valores de `sessions.status` (DB §3.A).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Active,
    Closed,
    Interrupted,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Closed => "CLOSED",
            Self::Interrupted => "INTERRUPTED",
        }
    }
}

pub fn start_session(
    conn: &Connection,
    id: SessionId,
    project: ProjectId,
    daemon_pid: u32,
    now: i64,
) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO sessions (id, project_id, status, daemon_pid, started_at) VALUES (?1, ?2, 'ACTIVE', ?3, ?4)",
        params![id.to_string(), project.to_string(), daemon_pid, now],
    )?;
    Ok(())
}

pub fn end_session(
    conn: &Connection,
    id: SessionId,
    status: SessionStatus,
    now: i64,
) -> Result<(), RepoError> {
    let n = conn.execute(
        "UPDATE sessions SET status = ?2, ended_at = ?3 WHERE id = ?1 AND ended_at IS NULL",
        params![id.to_string(), status.as_str(), now],
    )?;
    if n == 0 {
        return Err(not_found("sesión abierta", id)());
    }
    Ok(())
}

/// Sesiones que nunca se cerraron (Journey E: el daemon murió). Recovery las marca `INTERRUPTED`.
pub fn unclosed_sessions(
    conn: &Connection,
    project: ProjectId,
) -> Result<Vec<(SessionId, Option<u32>)>, RepoError> {
    let mut stmt = conn.prepare("SELECT id, daemon_pid FROM sessions WHERE project_id = ?1 AND ended_at IS NULL ORDER BY started_at")?;
    let rows = stmt.query_map([project.to_string()], |r| Ok((col(r, 0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

// --- tasks ------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub id: TaskId,
    pub project_id: ProjectId,
    pub code: String,
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub status_reason: Option<String>,
    pub priority: i64,
}

pub fn insert_task(conn: &Connection, t: &Task, now: i64) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO tasks (id, project_id, code, kind, title, description, status, status_reason, priority, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'WORK', ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![
            t.id.to_string(),
            t.project_id.to_string(),
            t.code,
            t.title,
            t.description,
            t.status.as_str(),
            t.status_reason,
            t.priority,
            now
        ],
    )?;
    Ok(())
}

pub fn get_task(conn: &Connection, id: TaskId) -> Result<Task, RepoError> {
    conn.query_row(
        "SELECT id, project_id, code, title, description, status, status_reason, priority FROM tasks WHERE id = ?1",
        [id.to_string()],
        |r| {
            Ok(Task {
                id: col(r, 0)?,
                project_id: col(r, 1)?,
                code: r.get(2)?,
                title: r.get(3)?,
                description: r.get(4)?,
                status: col(r, 5)?,
                status_reason: r.get(6)?,
                priority: r.get(7)?,
            })
        },
    )
    .optional()?
    .ok_or_else(not_found("task", id))
}

/// Siguiente código visible `T-N` del proyecto.
pub fn next_task_code(conn: &Connection, project: ProjectId) -> Result<String, RepoError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project_id = ?1",
        [project.to_string()],
        |r| r.get(0),
    )?;
    Ok(format!("T-{}", n + 1))
}

/// Cambia el estado validando la transición (`core`). Devuelve el estado anterior.
pub fn set_task_status(
    conn: &Connection,
    id: TaskId,
    to: TaskStatus,
    reason: Option<&str>,
    now: i64,
) -> Result<TaskStatus, RepoError> {
    let from = get_task(conn, id)?.status;
    from.transition(to)?;
    let completed = (to == TaskStatus::Done).then_some(now);
    conn.execute(
        "UPDATE tasks SET status = ?2, status_reason = ?3, updated_at = ?4, completed_at = COALESCE(?5, completed_at) WHERE id = ?1",
        params![id.to_string(), to.as_str(), reason, now, completed],
    )?;
    Ok(from)
}

// --- agents -----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Agent {
    pub id: AgentId,
    pub project_id: ProjectId,
    pub session_id: SessionId,
    pub task_id: TaskId,
    pub worktree_id: Option<WorktreeId>,
    pub number: i64,
    pub state: AgentState,
    pub state_reason: Option<String>,
    pub execution_mode: ExecutionMode,
    pub requested_model_id: Option<String>,
    pub requested_profile_id: Option<String>,
    pub failover_policy: FailoverPolicy,
    pub context_mode: ContextMode,
    pub priority: i64,
}

const AGENT_COLS: &str =
    "id, project_id, session_id, task_id, worktree_id, number, state, state_reason, execution_mode,
    requested_model_id, requested_profile_id, failover_policy, context_mode, priority";

fn agent_row(r: &Row<'_>) -> rusqlite::Result<Agent> {
    Ok(Agent {
        id: col(r, 0)?,
        project_id: col(r, 1)?,
        session_id: col(r, 2)?,
        task_id: col(r, 3)?,
        worktree_id: opt_col(r, 4)?,
        number: r.get(5)?,
        state: col(r, 6)?,
        state_reason: r.get(7)?,
        execution_mode: col(r, 8)?,
        requested_model_id: r.get(9)?,
        requested_profile_id: r.get(10)?,
        failover_policy: col(r, 11)?,
        context_mode: col(r, 12)?,
        priority: r.get(13)?,
    })
}

pub fn insert_agent(conn: &Connection, a: &Agent, now: i64) -> Result<(), RepoError> {
    if a.state.requires_reason() && a.state_reason.is_none() {
        return Err(RepoError::MissingReason {
            state: a.state.as_str(),
        });
    }
    conn.execute(
        &format!("INSERT INTO agents ({AGENT_COLS}, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)"),
        params![
            a.id.to_string(),
            a.project_id.to_string(),
            a.session_id.to_string(),
            a.task_id.to_string(),
            a.worktree_id.map(|w| w.to_string()),
            a.number,
            a.state.as_str(),
            a.state_reason,
            a.execution_mode.as_str(),
            a.requested_model_id,
            a.requested_profile_id,
            a.failover_policy.as_str(),
            a.context_mode.as_str(),
            a.priority,
            now
        ],
    )?;
    Ok(())
}

pub fn get_agent(conn: &Connection, id: AgentId) -> Result<Agent, RepoError> {
    conn.query_row(
        &format!("SELECT {AGENT_COLS} FROM agents WHERE id = ?1"),
        [id.to_string()],
        agent_row,
    )
    .optional()?
    .ok_or_else(not_found("agente", id))
}

/// Siguiente número visible `#N` del proyecto.
pub fn next_agent_number(conn: &Connection, project: ProjectId) -> Result<i64, RepoError> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(number), 0) + 1 FROM agents WHERE project_id = ?1",
        [project.to_string()],
        |r| r.get(0),
    )?)
}

/// Agentes no terminados ni archivados del proyecto (vista Home), por número.
pub fn live_agents(conn: &Connection, project: ProjectId) -> Result<Vec<Agent>, RepoError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {AGENT_COLS} FROM agents WHERE project_id = ?1 AND archived_at IS NULL
         AND state NOT IN ('COMPLETED','CANCELLED') ORDER BY number"
    ))?;
    let rows = stmt.query_map([project.to_string()], agent_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Cambia el estado validando la transición (`core`) y la razón obligatoria. Devuelve el estado anterior.
pub fn set_agent_state(
    conn: &Connection,
    id: AgentId,
    to: AgentState,
    reason: Option<&str>,
    now: i64,
) -> Result<AgentState, RepoError> {
    let from = get_agent(conn, id)?.state;
    from.transition(to)?;
    if to.requires_reason() && reason.is_none() {
        return Err(RepoError::MissingReason { state: to.as_str() });
    }
    conn.execute(
        "UPDATE agents SET state = ?2, state_reason = ?3, updated_at = ?4 WHERE id = ?1",
        params![id.to_string(), to.as_str(), reason, now],
    )?;
    Ok(from)
}

pub fn set_agent_worktree(
    conn: &Connection,
    id: AgentId,
    worktree: WorktreeId,
    now: i64,
) -> Result<(), RepoError> {
    let n = conn.execute(
        "UPDATE agents SET worktree_id = ?2, updated_at = ?3 WHERE id = ?1",
        params![id.to_string(), worktree.to_string(), now],
    )?;
    if n == 0 {
        return Err(not_found("agente", id)());
    }
    Ok(())
}

// --- worktrees --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Worktree {
    pub id: WorktreeId,
    pub project_id: ProjectId,
    pub path: String,
    pub branch: String,
    pub base_ref: String,
    pub deps_strategy: String,
    pub status: String,
}

pub fn insert_worktree(conn: &Connection, w: &Worktree, now: i64) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO worktrees (id, project_id, path, branch, base_ref, deps_strategy, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![w.id.to_string(), w.project_id.to_string(), w.path, w.branch, w.base_ref, w.deps_strategy, w.status, now],
    )?;
    Ok(())
}

pub fn get_worktree(conn: &Connection, id: WorktreeId) -> Result<Worktree, RepoError> {
    conn.query_row(
        "SELECT id, project_id, path, branch, base_ref, deps_strategy, status FROM worktrees WHERE id = ?1",
        [id.to_string()],
        |r| {
            Ok(Worktree {
                id: col(r, 0)?,
                project_id: col(r, 1)?,
                path: r.get(2)?,
                branch: r.get(3)?,
                base_ref: r.get(4)?,
                deps_strategy: r.get(5)?,
                status: r.get(6)?,
            })
        },
    )
    .optional()?
    .ok_or_else(not_found("worktree", id))
}

/// `status` se valida con el CHECK de la tabla (DB §3.C).
pub fn set_worktree_status(
    conn: &Connection,
    id: WorktreeId,
    status: &str,
    head_commit: Option<&str>,
    now: i64,
) -> Result<(), RepoError> {
    let removed = (status == "REMOVED").then_some(now);
    let n = conn.execute(
        "UPDATE worktrees SET status = ?2, head_commit = COALESCE(?3, head_commit), removed_at = COALESCE(?4, removed_at) WHERE id = ?1",
        params![id.to_string(), status, head_commit, removed],
    )?;
    if n == 0 {
        return Err(not_found("worktree", id)());
    }
    Ok(())
}

// --- providers y models -----------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    pub id: String,
    pub display_name: String,
    pub cli_name: String,
    pub cli_path: Option<String>,
    pub cli_version: Option<String>,
    pub setup_state: String,
    pub hooks_supported: bool,
    pub hooks_can_hold: Option<bool>,
}

/// Inserta o actualiza lo detectado del proveedor. No toca `enabled` (lo decide el usuario).
pub fn upsert_provider(conn: &Connection, p: &Provider, now: i64) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO providers (id, display_name, cli_name, cli_path, cli_version, adapter_mode, setup_state, hooks_supported, hooks_can_hold, last_checked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'CLI', ?6, ?7, ?8, ?9)
         ON CONFLICT(id) DO UPDATE SET display_name = excluded.display_name, cli_name = excluded.cli_name,
             cli_path = excluded.cli_path, cli_version = excluded.cli_version, setup_state = excluded.setup_state,
             hooks_supported = excluded.hooks_supported, hooks_can_hold = excluded.hooks_can_hold,
             last_checked_at = excluded.last_checked_at",
        params![p.id, p.display_name, p.cli_name, p.cli_path, p.cli_version, p.setup_state, p.hooks_supported, p.hooks_can_hold, now],
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// Canónico: `claude/sonnet`.
    pub id: String,
    pub provider_id: String,
    pub cli_model_id: String,
    pub display_name: String,
    pub context_window: Option<i64>,
}

/// Inserta o actualiza un modelo que el CLI reporta, y refresca `last_seen_at`.
pub fn upsert_model(conn: &Connection, m: &Model, now: i64) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO models (id, provider_id, cli_model_id, display_name, context_window, discovered_at, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         ON CONFLICT(id) DO UPDATE SET cli_model_id = excluded.cli_model_id, display_name = excluded.display_name,
             context_window = excluded.context_window, last_seen_at = excluded.last_seen_at",
        params![m.id, m.provider_id, m.cli_model_id, m.display_name, m.context_window, now],
    )?;
    Ok(())
}

pub fn models_of(conn: &Connection, provider_id: &str) -> Result<Vec<Model>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT id, provider_id, cli_model_id, display_name, context_window FROM models WHERE provider_id = ?1 AND enabled = 1 ORDER BY id",
    )?;
    let rows = stmt.query_map([provider_id], |r| {
        Ok(Model {
            id: r.get(0)?,
            provider_id: r.get(1)?,
            cli_model_id: r.get(2)?,
            display_name: r.get(3)?,
            context_window: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

// --- agent_runs -------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct AgentRun {
    pub id: RunId,
    pub agent_id: AgentId,
    pub seq: i64,
    pub provider_id: String,
    pub model_id: String,
    pub cli_session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub pid: Option<u32>,
    pub status: RunStatus,
    pub end_reason: Option<RunEndReason>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

const RUN_COLS: &str = "id, agent_id, seq, provider_id, model_id, cli_session_id, transcript_path, pid, status, end_reason, started_at, ended_at";

fn run_row(r: &Row<'_>) -> rusqlite::Result<AgentRun> {
    Ok(AgentRun {
        id: col(r, 0)?,
        agent_id: col(r, 1)?,
        seq: r.get(2)?,
        provider_id: r.get(3)?,
        model_id: r.get(4)?,
        cli_session_id: r.get(5)?,
        transcript_path: r.get(6)?,
        pid: r.get(7)?,
        status: col(r, 8)?,
        end_reason: opt_col(r, 9)?,
        started_at: r.get(10)?,
        ended_at: r.get(11)?,
    })
}

/// Abre un run nuevo (`seq` siguiente). Falla si el agente ya tiene uno abierto (índice parcial).
pub fn open_run(
    conn: &Connection,
    id: RunId,
    agent: AgentId,
    provider_id: &str,
    model_id: &str,
    now: i64,
) -> Result<AgentRun, RepoError> {
    let seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM agent_runs WHERE agent_id = ?1",
        [agent.to_string()],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO agent_runs (id, agent_id, seq, provider_id, model_id, status, started_at) VALUES (?1, ?2, ?3, ?4, ?5, 'STARTING', ?6)",
        params![id.to_string(), agent.to_string(), seq, provider_id, model_id, now],
    )?;
    get_run(conn, id)
}

pub fn get_run(conn: &Connection, id: RunId) -> Result<AgentRun, RepoError> {
    conn.query_row(
        &format!("SELECT {RUN_COLS} FROM agent_runs WHERE id = ?1"),
        [id.to_string()],
        run_row,
    )
    .optional()?
    .ok_or_else(not_found("run", id))
}

/// El executor vivo del agente: de acá sale el modelo actual (AGENT ≠ MODEL).
pub fn current_run(conn: &Connection, agent: AgentId) -> Result<Option<AgentRun>, RepoError> {
    Ok(conn
        .query_row(
            &format!("SELECT {RUN_COLS} FROM agent_runs WHERE agent_id = ?1 AND ended_at IS NULL"),
            [agent.to_string()],
            run_row,
        )
        .optional()?)
}

pub fn runs_of(conn: &Connection, agent: AgentId) -> Result<Vec<AgentRun>, RepoError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {RUN_COLS} FROM agent_runs WHERE agent_id = ?1 ORDER BY seq"
    ))?;
    let rows = stmt.query_map([agent.to_string()], run_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Datos del CLI que se conocen después del spawn.
pub fn set_run_process(
    conn: &Connection,
    id: RunId,
    pid: u32,
    cli_session_id: Option<&str>,
    transcript_path: Option<&str>,
) -> Result<(), RepoError> {
    let n = conn.execute(
        "UPDATE agent_runs SET status = 'RUNNING', pid = ?2, cli_session_id = COALESCE(?3, cli_session_id),
             transcript_path = COALESCE(?4, transcript_path) WHERE id = ?1 AND ended_at IS NULL",
        params![id.to_string(), pid, cli_session_id, transcript_path],
    )?;
    if n == 0 {
        return Err(not_found("run abierto", id)());
    }
    Ok(())
}

pub fn heartbeat(conn: &Connection, id: RunId, now: i64) -> Result<(), RepoError> {
    conn.execute(
        "UPDATE agent_runs SET last_heartbeat_at = ?2 WHERE id = ?1 AND ended_at IS NULL",
        params![id.to_string(), now],
    )?;
    Ok(())
}

/// Cierra el run. Un run cerrado no se vuelve a abrir.
pub fn close_run(
    conn: &Connection,
    id: RunId,
    status: RunStatus,
    reason: RunEndReason,
    exit_code: Option<i32>,
    now: i64,
) -> Result<(), RepoError> {
    if matches!(status, RunStatus::Starting | RunStatus::Running) {
        return Err(RepoError::Transition(InvalidTransition {
            kind: "RunStatus",
            from: "abierto",
            to: status.as_str(),
        }));
    }
    let n = conn.execute(
        "UPDATE agent_runs SET status = ?2, end_reason = ?3, exit_code = ?4, ended_at = ?5 WHERE id = ?1 AND ended_at IS NULL",
        params![id.to_string(), status.as_str(), reason.as_str(), exit_code, now],
    )?;
    if n == 0 {
        return Err(not_found("run abierto", id)());
    }
    Ok(())
}

/// Sesión abierta de este daemon para el proyecto, si ya hay una.
pub fn active_session(
    conn: &Connection,
    project: ProjectId,
    daemon_pid: u32,
) -> Result<Option<SessionId>, RepoError> {
    Ok(unclosed_sessions(conn, project)?
        .into_iter()
        .find(|(_, pid)| *pid == Some(daemon_pid))
        .map(|(id, _)| id))
}

// --- selección de executor --------------------------------------------------

/// Un modelo que se puede usar ya: proveedor detectado (`READY`) y habilitado, modelo habilitado.
#[derive(Debug, Clone, PartialEq)]
pub struct EligibleModel {
    pub model_id: String,
    pub provider_id: String,
    pub cli_model_id: String,
}

/// `Ok(model)` si el modelo exacto es elegible; `Err(motivo legible)` si no (FLOW §6).
pub fn eligible_model(
    conn: &Connection,
    model_id: &str,
) -> Result<Result<EligibleModel, String>, RepoError> {
    let row: Option<(String, String, bool, String, bool)> = conn
        .query_row(
            "SELECT m.provider_id, m.cli_model_id, m.enabled = 1, p.setup_state, p.enabled = 1
             FROM models m JOIN providers p ON p.id = m.provider_id WHERE m.id = ?1",
            [model_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    Ok(match row {
        None => Err(format!(
            "el modelo `{model_id}` no existe en ningún proveedor detectado"
        )),
        Some((_, _, false, _, _)) => Err(format!("el modelo `{model_id}` está deshabilitado")),
        Some((provider, _, _, _, false)) => {
            Err(format!("el proveedor `{provider}` está deshabilitado"))
        }
        Some((provider, _, _, state, _)) if state != "READY" => {
            Err(format!("el proveedor `{provider}` no está listo ({state})"))
        }
        Some((provider_id, cli_model_id, ..)) => Ok(EligibleModel {
            model_id: model_id.to_string(),
            provider_id,
            cli_model_id,
        }),
    })
}

/// Proveedores listos y habilitados, en orden estable.
pub fn ready_providers(conn: &Connection) -> Result<Vec<String>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT id FROM providers WHERE setup_state = 'READY' AND enabled = 1 ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

// --- checkpoints ------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewCheckpoint {
    pub id: symphony_core::CheckpointId,
    pub agent_id: AgentId,
    pub run_id: Option<RunId>,
    pub objective: String,
    /// Cola del último mensaje del asistente: el "qué seguía" (LEARNINGS H1).
    pub plan_tail: Option<String>,
    pub current_step: Option<String>,
    pub next_step: Option<String>,
    pub head_commit: Option<String>,
    pub diff_object_id: Option<symphony_core::ContextObjectId>,
    pub summary_json: Option<String>,
}

/// Inserta el siguiente checkpoint del agente y devuelve su `seq`.
pub fn insert_checkpoint(conn: &Connection, c: &NewCheckpoint, now: i64) -> Result<i64, RepoError> {
    let seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM checkpoints WHERE agent_id = ?1",
        [c.agent_id.to_string()],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO checkpoints (id, agent_id, run_id, seq, objective, plan_tail, current_step, next_step,
                                  head_commit, diff_object_id, summary_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            c.id.to_string(),
            c.agent_id.to_string(),
            c.run_id.map(|r| r.to_string()),
            seq,
            c.objective,
            c.plan_tail,
            c.current_step,
            c.next_step,
            c.head_commit,
            c.diff_object_id.map(|o| o.to_string()),
            c.summary_json,
            now
        ],
    )?;
    if let Some(object) = c.diff_object_id {
        conn.execute(
            "INSERT INTO checkpoint_refs (checkpoint_id, object_id, role) VALUES (?1, ?2, 'DIFF')",
            params![c.id.to_string(), object.to_string()],
        )?;
    }
    Ok(seq)
}

/// Lo que el checkpointer necesita saber de un agente para tomar un checkpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckpointBase {
    pub project_id: ProjectId,
    pub worktree_path: String,
    pub base_ref: String,
    pub objective: String,
    pub next_step: Option<String>,
    pub open_run: Option<RunId>,
}

/// `None` si el agente no existe, no tiene worktree o todavía no tiene checkpoint inicial.
pub fn checkpoint_base(
    conn: &Connection,
    agent: AgentId,
) -> Result<Option<CheckpointBase>, RepoError> {
    Ok(conn
        .query_row(
            "SELECT a.project_id, w.path, w.base_ref, c.objective, c.next_step,
                    (SELECT r.id FROM agent_runs r WHERE r.agent_id = a.id AND r.ended_at IS NULL)
             FROM agents a
             JOIN worktrees w ON w.id = a.worktree_id
             JOIN checkpoints c ON c.agent_id = a.id
             WHERE a.id = ?1 ORDER BY c.seq DESC LIMIT 1",
            [agent.to_string()],
            |r| {
                Ok(CheckpointBase {
                    project_id: col(r, 0)?,
                    worktree_path: r.get(1)?,
                    base_ref: r.get(2)?,
                    objective: r.get(3)?,
                    next_step: r.get(4)?,
                    open_run: opt_col(r, 5)?,
                })
            },
        )
        .optional()?)
}

/// Objeto `GIT_DIFF` del agente para ese blob: reutiliza el que ya existe (mismo diff,
/// otro checkpoint) o lo crea. Devuelve `(id, creado)`; si se creó, el llamador suma la ref del blob.
pub fn diff_object(
    conn: &Connection,
    project: ProjectId,
    agent: AgentId,
    run: Option<RunId>,
    blob_hash: &str,
    now: i64,
) -> Result<(symphony_core::ContextObjectId, bool), RepoError> {
    let uri = format!("ctx://diff/{agent}/{blob_hash}");
    let existing: Option<symphony_core::ContextObjectId> = conn
        .query_row(
            "SELECT id FROM context_objects WHERE uri = ?1",
            [&uri],
            |r| col(r, 0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok((id, false));
    }
    let id = symphony_core::ContextObjectId::new();
    insert_context_object(
        conn,
        id,
        &uri,
        project,
        Some(agent),
        run,
        "GIT_DIFF",
        blob_hash,
        now,
    )?;
    Ok((id, true))
}

/// Conserva los últimos `keep` checkpoints del agente más los que usan handoffs, runs
/// o cambios de executor (IDEA §5.5, DB §3.G). Borra los diffs que quedaron sin uso y
/// devuelve sus blobs para que el llamador suelte la referencia.
pub fn prune_checkpoints(
    conn: &Connection,
    agent: AgentId,
    keep: usize,
) -> Result<Vec<String>, RepoError> {
    let agent = agent.to_string();
    let keep = i64::try_from(keep).unwrap_or(i64::MAX);
    let doomed = "SELECT id FROM checkpoints c WHERE c.agent_id = ?1
        AND c.seq <= (SELECT MAX(seq) FROM checkpoints WHERE agent_id = ?1) - ?2
        AND NOT EXISTS (SELECT 1 FROM handoffs h WHERE h.checkpoint_id = c.id)
        AND NOT EXISTS (SELECT 1 FROM agent_runs r WHERE r.start_checkpoint_id = c.id)
        AND NOT EXISTS (SELECT 1 FROM executor_changes x WHERE x.checkpoint_id = c.id)";
    conn.execute(
        &format!("DELETE FROM checkpoint_refs WHERE checkpoint_id IN ({doomed})"),
        params![agent, keep],
    )?;
    conn.execute(
        &format!("DELETE FROM checkpoints WHERE id IN ({doomed})"),
        params![agent, keep],
    )?;
    let orphans = "FROM context_objects AS o WHERE o.agent_id = ?1 AND o.kind = 'GIT_DIFF'
        AND NOT EXISTS (SELECT 1 FROM checkpoint_refs r WHERE r.object_id = o.id)
        AND NOT EXISTS (SELECT 1 FROM checkpoints c WHERE c.diff_object_id = o.id)";
    let hashes: Vec<String> = conn
        .prepare(&format!("SELECT o.blob_hash {orphans}"))?
        .query_map([&agent], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    conn.execute(&format!("DELETE {orphans}"), [&agent])?;
    Ok(hashes)
}

/// El último checkpoint válido del agente (lo que usa un handoff).
#[derive(Debug, Clone, PartialEq)]
pub struct LatestCheckpoint {
    pub id: symphony_core::CheckpointId,
    pub seq: i64,
    pub created_at: i64,
    pub objective: String,
    pub plan_tail: Option<String>,
    pub current_step: Option<String>,
    pub next_step: Option<String>,
    pub summary_json: Option<String>,
    /// `ctx://…` del diff guardado en ese checkpoint.
    pub diff_uri: Option<String>,
}

pub fn latest_checkpoint(
    conn: &Connection,
    agent: AgentId,
) -> Result<Option<LatestCheckpoint>, RepoError> {
    Ok(conn
        .query_row(
            "SELECT c.id, c.seq, c.objective, c.plan_tail, c.current_step, c.next_step, c.summary_json, o.uri, c.created_at
             FROM checkpoints c LEFT JOIN context_objects o ON o.id = c.diff_object_id
             WHERE c.agent_id = ?1 AND c.is_valid = 1 ORDER BY c.seq DESC LIMIT 1",
            [agent.to_string()],
            |r| {
                Ok(LatestCheckpoint {
                    id: col(r, 0)?,
                    seq: r.get(1)?,
                    objective: r.get(2)?,
                    plan_tail: r.get(3)?,
                    current_step: r.get(4)?,
                    next_step: r.get(5)?,
                    summary_json: r.get(6)?,
                    diff_uri: r.get(7)?,
                    created_at: r.get(8)?,
                })
            },
        )
        .optional()?)
}

/// Caracteres de toda la conversación del agente (cortos + largos en el object store):
/// lo que costaría reenviar el historial sin optimizar.
pub fn conversation_chars(conn: &Connection, agent: AgentId) -> Result<i64, RepoError> {
    Ok(conn.query_row(
        "SELECT COALESCE((SELECT SUM(LENGTH(content)) FROM messages WHERE agent_id = ?1), 0)
              + COALESCE((SELECT SUM(b.size_bytes) FROM messages m
                          JOIN context_objects o ON o.id = m.content_object_id
                          JOIN blobs b ON b.hash = o.blob_hash WHERE m.agent_id = ?1), 0)",
        [agent.to_string()],
        |r| r.get(0),
    )?)
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewHandoff {
    pub id: symphony_core::HandoffId,
    pub agent_id: AgentId,
    /// `None` en el primer spawn.
    pub checkpoint_id: Option<symphony_core::CheckpointId>,
    pub to_run_id: RunId,
    pub mode: ContextMode,
    pub tokens_raw_estimate: i64,
    pub tokens_sent: i64,
    pub build_ms: i64,
}

/// Un handoff inicia exactamente un run (`to_run_id` UNIQUE).
pub fn insert_handoff(conn: &Connection, h: &NewHandoff, now: i64) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO handoffs (id, agent_id, checkpoint_id, to_run_id, mode, tokens_raw_estimate, tokens_sent, build_ms, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            h.id.to_string(),
            h.agent_id.to_string(),
            h.checkpoint_id.map(|c| c.to_string()),
            h.to_run_id.to_string(),
            h.mode.as_str(),
            h.tokens_raw_estimate,
            h.tokens_sent,
            h.build_ms,
            now
        ],
    )?;
    Ok(())
}

/// `outcome`: `CONTINUED` · `NEEDED_RETRIEVAL` · `FAILED_TO_CONTINUE` · `RETRIED_SAFER` (CHECK de la tabla).
pub fn set_handoff_outcome(
    conn: &Connection,
    to_run: RunId,
    outcome: &str,
) -> Result<(), RepoError> {
    conn.execute(
        "UPDATE handoffs SET outcome = ?2 WHERE to_run_id = ?1",
        params![to_run.to_string(), outcome],
    )?;
    Ok(())
}

// --- cambio de executor (P06.S5) --------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct NewProviderFailure {
    pub id: symphony_core::ProviderFailureId,
    pub provider_id: String,
    pub model_id: Option<String>,
    pub run_id: Option<RunId>,
    pub failure_type: symphony_core::FailureType,
    pub raw_code: Option<String>,
    /// Ya redactado.
    pub message: String,
    pub reset_at: Option<i64>,
}

pub fn insert_provider_failure(
    conn: &Connection,
    f: &NewProviderFailure,
    now: i64,
) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO provider_failures (id, provider_id, model_id, run_id, failure_type, raw_code, message, reset_at, confidence, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1.0, ?9)",
        params![
            f.id.to_string(),
            f.provider_id,
            f.model_id,
            f.run_id.map(|r| r.to_string()),
            f.failure_type.as_str(),
            f.raw_code,
            f.message,
            f.reset_at,
            now
        ],
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewExecutorChange {
    pub id: symphony_core::ExecutorChangeId,
    pub agent_id: AgentId,
    pub from_run_id: RunId,
    /// `None` si no hubo reemplazo (el agente quedó `WAITING_PROVIDER`).
    pub to_run_id: Option<RunId>,
    /// `FAILOVER` · `USER_SWITCH` · `SUGGESTION_ACCEPTED` · `RESTART` · `RECLAIM` (CHECK).
    pub reason: &'static str,
    pub failure_id: Option<symphony_core::ProviderFailureId>,
    pub checkpoint_id: Option<symphony_core::CheckpointId>,
    pub checkpoint_age_ms: Option<i64>,
}

pub fn insert_executor_change(
    conn: &Connection,
    c: &NewExecutorChange,
    now: i64,
) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO executor_changes (id, agent_id, from_run_id, to_run_id, reason, failure_id, checkpoint_id, checkpoint_age_ms, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            c.id.to_string(),
            c.agent_id.to_string(),
            c.from_run_id.to_string(),
            c.to_run_id.map(|r| r.to_string()),
            c.reason,
            c.failure_id.map(|f| f.to_string()),
            c.checkpoint_id.map(|x| x.to_string()),
            c.checkpoint_age_ms,
            now
        ],
    )?;
    Ok(())
}

/// Un cambio manual de modelo es una elección exacta del usuario (IDEA §5.7): se recuerda.
pub fn set_agent_exact_model(
    conn: &Connection,
    agent: AgentId,
    model_id: &str,
    now: i64,
) -> Result<(), RepoError> {
    conn.execute(
        "UPDATE agents SET execution_mode = 'EXACT', requested_model_id = ?2, requested_profile_id = NULL, updated_at = ?3 WHERE id = ?1",
        params![agent.to_string(), model_id, now],
    )?;
    Ok(())
}

/// Siguiente executor para un failover básico (P06.S5; el routing completo llega en P10).
///
/// Nunca vuelve a un modelo (ni, con `ANY`, a un proveedor) que este agente ya agotó o
/// que rechazó el login. Con `ANY` prueba primero otros proveedores listos (orden estable
/// por id) y después otros modelos del mismo; con `SAME_PROVIDER`, solo lo segundo. Con
/// un fallo de login, el mismo proveedor no sirve.
#[allow(clippy::too_many_arguments)]
pub fn next_executor(
    conn: &Connection,
    agent: AgentId,
    policy: FailoverPolicy,
    current_provider: &str,
    current_model: &str,
    auth_failure: bool,
    has_adapter: &dyn Fn(&str) -> bool,
) -> Result<Option<EligibleModel>, RepoError> {
    if policy == FailoverPolicy::None {
        return Ok(None);
    }
    let failed: Vec<(String, String)> = conn
        .prepare(
            "SELECT provider_id, model_id FROM agent_runs
             WHERE agent_id = ?1 AND end_reason IN ('QUOTA_EXHAUSTED','AUTH_ERROR')",
        )?
        .query_map([agent.to_string()], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let failed_provider = |p: &str| p == current_provider || failed.iter().any(|(fp, _)| fp == p);
    let failed_model = |m: &str| m == current_model || failed.iter().any(|(_, fm)| fm == m);
    let first_model =
        |provider: &str, skip: &dyn Fn(&str) -> bool| -> Result<Option<EligibleModel>, RepoError> {
            Ok(models_of(conn, provider)?
                .into_iter()
                .find(|m| !skip(&m.id))
                .map(|m| EligibleModel {
                    model_id: m.id,
                    provider_id: m.provider_id,
                    cli_model_id: m.cli_model_id,
                }))
        };
    let ready = ready_providers(conn)?;
    if policy == FailoverPolicy::Any {
        for p in ready
            .iter()
            .filter(|p| !failed_provider(p) && has_adapter(p))
        {
            if let Some(m) = first_model(p, &|_| false)? {
                return Ok(Some(m));
            }
        }
    }
    if auth_failure
        || !ready.iter().any(|p| p == current_provider)
        || !has_adapter(current_provider)
    {
        return Ok(None);
    }
    first_model(current_provider, &failed_model)
}

// --- conversación y tool calls ----------------------------------------------

/// Valores de `messages.role` (DB §3.C).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
    ExecutorChange,
}

impl MessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "USER",
            Self::Assistant => "ASSISTANT",
            Self::System => "SYSTEM",
            Self::ExecutorChange => "EXECUTOR_CHANGE",
        }
    }
}

/// Un mensaje corto va en `content`; uno largo, en `content_object_id`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewMessage {
    pub id: symphony_core::MessageId,
    pub agent_id: AgentId,
    pub run_id: Option<RunId>,
    pub role: MessageRole,
    pub content: Option<String>,
    pub content_object_id: Option<symphony_core::ContextObjectId>,
}

pub fn insert_message(conn: &Connection, m: &NewMessage, now: i64) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO messages (id, agent_id, run_id, role, content, content_object_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            m.id.to_string(),
            m.agent_id.to_string(),
            m.run_id.map(|r| r.to_string()),
            m.role.as_str(),
            m.content,
            m.content_object_id.map(|o| o.to_string()),
            now
        ],
    )?;
    Ok(())
}

/// Objeto de contexto `ctx://…` sobre un blob ya guardado (`kind` lo valida el CHECK).
#[allow(clippy::too_many_arguments)]
pub fn insert_context_object(
    conn: &Connection,
    id: symphony_core::ContextObjectId,
    uri: &str,
    project: ProjectId,
    agent: Option<AgentId>,
    run: Option<RunId>,
    kind: &str,
    blob_hash: &str,
    now: i64,
) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO context_objects (id, uri, project_id, agent_id, run_id, kind, blob_hash, compressor, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'NONE', ?8)",
        params![
            id.to_string(),
            uri,
            project.to_string(),
            agent.map(|a| a.to_string()),
            run.map(|r| r.to_string()),
            kind,
            blob_hash,
            now
        ],
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewToolCall {
    pub id: symphony_core::ToolCallId,
    pub agent_id: AgentId,
    pub run_id: RunId,
    pub tool_name: String,
    /// Ya redactado.
    pub command: Option<String>,
    pub op_class: i64,
}

/// Registra una herramienta pedida y dejada correr (sin retención: `RUNNING`, `NONE`).
pub fn insert_tool_call(conn: &Connection, c: &NewToolCall, now: i64) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO tool_calls (id, agent_id, run_id, tool_name, command, op_class, status, enforcement, requested_at, started_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'RUNNING', 'NONE', ?7, ?7)",
        params![
            c.id.to_string(),
            c.agent_id.to_string(),
            c.run_id.to_string(),
            c.tool_name,
            c.command,
            c.op_class,
            now
        ],
    )?;
    Ok(())
}

/// Cierra una tool call abierta (`DONE` o `FAILED`). Una ya cerrada no cambia.
pub fn finish_tool_call(
    conn: &Connection,
    id: symphony_core::ToolCallId,
    ok: bool,
    exit_code: Option<i32>,
    now: i64,
) -> Result<(), RepoError> {
    conn.execute(
        "UPDATE tool_calls SET status = ?2, exit_code = ?3, finished_at = ?4
         WHERE id = ?1 AND status IN ('REQUESTED','QUEUED','RUNNING')",
        params![
            id.to_string(),
            if ok { "DONE" } else { "FAILED" },
            exit_code,
            now
        ],
    )?;
    Ok(())
}

// --- recuperación -----------------------------------------------------------

/// Abre un problema en el Recovery Center (`kind` lo valida el CHECK de la tabla).
pub fn open_recovery_item(
    conn: &Connection,
    project: ProjectId,
    agent: Option<AgentId>,
    run: Option<RunId>,
    kind: &str,
    detail: &str,
    now: i64,
) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO recovery_items (id, project_id, agent_id, run_id, kind, detail, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'OPEN', ?7)",
        params![
            symphony_core::RecoveryItemId::new().to_string(),
            project.to_string(),
            agent.map(|a| a.to_string()),
            run.map(|r| r.to_string()),
            kind,
            detail,
            now
        ],
    )?;
    Ok(())
}

/// Resuelve los items de recuperación abiertos de un agente (restart, reclaim,
/// failover aceptado…). Devuelve cuántos cerró.
pub fn resolve_recovery_items(
    conn: &Connection,
    agent: AgentId,
    resolution: &str,
    now: i64,
) -> Result<usize, RepoError> {
    Ok(conn.execute(
        "UPDATE recovery_items SET status = 'RESOLVED', resolution = ?2, resolved_at = ?3
         WHERE agent_id = ?1 AND status = 'OPEN'",
        params![agent.to_string(), resolution, now],
    )?)
}

/// Al arrancar el daemon (con el lock de instancia tomado): toda sesión que
/// siga `ACTIVE` es de un daemon anterior que murió sin cerrarla. Se marca
/// `INTERRUPTED` y se abre un `recovery_items(SESSION_INTERRUPTED)` por cada una
/// (FLOW §16, Journey E). Devuelve las sesiones recuperadas.
pub fn interrupt_orphan_sessions(conn: &Connection, now: i64) -> Result<Vec<String>, RepoError> {
    // IDs como texto: la recuperación no puede fallar por una fila inesperada.
    let orphans: Vec<(String, String, Option<u32>)> = conn
        .prepare("SELECT id, project_id, daemon_pid FROM sessions WHERE status = 'ACTIVE' AND ended_at IS NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (session, project, pid) in &orphans {
        conn.execute(
            "UPDATE sessions SET status = 'INTERRUPTED', ended_at = ?2 WHERE id = ?1",
            params![session, now],
        )?;
        let detail = match pid {
            Some(pid) => format!(
                "La sesión anterior se cortó sin cerrarse: el daemon (pid {pid}) dejó de correr."
            ),
            None => "La sesión anterior se cortó sin cerrarse.".to_string(),
        };
        conn.execute(
            "INSERT INTO recovery_items (id, project_id, kind, detail, status, created_at)
             VALUES (?1, ?2, 'SESSION_INTERRUPTED', ?3, 'OPEN', ?4)",
            params![
                symphony_core::RecoveryItemId::new().to_string(),
                project.to_string(),
                detail,
                now
            ],
        )?;
    }
    Ok(orphans.into_iter().map(|(s, _, _)| s).collect())
}

/// Cantidad de problemas abiertos en el Recovery Center.
pub fn open_recovery_count(conn: &Connection) -> Result<u64, RepoError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM recovery_items WHERE status = 'OPEN'",
        [],
        |r| r.get(0),
    )?;
    Ok(u64::try_from(n).unwrap_or(0))
}

// --- consultas de vistas (DB §5) --------------------------------------------

/// Una fila de la vista Home: agente vivo + su run abierto (modelo actual) + su task.
#[derive(Debug, Clone, PartialEq)]
pub struct HomeRow {
    pub agent_id: AgentId,
    pub number: i64,
    pub state: AgentState,
    pub state_reason: Option<String>,
    pub task_code: String,
    pub task_title: String,
    /// `None` si el agente no tiene executor vivo (p. ej. `WAITING_PROVIDER`).
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
}

/// Home (DB §5): agentes activos + run abierto + modelo + task. `provider_health`
/// y `resource_samples` se suman en sus fases (4 y 2).
pub fn home_rows(conn: &Connection, project: ProjectId) -> Result<Vec<HomeRow>, RepoError> {
    list_agents_rows(conn, Some(project), false)
}

/// Lista agentes de un proyecto (o todos si project_id es None), activos o todos.
pub fn list_agents_rows(
    conn: &Connection,
    project_id: Option<ProjectId>,
    all: bool,
) -> Result<Vec<HomeRow>, RepoError> {
    let filter_active = if all {
        ""
    } else {
        "AND a.state NOT IN ('COMPLETED','CANCELLED')"
    };
    let sql = if project_id.is_some() {
        format!(
            "SELECT a.id, a.number, a.state, a.state_reason, t.code, t.title, r.provider_id, r.model_id
             FROM agents a
             JOIN tasks t ON t.id = a.task_id
             LEFT JOIN agent_runs r ON r.agent_id = a.id AND r.ended_at IS NULL
             WHERE a.project_id = ?1 AND a.archived_at IS NULL {filter_active}
             ORDER BY a.number"
        )
    } else {
        format!(
            "SELECT a.id, a.number, a.state, a.state_reason, t.code, t.title, r.provider_id, r.model_id
             FROM agents a
             JOIN tasks t ON t.id = a.task_id
             LEFT JOIN agent_runs r ON r.agent_id = a.id AND r.ended_at IS NULL
             WHERE a.archived_at IS NULL {filter_active}
             ORDER BY a.number"
        )
    };
    let mut stmt = conn.prepare(&sql)?;
    let map_row = |r: &rusqlite::Row| {
        Ok(HomeRow {
            agent_id: col(r, 0)?,
            number: r.get(1)?,
            state: col(r, 2)?,
            state_reason: r.get(3)?,
            task_code: r.get(4)?,
            task_title: r.get(5)?,
            provider_id: r.get(6)?,
            model_id: r.get(7)?,
        })
    };
    let rows: Vec<HomeRow> = if let Some(proj) = project_id {
        stmt.query_map([proj.to_string()], map_row)?
            .collect::<Result<_, _>>()?
    } else {
        stmt.query_map([], map_row)?.collect::<Result<_, _>>()?
    };
    Ok(rows)
}

/// Resuelve un agente por ID (ULID) o por número (`1`, `#1`, `agent-1`).
pub fn find_agent_by_ident(
    conn: &Connection,
    project_id: Option<ProjectId>,
    ident: &str,
) -> Result<Agent, RepoError> {
    use std::str::FromStr;
    let clean = ident.trim();
    if let Ok(id) = AgentId::from_str(clean) {
        return get_agent(conn, id);
    }
    let num_str = clean
        .strip_prefix('#')
        .or_else(|| clean.strip_prefix("agent-"))
        .or_else(|| clean.strip_prefix("agent_"))
        .unwrap_or(clean);
    if let Ok(num) = num_str.parse::<i64>() {
        let sql = if project_id.is_some() {
            format!(
                "SELECT {AGENT_COLS} FROM agents WHERE number = ?1 AND project_id = ?2 ORDER BY created_at DESC LIMIT 1"
            )
        } else {
            format!(
                "SELECT {AGENT_COLS} FROM agents WHERE number = ?1 ORDER BY created_at DESC LIMIT 1"
            )
        };
        let mut stmt = conn.prepare(&sql)?;
        let row = if let Some(proj) = project_id {
            stmt.query_row(params![num, proj.to_string()], agent_row)
                .optional()?
        } else {
            stmt.query_row(params![num], agent_row).optional()?
        };
        if let Some(agent) = row {
            return Ok(agent);
        }
    }
    Err(RepoError::NotFound {
        entity: "agente",
        id: ident.to_string(),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct MessageRecord {
    pub id: symphony_core::MessageId,
    pub agent_id: AgentId,
    pub run_id: Option<RunId>,
    pub role: String,
    pub content: Option<String>,
    pub content_object_id: Option<symphony_core::ContextObjectId>,
    pub created_at: i64,
}

pub fn agent_messages(
    conn: &Connection,
    agent: AgentId,
    limit: Option<usize>,
) -> Result<Vec<MessageRecord>, RepoError> {
    let limit_clause = limit.map_or(String::new(), |l| format!("LIMIT {l}"));
    let sql = format!(
        "SELECT id, agent_id, run_id, role, content, content_object_id, created_at
         FROM messages WHERE agent_id = ?1 ORDER BY created_at ASC {limit_clause}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([agent.to_string()], |r| {
        Ok(MessageRecord {
            id: col(r, 0)?,
            agent_id: col(r, 1)?,
            run_id: opt_col(r, 2)?,
            role: r.get(3)?,
            content: r.get(4)?,
            content_object_id: opt_col(r, 5)?,
            created_at: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
