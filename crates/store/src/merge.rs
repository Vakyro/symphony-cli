//! Cola de integración, conflictos y revisiones (DB §3.H, P08.S7).

use rusqlite::{Connection, OptionalExtension, params};
use symphony_core::{
    AgentId, ConflictResolution, MergeStatus, ReviewRecommendation, ReviewStatus, TaskId,
};

use crate::repo::RepoError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeRequest {
    pub id: String,
    pub agent_id: AgentId,
    pub task_id: TaskId,
    pub target_branch: String,
    pub status: MergeStatus,
    pub preflight_json: Option<String>,
    pub requires_approval: bool,
    pub approved_at: Option<i64>,
    pub queue_position: Option<i64>,
    pub merge_commit: Option<String>,
    pub created_at: i64,
    pub merged_at: Option<i64>,
}

fn conv(idx: usize, e: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
}

const MR_COLS: &str =
    "id, agent_id, task_id, target_branch, status, preflight_json, requires_approval,
     approved_at, queue_position, merge_commit, created_at, merged_at";

fn mr_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<MergeRequest> {
    let (agent, task, status): (String, String, String) = (r.get(1)?, r.get(2)?, r.get(4)?);
    Ok(MergeRequest {
        id: r.get(0)?,
        agent_id: agent.parse().map_err(|e| conv(1, e))?,
        task_id: task.parse().map_err(|e| conv(2, e))?,
        target_branch: r.get(3)?,
        status: status.parse().map_err(|e| conv(4, e))?,
        preflight_json: r.get(5)?,
        requires_approval: r.get::<_, i64>(6)? != 0,
        approved_at: r.get(7)?,
        queue_position: r.get(8)?,
        merge_commit: r.get(9)?,
        created_at: r.get(10)?,
        merged_at: r.get(11)?,
    })
}

pub fn get_mr(conn: &Connection, id: &str) -> Result<MergeRequest, RepoError> {
    conn.query_row(
        &format!("SELECT {MR_COLS} FROM merge_requests WHERE id = ?1"),
        [id],
        mr_row,
    )
    .optional()?
    .ok_or(RepoError::NotFound {
        entity: "merge request",
        id: id.to_string(),
    })
}

/// La solicitud más reciente del agente.
pub fn latest_mr(conn: &Connection, agent: AgentId) -> Result<Option<MergeRequest>, RepoError> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {MR_COLS} FROM merge_requests WHERE agent_id = ?1
                 ORDER BY created_at DESC, rowid DESC LIMIT 1"
            ),
            [agent.to_string()],
            mr_row,
        )
        .optional()?)
}

