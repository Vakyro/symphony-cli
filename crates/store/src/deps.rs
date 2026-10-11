//! Dependencias entre tareas y milestones (DB §3.B, P08.S5). Todo corre dentro de la
//! transacción del escritor; las reacciones (`reconcile_*`) devuelven qué cambió para que el
//! daemon arranque lo que quedó listo.

use rusqlite::{Connection, OptionalExtension, params};
use symphony_core::{
    AgentId, AgentState, MilestoneId, MilestoneStatus, ProjectId, TaskId, TaskStatus,
};

use crate::repo::{self, RepoError};

/// Una dependencia de una tarea.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepRow {
    pub task_id: TaskId,
    pub code: String,
    pub title: String,
    pub status: TaskStatus,
}

fn conv(idx: usize, e: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
}

fn dep_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<DepRow> {
    let id: String = r.get(0)?;
    let status: String = r.get(3)?;
    Ok(DepRow {
        task_id: id.parse().map_err(|e| conv(0, e))?,
        code: r.get(1)?,
        title: r.get(2)?,
        status: status.parse().map_err(|e| conv(3, e))?,
    })
}

/// De qué tareas depende `task` (todas).
pub fn dependencies_of(conn: &Connection, task: TaskId) -> Result<Vec<DepRow>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.code, t.title, t.status FROM task_dependencies d
         JOIN tasks t ON t.id = d.depends_on_task_id
         WHERE d.task_id = ?1 ORDER BY t.code",
    )?;
    let rows = stmt.query_map([task.to_string()], dep_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Las dependencias de `task` que todavía no están `DONE`.
pub fn unmet_dependencies(conn: &Connection, task: TaskId) -> Result<Vec<DepRow>, RepoError> {
    Ok(dependencies_of(conn, task)?
        .into_iter()
        .filter(|d| d.status != TaskStatus::Done)
        .collect())
}

/// Qué tareas esperan a `task`.
pub fn dependents_of(conn: &Connection, task: TaskId) -> Result<Vec<TaskId>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT task_id FROM task_dependencies WHERE depends_on_task_id = ?1 ORDER BY created_at, task_id",
    )?;
    let ids = stmt.query_map([task.to_string()], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for id in ids {
        if let Ok(t) = id?.parse() {
            out.push(t);
        }
    }
    Ok(out)
}

/// Frase humana de lo que se espera (FLOW §9.1: «espera a T-12 y T-16»).
pub fn waiting_phrase(unmet: &[DepRow]) -> String {
    let codes: Vec<&str> = unmet.iter().map(|d| d.code.as_str()).collect();
    match codes.as_slice() {
        [] => String::new(),
        [one] => format!("espera a {one}"),
        [init @ .., last] => format!("espera a {} y {last}", init.join(", ")),
    }
}

/// ¿Añadir `task → depends_on` cerraría un ciclo? (la misma regla que
/// `symphony_scheduler::creates_cycle`, aplicada sobre las aristas guardadas)
pub fn would_cycle(conn: &Connection, task: TaskId, depends_on: TaskId) -> Result<bool, RepoError> {
    if task == depends_on {
        return Ok(true);
    }
    let reached: bool = conn.query_row(
        "WITH RECURSIVE reach(id) AS (
             SELECT depends_on_task_id FROM task_dependencies WHERE task_id = ?1
             UNION
             SELECT d.depends_on_task_id FROM task_dependencies d JOIN reach r ON d.task_id = r.id
         )
         SELECT EXISTS (SELECT 1 FROM reach WHERE id = ?2)",
        params![depends_on.to_string(), task.to_string()],
        |r| r.get(0),
    )?;
    Ok(reached)
}

#[derive(Debug, thiserror::Error)]
pub enum DepError {
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error("`{task}` ya depende, directa o indirectamente, de `{depends_on}`: sería un ciclo")]
    Cycle { task: String, depends_on: String },
    #[error("una tarea no puede depender de sí misma")]
    SelfDependency,
    #[error("las tareas son de proyectos distintos")]
    OtherProject,
    #[error("no se puede depender de `{0}`: ya está cancelada")]
    DependsOnCancelled(String),
}

