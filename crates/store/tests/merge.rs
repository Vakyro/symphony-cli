//! Cola de integración, conflictos y revisiones en el almacén (P08.S7).
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use rusqlite::Connection;
use symphony_core::{
    AgentId, ConflictResolution, MergeStatus, ReviewRecommendation, ReviewStatus, TaskId,
};
use symphony_store::merge::{
    approve, conflicts_of, finish_review, insert_review, latest_mr, mark_merged, open_or_reuse,
    replace_conflicts, resolve_conflicts, review_by_reviewer, reviews_of_target, set_status,
};
use symphony_store::open_in_memory;

fn seeded() -> (Connection, AgentId, TaskId) {
    let conn = open_in_memory().unwrap();
    let (agent, task) = (AgentId::new(), TaskId::new());
    conn.execute_batch(&format!(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','d','/r','main',0);
         INSERT INTO sessions (id, project_id, status, started_at) VALUES ('s1','p1','ACTIVE',0);
         INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at) VALUES ('{task}','p1','T-1','WORK','x','DONE',0,0);
         INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, failover_policy, context_mode, created_at, updated_at)
             VALUES ('{agent}','p1','s1','{task}',1,'COMPLETED','DECIDE_LATER','ANY','BALANCED',0,0);"
    ))
    .unwrap();
    (conn, agent, task)
}

#[test]
fn one_open_request_per_agent_is_reused_until_it_is_merged() {
    let (conn, agent, task) = seeded();
    let first = open_or_reuse(&conn, agent, task, "symphony/chat", true, 1).unwrap();
    assert_eq!(first.status, MergeStatus::Preflight);
    set_status(
        &conn,
        &first.id,
        MergeStatus::Blocked,
        Some("{\"checks\":[]}"),
    )
    .unwrap();

    let again = open_or_reuse(&conn, agent, task, "symphony/chat", false, 2).unwrap();
    assert_eq!(again.id, first.id, "se reaprovecha la abierta");
    assert_eq!(again.status, MergeStatus::Preflight);
    assert!(!again.requires_approval);
    assert_eq!(again.preflight_json.as_deref(), Some("{\"checks\":[]}"));

    set_status(&conn, &first.id, MergeStatus::Ready, None).unwrap();
    approve(&conn, &first.id, 3).unwrap();
    mark_merged(&conn, &first.id, "abc123", 4).unwrap();
    let merged = latest_mr(&conn, agent).unwrap().unwrap();
    assert_eq!(
        (
            merged.status,
            merged.merge_commit.as_deref(),
            merged.approved_at
        ),
        (MergeStatus::Merged, Some("abc123"), Some(3))
    );

    // Una vez integrada, una solicitud nueva abre otra fila.
    let next = open_or_reuse(&conn, agent, task, "symphony/chat", true, 5).unwrap();
    assert_ne!(next.id, first.id);
}

#[test]
fn conflicts_are_replaced_and_resolved_in_bulk() {
    let (conn, agent, task) = seeded();
    let mr = open_or_reuse(&conn, agent, task, "symphony/chat", true, 1).unwrap();
    replace_conflicts(
        &conn,
        &mr.id,
        &[("a.js".into(), None), ("b.js".into(), Some(agent))],
    )
    .unwrap();
    let rows = conflicts_of(&conn, &mr.id).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|c| c.resolution == ConflictResolution::Pending)
    );
    assert_eq!(rows[1].other_agent_id, Some(agent));

    // Un preflight nuevo reemplaza la lista.
    replace_conflicts(&conn, &mr.id, &[("a.js".into(), None)]).unwrap();
    assert_eq!(conflicts_of(&conn, &mr.id).unwrap().len(), 1);
    assert_eq!(
        resolve_conflicts(&conn, &mr.id, ConflictResolution::Manual, 9).unwrap(),
        1
    );
    assert_eq!(
        conflicts_of(&conn, &mr.id).unwrap()[0].resolution,
        ConflictResolution::Manual
    );
    // Lo ya resuelto no se vuelve a tocar.
    assert_eq!(
        resolve_conflicts(&conn, &mr.id, ConflictResolution::Aborted, 10).unwrap(),
        0
    );
}

#[test]
fn a_review_only_recommends() {
    let (conn, target, _) = seeded();
    let (reviewer_task, reviewer) = (TaskId::new(), AgentId::new());
    conn.execute_batch(&format!(
        "INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at) VALUES ('{reviewer_task}','p1','T-2','REVIEW','r','RUNNING',0,0);
         INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, failover_policy, context_mode, created_at, updated_at)
             VALUES ('{reviewer}','p1','s1','{reviewer_task}',2,'RUNNING','DECIDE_LATER','ANY','BALANCED',0,0);"
    ))
    .unwrap();
    let id = insert_review(&conn, target, reviewer, None, 1).unwrap();
    let r = review_by_reviewer(&conn, reviewer).unwrap().unwrap();
    assert_eq!((r.status, r.recommendation), (ReviewStatus::Running, None));

    finish_review(
        &conn,
        &id,
        ReviewStatus::Done,
        Some(ReviewRecommendation::RequestChanges),
        Some("[{\"text\":\"falta un caso\"}]"),
        5,
    )
    .unwrap();
    let r = reviews_of_target(&conn, target).unwrap().remove(0);
    assert_eq!(r.recommendation, Some(ReviewRecommendation::RequestChanges));
    assert_eq!(r.finished_at, Some(5));
    // La tabla solo conoce tres recomendaciones: no hay «MERGE».
    assert!(
        conn.execute(
            "UPDATE reviews SET recommendation = 'MERGE' WHERE id = ?1",
            [&id]
        )
        .is_err()
    );
}
