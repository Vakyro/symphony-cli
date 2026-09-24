use rusqlite::{Connection, params};
use symphony_core::{AgentState, RunStatus, TaskStatus};

use super::*;

const PHASE1_TABLES: [&str; 19] = [
    "projects",
    "sessions",
    "tasks",
    "agents",
    "worktrees",
    "agent_runs",
    "executor_changes",
    "messages",
    "providers",
    "models",
    "provider_failures",
    "events",
    "tool_calls",
    "checkpoints",
    "checkpoint_refs",
    "blobs",
    "context_objects",
    "handoffs",
    "recovery_items",
];

/// Proyecto, sesión, dos tasks, un proveedor y un modelo.
fn seeded() -> Connection {
    let conn = open_in_memory().unwrap();
    conn.execute_batch(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','demo','/repo','main',0);
         INSERT INTO sessions (id, project_id, status, started_at) VALUES ('s1','p1','ACTIVE',0);
         INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at)
             VALUES ('t1','p1','T-1','WORK','uno','READY',0,0), ('t2','p1','T-2','WORK','dos','READY',0,0);
         INSERT INTO providers (id, display_name, cli_name, adapter_mode, setup_state) VALUES ('anthropic','Claude','claude','CLI','READY');
         INSERT INTO models (id, provider_id, cli_model_id, display_name, discovered_at) VALUES ('claude/sonnet','anthropic','sonnet','Sonnet',0);",
    )
    .unwrap();
    conn
}

fn insert_agent(
    conn: &Connection,
    id: &str,
    task: &str,
    number: i64,
    state: &str,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, requested_model_id,
                             failover_policy, context_mode, created_at, updated_at)
         VALUES (?1, 'p1', 's1', ?2, ?3, ?4, 'EXACT', 'claude/sonnet', 'ANY', 'BALANCED', 0, 0)",
        params![id, task, number, state],
    )
}

fn insert_run(
    conn: &Connection,
    id: &str,
    agent: &str,
    seq: i64,
    ended: Option<i64>,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO agent_runs (id, agent_id, seq, provider_id, model_id, status, started_at, ended_at)
         VALUES (?1, ?2, ?3, 'anthropic', 'claude/sonnet', 'RUNNING', 0, ?4)",
        params![id, agent, seq, ended],
    )
}

#[test]
fn migrations_are_valid() {
    MIGRATIONS.validate().unwrap();
}

#[test]
fn file_db_has_phase1_schema_and_pragmas() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data").join("symphony.db");
    let conn = open(&path).unwrap();

    let mut tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut expected: Vec<String> = PHASE1_TABLES.iter().map(|s| s.to_string()).collect();
    tables.sort();
    expected.sort();
    assert_eq!(tables, expected);

    let pragma = |name: &str| -> String {
        conn.query_row(&format!("PRAGMA {name}"), [], |r| {
            r.get::<_, rusqlite::types::Value>(0)
        })
        .map(|v| match v {
            rusqlite::types::Value::Integer(i) => i.to_string(),
            rusqlite::types::Value::Text(t) => t,
            other => format!("{other:?}"),
        })
        .unwrap()
    };
    assert_eq!(pragma("user_version"), "1");
    assert_eq!(pragma("journal_mode"), "wal");
    assert_eq!(pragma("foreign_keys"), "1");
    assert_eq!(pragma("synchronous"), "1"); // NORMAL
    assert_eq!(pragma("busy_timeout"), "5000");
    let fk_problems: i64 = conn
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(fk_problems, 0);

    drop(conn);
    // Reabrir no vuelve a migrar ni falla.
    let conn = open(&path).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn agents_table_never_stores_the_current_model() {
    // AGENT ≠ MODEL (PLAN §2.1, CONSTRAINTS A1): el modelo vive en el run abierto.
    let conn = open_in_memory().unwrap();
    let cols: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('agents')")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for forbidden in [
        "model_id",
        "current_model_id",
        "provider_id",
        "current_provider_id",
    ] {
        assert!(
            !cols.iter().any(|c| c == forbidden),
            "agents no puede tener `{forbidden}`"
        );
    }
    assert!(cols.iter().any(|c| c == "requested_model_id"));
}

#[test]
fn only_one_active_agent_per_task() {
    let conn = seeded();
    insert_agent(&conn, "a1", "t1", 1, "RUNNING").unwrap();
    assert!(
        insert_agent(&conn, "a2", "t1", 2, "READY").is_err(),
        "dos agentes activos en la misma task"
    );
    // Otra task sí.
    insert_agent(&conn, "a3", "t2", 3, "RUNNING").unwrap();
    // Si el primero termina, la task puede tener un agente nuevo.
    conn.execute("UPDATE agents SET state='FAILED' WHERE id='a1'", [])
        .unwrap();
    insert_agent(&conn, "a2", "t1", 2, "READY").unwrap();
}