impl From<rusqlite::Error> for DepError {
    fn from(e: rusqlite::Error) -> Self {
        DepError::Repo(RepoError::Sqlite(e))
    }
}

/// Cómo terminó el agente de una tarea. El agente puede quedar `FAILED` sin que la tarea
/// haya pasado a `FAILED` (se puede recuperar), por eso lo decide quien llama.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finished {
    Done,
    Failed,
    Cancelled,
}

/// La razón con la que un agente queda `BLOCKED` por una dependencia; sirve para reconocer
/// ese bloqueo (y solo ese) cuando la causa se resuelve.
pub const DEPENDENCY_BLOCK_MARK: &str = "replanificar, ignorar la dependencia o cancelar";

/// Lo que pasó con la tarea al añadirle una dependencia.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    /// La tarea quedó esperando (su agente pasó a `WAITING_DEPENDENCY` o a `BLOCKED`).
    pub waiting: bool,
}

/// `task` pasa a depender de `depends_on`. Rechaza ciclos y tareas de otro proyecto.
/// Si la dependencia no está hecha y el agente de `task` no corre todavía, queda en
/// `WAITING_DEPENDENCY` (y la task en `WAITING`) con la frase de qué espera; si la
/// dependencia ya había fallado, queda `BLOCKED` con la decisión pendiente.
pub fn add_dependency(
    conn: &Connection,
    task: TaskId,
    depends_on: TaskId,
    now: i64,
) -> Result<Added, DepError> {
    if task == depends_on {
        return Err(DepError::SelfDependency);
    }
    let (t, d) = (
        repo::get_task(conn, task)?,
        repo::get_task(conn, depends_on)?,
    );
    if t.project_id != d.project_id {
        return Err(DepError::OtherProject);
    }
    if d.status == TaskStatus::Cancelled {
        return Err(DepError::DependsOnCancelled(d.code));
    }
    if would_cycle(conn, task, depends_on)? {
        return Err(DepError::Cycle {
            task: t.code,
            depends_on: d.code,
        });
    }
    conn.execute(
        "INSERT OR IGNORE INTO task_dependencies (task_id, depends_on_task_id, created_at) VALUES (?1, ?2, ?3)",
        params![task.to_string(), depends_on.to_string(), now],
    )?;
    let unmet = unmet_dependencies(conn, task)?;
    if unmet.is_empty() {
        return Ok(Added { waiting: false });
    }
    // Solo se frena a quien todavía no corre: un agente `RUNNING` sigue (la dependencia
    // queda registrada para el merge y la integración).
    let phrase = waiting_phrase(&unmet);
    let mut waiting = false;
    if let Some((agent, state)) = active_agent_of(conn, task)? {
        if state == AgentState::Ready {
            repo::set_agent_state(
                conn,
                agent,
                AgentState::WaitingDependency,
                Some(&phrase),
                now,
            )?;
            waiting = true;
        } else if state == AgentState::WaitingDependency {
            conn.execute(
                "UPDATE agents SET state_reason = ?2, updated_at = ?3 WHERE id = ?1",
                params![agent.to_string(), phrase, now],
            )?;
            waiting = true;
        }
    }
    let status = repo::get_task(conn, task)?.status;
    if waiting && matches!(status, TaskStatus::Ready | TaskStatus::Backlog) {
        repo::set_task_status(conn, task, TaskStatus::Waiting, Some(&phrase), now)?;
    }
    // La dependencia ya había fallado antes de declararla: no hay quien avise después.
    if waiting {
        for d in &unmet {
            if let Some(outcome) = failure_of(conn, d.task_id)?
                && block_on_failed_dependency(conn, task, &d.code, outcome, now)?.is_some()
            {
                break;
            }
        }
    }
    Ok(Added { waiting })
}

