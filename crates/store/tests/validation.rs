//! Resultados de validación y la migración 005 (P08.S6).
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use rusqlite::Connection;
use symphony_core::{AgentId, CheckName, CheckStatus, ValidationStatus, ValidationTrigger};
use symphony_store::validation::{
    NewCheck, NewValidationRun, failed_runs, finish_run, insert_check, insert_run, runs_of,
};
use symphony_store::{MIGRATIONS, open_in_memory};

fn seeded() -> (Connection, AgentId) {
    let conn = open_in_memory().unwrap();
    let agent = AgentId::new();
    conn.execute_batch(&format!(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','d','/r','main',0);
         INSERT INTO sessions (id, project_id, status, started_at) VALUES ('s1','p1','ACTIVE',0);
         INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at) VALUES ('t1','p1','T-1','WORK','x','RUNNING',0,0);
         INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, failover_policy, context_mode, created_at, updated_at)
             VALUES ('{agent}','p1','s1','t1',1,'TESTING','DECIDE_LATER','ANY','BALANCED',0,0);"
    ))
    .unwrap();
    (conn, agent)
}

fn run(conn: &Connection, agent: AgentId, id: &str, tier: u8, at: i64, status: ValidationStatus) {
    insert_run(
        conn,
        &NewValidationRun {
            id: id.into(),
            agent_id: agent,
            run_id: None,
            tier,
            trigger: ValidationTrigger::Auto,
        },
        at,
    )
    .unwrap();
    finish_run(conn, id, status, at + 5).unwrap();
}

#[test]
fn runs_and_checks_roundtrip_newest_first() {
    let (conn, agent) = seeded();
    run(&conn, agent, "v1", 1, 10, ValidationStatus::Pass);
    run(&conn, agent, "v2", 2, 20, ValidationStatus::Fail);
    insert_check(
        &conn,
        &NewCheck {
            id: "c1".into(),
            validation_run_id: "v2".into(),
            name: CheckName::TargetedTests,
            status: CheckStatus::Fail,
            passed: Some(3),
            failed: Some(1),
            summary: Some("auth.refresh.test.ts → expected 200, received 401".into()),
            output_object_id: None,
        },
    )
    .unwrap();

    assert_eq!(failed_runs(&conn, agent).unwrap(), 1);
    let runs = runs_of(&conn, agent, 10).unwrap();
    assert_eq!(
        runs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        ["v2", "v1"]
    );
    assert_eq!((runs[0].tier, runs[0].status.as_str()), (2, "FAIL"));
    assert_eq!(runs[0].checks.len(), 1);
    let c = &runs[0].checks[0];
    assert_eq!(
        (c.name.as_str(), c.passed, c.failed),
        ("targeted_tests", Some(3), Some(1))
    );
    assert!(runs[1].checks.is_empty());
    assert_eq!(runs_of(&conn, agent, 1).unwrap().len(), 1);
}

#[test]
fn enum_values_are_checked_by_the_table() {
    let (conn, agent) = seeded();
    assert!(
        conn.execute(
            "INSERT INTO validation_runs (id, agent_id, tier, trigger, status) VALUES ('x', ?1, 4, 'AUTO', 'RUNNING')",
            [agent.to_string()]
        )
        .is_err(),
        "tier 4 no existe"
    );
    assert!(
        conn.execute(
            "INSERT INTO validation_runs (id, agent_id, tier, trigger, status) VALUES ('y', ?1, 1, 'SOMETIMES', 'RUNNING')",
            [agent.to_string()]
        )
        .is_err()
    );
}

/// Una base en la 004 con recovery items pasa a la 005 sin perderlos y acepta el tipo nuevo.
#[test]
fn migration_005_keeps_recovery_items_and_adds_validation_failed() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    MIGRATIONS.to_version(&mut conn, 4).unwrap();
    conn.execute_batch(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','d','/r','main',0);
         INSERT INTO recovery_items (id, project_id, kind, detail, status, created_at)
             VALUES ('r1','p1','EXECUTOR_EXITED','murió','OPEN',1), ('r2','p1','MERGE_CONFLICT','choca','RESOLVED',2);",
    )
    .unwrap();
    assert!(
        conn.execute(
            "INSERT INTO recovery_items (id, project_id, kind, detail, status, created_at) VALUES ('x','p1','VALIDATION_FAILED','n','OPEN',3)",
            []
        )
        .is_err(),
        "antes de la 005 el tipo no existe"
    );

    MIGRATIONS.to_latest(&mut conn).unwrap();

    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM recovery_items", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);
    conn.execute(
        "INSERT INTO recovery_items (id, project_id, kind, detail, status, created_at) VALUES ('x','p1','VALIDATION_FAILED','n','OPEN',3)",
        [],
    )
    .unwrap();
    let open: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM recovery_items WHERE status = 'OPEN'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(open, 2, "el índice parcial y los datos siguen");
    let fk: i64 = conn
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(fk, 0);
}