/// Las pendientes de integrar (la cola), en orden de llegada.
pub fn queue(conn: &Connection, target_branch: &str) -> Result<Vec<MergeRequest>, RepoError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {MR_COLS} FROM merge_requests
         WHERE target_branch = ?1 AND status IN ('READY','QUEUED') ORDER BY created_at, rowid"
    ))?;
    let rows = stmt.query_map([target_branch], mr_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Abre la solicitud del agente o reaprovecha la que sigue sin cerrar (`PREFLIGHT`, `READY`,
/// `QUEUED` o `BLOCKED`): una por agente a la vez.
pub fn open_or_reuse(
    conn: &Connection,
    agent: AgentId,
    task: TaskId,
    target_branch: &str,
    requires_approval: bool,
    now: i64,
) -> Result<MergeRequest, RepoError> {
    let open: Option<String> = conn
        .query_row(
            "SELECT id FROM merge_requests WHERE agent_id = ?1
             AND status IN ('PREFLIGHT','READY','QUEUED','BLOCKED') ORDER BY created_at DESC LIMIT 1",
            [agent.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = open {
        conn.execute(
            "UPDATE merge_requests SET target_branch = ?2, requires_approval = ?3, status = 'PREFLIGHT'
             WHERE id = ?1",
            params![id, target_branch, requires_approval],
        )?;
        return get_mr(conn, &id);
    }
    let id = symphony_core::ToolCallId::new().to_string();
    conn.execute(
        "INSERT INTO merge_requests (id, agent_id, task_id, target_branch, status, requires_approval, created_at)
         VALUES (?1, ?2, ?3, ?4, 'PREFLIGHT', ?5, ?6)",
        params![id, agent.to_string(), task.to_string(), target_branch, requires_approval, now],
    )?;
    get_mr(conn, &id)
}

pub fn set_status(
    conn: &Connection,
    id: &str,
    status: MergeStatus,
    preflight_json: Option<&str>,
) -> Result<(), RepoError> {
    conn.execute(
        "UPDATE merge_requests SET status = ?2, preflight_json = COALESCE(?3, preflight_json) WHERE id = ?1",
        params![id, status.as_str(), preflight_json],
    )?;
    Ok(())
}

pub fn approve(conn: &Connection, id: &str, now: i64) -> Result<(), RepoError> {
    conn.execute(
        "UPDATE merge_requests SET approved_at = ?2 WHERE id = ?1 AND status <> 'MERGED'",
        params![id, now],
    )?;
    Ok(())
}

pub fn mark_merged(conn: &Connection, id: &str, commit: &str, now: i64) -> Result<(), RepoError> {
    conn.execute(
        "UPDATE merge_requests SET status = 'MERGED', merge_commit = ?2, merged_at = ?3, queue_position = NULL
         WHERE id = ?1",
        params![id, commit, now],
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub path: String,
    pub other_agent_id: Option<AgentId>,
    pub resolution: ConflictResolution,
}

/// Reemplaza los conflictos de la solicitud por los que se acaban de detectar.
pub fn replace_conflicts(
    conn: &Connection,
    mr: &str,
    conflicts: &[(String, Option<AgentId>)],
) -> Result<(), RepoError> {
    conn.execute(
        "DELETE FROM merge_conflicts WHERE merge_request_id = ?1",
        [mr],
    )?;
    for (path, other) in conflicts {
        conn.execute(
            "INSERT INTO merge_conflicts (id, merge_request_id, path, other_agent_id, resolution)
             VALUES (?1, ?2, ?3, ?4, 'PENDING')",
            params![
                symphony_core::ToolCallId::new().to_string(),
                mr,
                path,
                other.map(|a| a.to_string())
            ],
        )?;
    }
    Ok(())
}

pub fn conflicts_of(conn: &Connection, mr: &str) -> Result<Vec<Conflict>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT path, other_agent_id, resolution FROM merge_conflicts
         WHERE merge_request_id = ?1 ORDER BY path",
    )?;
    let rows = stmt.query_map([mr], |r| {
        let other: Option<String> = r.get(1)?;
        let res: String = r.get(2)?;
        Ok(Conflict {
            path: r.get(0)?,
            other_agent_id: other.and_then(|o| o.parse().ok()),
            resolution: res.parse().map_err(|e| conv(2, e))?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn resolve_conflicts(
    conn: &Connection,
    mr: &str,
    resolution: ConflictResolution,
    now: i64,
) -> Result<usize, RepoError> {
    Ok(conn.execute(
        "UPDATE merge_conflicts SET resolution = ?2, resolved_at = ?3
         WHERE merge_request_id = ?1 AND resolution = 'PENDING'",
        params![mr, resolution.as_str(), now],
    )?)
}

// --- reviews ------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub id: String,
    pub target_agent_id: AgentId,
    pub reviewer_agent_id: AgentId,
    pub merge_request_id: Option<String>,
    pub status: ReviewStatus,
    pub recommendation: Option<ReviewRecommendation>,
    pub findings_json: Option<String>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
}

fn review_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Review> {
    let (target, reviewer, status): (String, String, String) = (r.get(1)?, r.get(2)?, r.get(4)?);
    let rec: Option<String> = r.get(5)?;
    Ok(Review {
        id: r.get(0)?,
        target_agent_id: target.parse().map_err(|e| conv(1, e))?,
        reviewer_agent_id: reviewer.parse().map_err(|e| conv(2, e))?,
        merge_request_id: r.get(3)?,
        status: status.parse().map_err(|e| conv(4, e))?,
        recommendation: rec.and_then(|x| x.parse().ok()),
        findings_json: r.get(6)?,
        created_at: r.get(7)?,
        finished_at: r.get(8)?,
    })
}

const REVIEW_COLS: &str = "id, target_agent_id, reviewer_agent_id, merge_request_id, status,
     recommendation, findings_json, created_at, finished_at";

pub fn insert_review(
    conn: &Connection,
    target: AgentId,
    reviewer: AgentId,
    mr: Option<&str>,
    now: i64,
) -> Result<String, RepoError> {
    let id = symphony_core::ToolCallId::new().to_string();
    conn.execute(
        "INSERT INTO reviews (id, target_agent_id, reviewer_agent_id, merge_request_id, status, created_at)
         VALUES (?1, ?2, ?3, ?4, 'RUNNING', ?5)",
        params![id, target.to_string(), reviewer.to_string(), mr, now],
    )?;
    Ok(id)
}

/// La revisión que hace este agente, si es un revisor.
pub fn review_by_reviewer(
    conn: &Connection,
    reviewer: AgentId,
) -> Result<Option<Review>, RepoError> {
    Ok(conn
        .query_row(
            &format!("SELECT {REVIEW_COLS} FROM reviews WHERE reviewer_agent_id = ?1"),
            [reviewer.to_string()],
            review_row,
        )
        .optional()?)
}

pub fn finish_review(
    conn: &Connection,
    id: &str,
    status: ReviewStatus,
    recommendation: Option<ReviewRecommendation>,
    findings_json: Option<&str>,
    now: i64,
) -> Result<(), RepoError> {
    conn.execute(
        "UPDATE reviews SET status = ?2, recommendation = ?3, findings_json = ?4, finished_at = ?5 WHERE id = ?1",
        params![
            id,
            status.as_str(),
            recommendation.map(|r| r.as_str()),
            findings_json,
            now
        ],
    )?;
    Ok(())
}

pub fn reviews_of_target(conn: &Connection, target: AgentId) -> Result<Vec<Review>, RepoError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {REVIEW_COLS} FROM reviews WHERE target_agent_id = ?1 ORDER BY created_at DESC, rowid DESC"
    ))?;
    let rows = stmt.query_map([target.to_string()], review_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}