pub fn remove_dependency(
    conn: &Connection,
    task: TaskId,
    depends_on: TaskId,
) -> Result<bool, RepoError> {
    let n = conn.execute(
        "DELETE FROM task_dependencies WHERE task_id = ?1 AND depends_on_task_id = ?2",
        params![task.to_string(), depends_on.to_string()],
    )?;
    Ok(n > 0)
}

/// El agente vivo (no terminado) de una tarea, con su estado.
fn active_agent_of(
    conn: &Connection,
    task: TaskId,
) -> Result<Option<(AgentId, AgentState)>, RepoError> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT id, state FROM agents WHERE task_id = ?1
             AND state NOT IN ('COMPLETED','FAILED','CANCELLED') ORDER BY created_at DESC, rowid DESC LIMIT 1",
            [task.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.and_then(|(id, state)| Some((id.parse().ok()?, state.parse().ok()?))))
}

/// Bloquea al agente de `task` porque su dependencia `dep_code` falló o se canceló, y deja la
/// decisión pendiente (FLOW §9.3) con un recovery item. `None` si su estado no admite el
/// bloqueo (ya terminó, por ejemplo).
fn block_on_failed_dependency(
    conn: &Connection,
    task: TaskId,
    dep_code: &str,
    outcome: Finished,
    now: i64,
) -> Result<Option<String>, RepoError> {
    let Some((agent, state)) = active_agent_of(conn, task)? else {
        return Ok(None);
    };
    let because = format!(
        "{dep_code} {}: {DEPENDENCY_BLOCK_MARK}",
        if outcome == Finished::Failed {
            "falló"
        } else {
            "se canceló"
        }
    );
    match state {
        AgentState::Ready => {
            // READY → BLOCKED no existe: pasa por la espera.
            repo::set_agent_state(
                conn,
                agent,
                AgentState::WaitingDependency,
                Some(&because),
                now,
            )?;
            repo::set_agent_state(conn, agent, AgentState::Blocked, Some(&because), now)?;
        }
        AgentState::WaitingDependency | AgentState::Running => {
            repo::set_agent_state(conn, agent, AgentState::Blocked, Some(&because), now)?;
        }
        _ => return Ok(None),
    }
    let t = repo::get_task(conn, task)?;
    if matches!(
        t.status,
        TaskStatus::Waiting | TaskStatus::Ready | TaskStatus::Running
    ) {
        repo::set_task_status(conn, task, TaskStatus::Blocked, Some(&because), now)?;
    }
    repo::open_recovery_item(
        conn,
        t.project_id,
        Some(agent),
        None,
        "DEPENDENCY_FAILED",
        &because,
        now,
    )?;
    Ok(Some(because))
}

/// ¿La tarea ya falló o se canceló? (el agente puede estar `FAILED` con la tarea todavía
/// `RUNNING`: se puede recuperar, pero hasta entonces quien la espera no avanza.)
fn failure_of(conn: &Connection, task: TaskId) -> Result<Option<Finished>, RepoError> {
    match repo::get_task(conn, task)?.status {
        TaskStatus::Failed => return Ok(Some(Finished::Failed)),
        TaskStatus::Cancelled => return Ok(Some(Finished::Cancelled)),
        TaskStatus::Done => return Ok(None),
        _ => {}
    }
    let live: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM agents WHERE task_id = ?1
                        AND state NOT IN ('COMPLETED','FAILED','CANCELLED'))",
        [task.to_string()],
        |r| r.get(0),
    )?;
    if live {
        return Ok(None);
    }
    let last: Option<String> = conn
        .query_row(
            "SELECT state FROM agents WHERE task_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT 1",
            [task.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match last.as_deref() {
        Some("FAILED") => Some(Finished::Failed),
        Some("CANCELLED") => Some(Finished::Cancelled),
        _ => None,
    })
}

/// Qué cambió al terminar una tarea (para que el daemon actúe).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reaction {
    /// Todas las dependencias están hechas: el agente quedó `READY`.
    Released { agent: AgentId, task: TaskId },
    /// Una dependencia falló o se canceló: el agente quedó `BLOCKED` y espera decisión.
    Blocked {
        agent: AgentId,
        task: TaskId,
        because: String,
    },
}