#[test]
fn only_one_open_run_per_agent() {
    let conn = seeded();
    insert_agent(&conn, "a1", "t1", 1, "RUNNING").unwrap();
    insert_run(&conn, "r1", "a1", 1, None).unwrap();
    assert!(
        insert_run(&conn, "r2", "a1", 2, None).is_err(),
        "dos executors vivos en un agente"
    );
    conn.execute(
        "UPDATE agent_runs SET ended_at = 10, status = 'HANDED_OFF' WHERE id='r1'",
        [],
    )
    .unwrap();
    insert_run(&conn, "r2", "a1", 2, None).unwrap();
    // seq único por agente.
    conn.execute("UPDATE agent_runs SET ended_at = 20 WHERE id='r2'", [])
        .unwrap();
    assert!(insert_run(&conn, "r3", "a1", 2, Some(30)).is_err());
}

#[test]
fn foreign_keys_are_enforced() {
    let conn = seeded();
    let err = conn.execute(
        "INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at)
         VALUES ('tx','no-existe','T-9','WORK','x','READY',0,0)",
        [],
    );
    assert!(err.is_err());
    assert!(insert_run(&conn, "r1", "agente-fantasma", 1, None).is_err());
}

#[test]
fn enum_checks_match_core_enums() {
    let conn = seeded();
    for (i, state) in AgentState::ALL.iter().enumerate() {
        let id = format!("a{i}");
        let task = if matches!(
            state,
            AgentState::Completed | AgentState::Failed | AgentState::Cancelled
        ) {
            "t1"
        } else {
            "t2"
        };
        // Solo un activo por task: los activos se van cerrando antes de insertar el siguiente.
        conn.execute(
            "UPDATE agents SET state='CANCELLED' WHERE task_id=?1",
            [task],
        )
        .unwrap();
        insert_agent(&conn, &id, task, i as i64 + 1, state.as_str())
            .unwrap_or_else(|e| panic!("{state}: {e}"));
    }
    assert!(insert_agent(&conn, "bad", "t1", 99, "SLEEPING").is_err());

    for status in TaskStatus::ALL {
        conn.execute(
            "UPDATE tasks SET status=?1 WHERE id='t1'",
            [status.as_str()],
        )
        .unwrap();
    }
    assert!(
        conn.execute("UPDATE tasks SET status='FINISHED' WHERE id='t1'", [])
            .is_err()
    );

    conn.execute("UPDATE agents SET state='CANCELLED'", [])
        .unwrap();
    insert_agent(&conn, "ar", "t1", 100, "RUNNING").unwrap();
    insert_run(&conn, "r1", "ar", 1, None).unwrap();
    for status in RunStatus::ALL {
        conn.execute(
            "UPDATE agent_runs SET status=?1 WHERE id='r1'",
            [status.as_str()],
        )
        .unwrap();
    }
    assert!(
        conn.execute("UPDATE agent_runs SET status='ZOMBIE' WHERE id='r1'", [])
            .is_err()
    );
}

#[test]
fn execution_mode_requires_its_target() {
    let conn = seeded();
    let insert = |mode: &str, model: Option<&str>, profile: Option<&str>| {
        conn.execute(
            "INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode,
                                 requested_model_id, requested_profile_id, failover_policy, context_mode, created_at, updated_at)
             VALUES (?1, 'p1', 's1', 't2', ?2, 'CANCELLED', ?3, ?4, ?5, 'ANY', 'BALANCED', 0, 0)",
            params![format!("m-{mode}-{}", model.is_some()), agent_number(mode, model), mode, model, profile],
        )
    };
    assert!(insert("EXACT", None, None).is_err());
    assert!(insert("EXACT", Some("claude/sonnet"), None).is_ok());
    assert!(insert("PROFILE", None, None).is_err());
    assert!(insert("PROFILE", None, Some("@code")).is_ok());
    assert!(insert("DECIDE_LATER", None, None).is_ok());
}

fn agent_number(mode: &str, model: Option<&str>) -> i64 {
    mode.len() as i64 * 10 + i64::from(model.is_some())
}

#[test]
fn json_columns_are_validated() {
    let conn = seeded();
    let ok = conn.execute(
        "INSERT INTO events (project_id, type, source, payload_json, occurred_at) VALUES ('p1','AgentStarted','HOOK','{\"a\":1}',0)",
        [],
    );
    assert!(ok.is_ok());
    let bad = conn.execute(
        "INSERT INTO events (project_id, type, source, payload_json, occurred_at) VALUES ('p1','AgentStarted','HOOK','{no es json',0)",
        [],
    );
    assert!(bad.is_err());
}