/// Una tarea terminó: reevalúa a quienes la esperaban.
pub fn reconcile_dependents(
    conn: &Connection,
    finished: TaskId,
    outcome: Finished,
    now: i64,
) -> Result<Vec<Reaction>, RepoError> {
    let done = repo::get_task(conn, finished)?;
    let mut out = Vec::new();
    for dep in dependents_of(conn, finished)? {
        let Some((agent, state)) = active_agent_of(conn, dep)? else {
            continue;
        };
        match outcome {
            Finished::Done => {
                let still = unmet_dependencies(conn, dep)?;
                if !still.is_empty() {
                    // Aún le faltan otras: se actualiza la frase de espera.
                    conn.execute(
                        "UPDATE agents SET state_reason = ?2, updated_at = ?3
                         WHERE id = ?1 AND state = 'WAITING_DEPENDENCY'",
                        params![agent.to_string(), waiting_phrase(&still), now],
                    )?;
                    continue;
                }
                // Un agente bloqueado *por esta dependencia* se libera al cumplirse.
                let blocked_by_deps = state == AgentState::Blocked
                    && repo::get_agent(conn, agent)?
                        .state_reason
                        .is_some_and(|r| r.contains(DEPENDENCY_BLOCK_MARK));
                if state == AgentState::WaitingDependency || blocked_by_deps {
                    repo::set_agent_state(conn, agent, AgentState::Ready, None, now)?;
                    if matches!(
                        repo::get_task(conn, dep)?.status,
                        TaskStatus::Waiting | TaskStatus::Blocked
                    ) {
                        repo::set_task_status(conn, dep, TaskStatus::Ready, None, now)?;
                    }
                    out.push(Reaction::Released { agent, task: dep });
                }
            }
            Finished::Failed | Finished::Cancelled => {
                if let Some(because) =
                    block_on_failed_dependency(conn, dep, &done.code, outcome, now)?
                {
                    out.push(Reaction::Blocked {
                        agent,
                        task: dep,
                        because,
                    });
                }
            }
        }
    }
    Ok(out)
}

// --- milestones ---------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Milestone {
    pub id: MilestoneId,
    pub project_id: ProjectId,
    pub name: String,
    pub requires_approval: bool,
    pub status: MilestoneStatus,
    pub decided_at: Option<i64>,
    pub tasks_total: i64,
    pub tasks_done: i64,
}

pub fn create_milestone(
    conn: &Connection,
    project: ProjectId,
    name: &str,
    requires_approval: bool,
) -> Result<MilestoneId, RepoError> {
    let id = MilestoneId::new();
    conn.execute(
        "INSERT INTO milestones (id, project_id, name, requires_approval, status) VALUES (?1, ?2, ?3, ?4, 'OPEN')",
        params![id.to_string(), project.to_string(), name, requires_approval],
    )?;
    Ok(id)
}

pub fn assign_task(
    conn: &Connection,
    task: TaskId,
    milestone: Option<MilestoneId>,
    now: i64,
) -> Result<(), RepoError> {
    let n = conn.execute(
        "UPDATE tasks SET milestone_id = ?2, updated_at = ?3 WHERE id = ?1",
        params![task.to_string(), milestone.map(|m| m.to_string()), now],
    )?;
    if n == 0 {
        return Err(RepoError::NotFound {
            entity: "tarea",
            id: task.to_string(),
        });
    }
    Ok(())
}

fn milestone_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Milestone> {
    let id: String = r.get(0)?;
    let project: String = r.get(1)?;
    let status: String = r.get(4)?;
    Ok(Milestone {
        id: id.parse().map_err(|e| conv(0, e))?,
        project_id: project.parse().map_err(|e| conv(1, e))?,
        name: r.get(2)?,
        requires_approval: r.get::<_, i64>(3)? != 0,
        status: status.parse().map_err(|e| conv(4, e))?,
        decided_at: r.get(5)?,
        tasks_total: r.get(6)?,
        tasks_done: r.get(7)?,
    })
}

const MILESTONE_SELECT: &str =
    "SELECT m.id, m.project_id, m.name, m.requires_approval, m.status, m.decided_at,
        (SELECT COUNT(*) FROM tasks t WHERE t.milestone_id = m.id AND t.status <> 'CANCELLED'),
        (SELECT COUNT(*) FROM tasks t WHERE t.milestone_id = m.id AND t.status = 'DONE')
     FROM milestones m";

pub fn get_milestone(conn: &Connection, id: MilestoneId) -> Result<Milestone, RepoError> {
    conn.query_row(
        &format!("{MILESTONE_SELECT} WHERE m.id = ?1"),
        [id.to_string()],
        milestone_row,
    )
    .optional()?
    .ok_or(RepoError::NotFound {
        entity: "milestone",
        id: id.to_string(),
    })
}

pub fn milestones_of(conn: &Connection, project: ProjectId) -> Result<Vec<Milestone>, RepoError> {
    let mut stmt = conn.prepare(&format!(
        "{MILESTONE_SELECT} WHERE m.project_id = ?1 ORDER BY m.rowid"
    ))?;
    let rows = stmt.query_map([project.to_string()], milestone_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// El milestone de una tarea, si lo tiene.
pub fn milestone_of_task(
    conn: &Connection,
    task: TaskId,
) -> Result<Option<MilestoneId>, RepoError> {
    let id: Option<String> = conn
        .query_row(
            "SELECT milestone_id FROM tasks WHERE id = ?1",
            [task.to_string()],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(id.and_then(|i| i.parse().ok()))
}

/// Reevalúa un milestone: con todas sus tareas hechas pasa a `IN_REVIEW` (si pide
/// aprobación) o a `APPROVED`. Devuelve el estado nuevo si cambió (FLOW §9.4).
pub fn evaluate_milestone(
    conn: &Connection,
    id: MilestoneId,
    now: i64,
) -> Result<Option<MilestoneStatus>, RepoError> {
    let m = get_milestone(conn, id)?;
    let all_done = m.tasks_total > 0 && m.tasks_done == m.tasks_total;
    if !all_done
        || !matches!(
            m.status,
            MilestoneStatus::Open | MilestoneStatus::ChangesRequested
        )
    {
        return Ok(None);
    }
    let to = if m.requires_approval {
        MilestoneStatus::InReview
    } else {
        MilestoneStatus::Approved
    };
    let decided = (to == MilestoneStatus::Approved).then_some(now);
    conn.execute(
        "UPDATE milestones SET status = ?2, decided_at = COALESCE(?3, decided_at) WHERE id = ?1",
        params![id.to_string(), to.as_str(), decided],
    )?;
    Ok(Some(to))
}

/// La decisión del usuario en la revisión del gate (FLOW §9.4): `Approve`, `Request changes`
/// o `Hold`. Solo se decide un milestone que está en revisión (o en espera).
pub fn decide_milestone(
    conn: &Connection,
    id: MilestoneId,
    to: MilestoneStatus,
    now: i64,
) -> Result<(), RepoError> {
    let m = get_milestone(conn, id)?;
    let allowed = matches!(m.status, MilestoneStatus::InReview | MilestoneStatus::Held)
        && matches!(
            to,
            MilestoneStatus::Approved | MilestoneStatus::ChangesRequested | MilestoneStatus::Held
        );
    if !allowed {
        return Err(RepoError::Invalid(format!(
            "el milestone `{}` está {}: solo se decide en IN_REVIEW o HELD (aprobar, pedir cambios o retener)",
            m.name,
            m.status.as_str()
        )));
    }
    conn.execute(
        "UPDATE milestones SET status = ?2, decided_at = ?3 WHERE id = ?1",
        params![id.to_string(), to.as_str(), now],
    )?;
    Ok(())
}
