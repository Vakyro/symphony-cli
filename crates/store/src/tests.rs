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

/// Tablas que añade la migración 002 (salud, uso y routing).
const PHASE4_TABLES: [&str; 7] = [
    "provider_accounts",
    "provider_health",
    "usage_records",
    "profiles",
    "profile_models",
    "routing_decisions",
    "routing_candidates",
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
fn file_db_has_the_schema_and_pragmas() {
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
    let mut expected: Vec<String> = PHASE1_TABLES
        .iter()
        .chain(PHASE4_TABLES.iter())
        .map(|s| s.to_string())
        .collect();
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
    assert_eq!(pragma("user_version"), "2");
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
        2
    );
}

/// Una base de v0.1 (migración 001) con datos reales pasa a la 002 sin perder nada y con las
/// FKs nuevas: las tablas hijas siguen enlazadas y la integridad referencial sigue activa.
#[test]
fn migration_002_keeps_v01_data_and_adds_the_pending_foreign_keys() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    MIGRATIONS.to_version(&mut conn, 1).unwrap();
    conn.execute_batch(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','demo','/repo','main',0);
         INSERT INTO sessions (id, project_id, status, started_at) VALUES ('s1','p1','ACTIVE',0);
         INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at) VALUES ('t1','p1','T-1','WORK','uno','RUNNING',0,0);
         INSERT INTO providers (id, display_name, cli_name, adapter_mode, setup_state) VALUES ('anthropic','Claude','claude','CLI','READY'), ('openai','Codex','codex','CLI','READY');
         INSERT INTO models (id, provider_id, cli_model_id, display_name, discovered_at) VALUES ('claude/sonnet','anthropic','sonnet','Sonnet',0), ('openai/sol','openai','sol','Sol',0);
         INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, requested_model_id, failover_policy, context_mode, created_at, updated_at)
             VALUES ('a1','p1','s1','t1',1,'RUNNING','EXACT','claude/sonnet','ANY','BALANCED',0,0);
         INSERT INTO agent_runs (id, agent_id, seq, provider_id, model_id, status, end_reason, started_at, ended_at)
             VALUES ('r1','a1',1,'anthropic','claude/sonnet','HANDED_OFF','QUOTA_EXHAUSTED',0,5);
         INSERT INTO agent_runs (id, agent_id, seq, provider_id, model_id, status, started_at)
             VALUES ('r2','a1',2,'openai','openai/sol','RUNNING',6);
         INSERT INTO provider_failures (id, provider_id, model_id, run_id, failure_type, message, occurred_at)
             VALUES ('f1','anthropic','claude/sonnet','r1','DAILY_QUOTA','sin cuota',5);
         INSERT INTO executor_changes (id, agent_id, from_run_id, to_run_id, reason, failure_id, occurred_at)
             VALUES ('c1','a1','r1','r2','FAILOVER','f1',6);
         INSERT INTO messages (id, agent_id, run_id, role, content, created_at) VALUES ('m1','a1','r2','USER','hola',7);",
    )
    .unwrap();

    MIGRATIONS.to_latest(&mut conn).unwrap();

    let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
    assert_eq!(count("SELECT COUNT(*) FROM agents"), 1);
    assert_eq!(count("SELECT COUNT(*) FROM agent_runs"), 2);
    assert_eq!(count("SELECT COUNT(*) FROM provider_failures"), 1);
    assert_eq!(count("SELECT COUNT(*) FROM executor_changes"), 1);
    assert_eq!(count("SELECT COUNT(*) FROM messages"), 1);
    assert_eq!(count("SELECT COUNT(*) FROM pragma_foreign_key_check"), 0);

    // Las filas antiguas conservan sus datos y quedan con la cuenta «default».
    let (account, reason): (String, String) = conn
        .query_row(
            "SELECT account_id, end_reason FROM agent_runs WHERE id='r1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (account.as_str(), reason.as_str()),
        ("acct-anthropic", "QUOTA_EXHAUSTED")
    );
    assert_eq!(count("SELECT COUNT(*) FROM provider_accounts"), 2);
    assert_eq!(count("SELECT COUNT(*) FROM profiles WHERE builtin = 1"), 7);

    // Las FKs nuevas existen (y las de las hijas siguen apuntando a las tablas reconstruidas).
    let fk_targets = |table: &str| -> Vec<String> {
        conn.prepare(&format!(
            "SELECT \"table\" FROM pragma_foreign_key_list('{table}')"
        ))
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    };
    assert!(fk_targets("agent_runs").contains(&"provider_accounts".to_string()));
    assert!(fk_targets("agent_runs").contains(&"routing_decisions".to_string()));
    assert!(fk_targets("agents").contains(&"profiles".to_string()));
    assert!(fk_targets("provider_failures").contains(&"provider_accounts".to_string()));
    assert!(fk_targets("messages").contains(&"agents".to_string()));

    // La integridad referencial sigue activa y los índices únicos parciales se recrearon.
    assert!(
        conn.execute(
            "INSERT INTO messages (id, agent_id, role, content, created_at) VALUES ('bad','nadie','USER','x',0)",
            []
        )
        .is_err()
    );
    assert!(
        conn.execute(
            "INSERT INTO agent_runs (id, agent_id, seq, provider_id, model_id, status, started_at) VALUES ('r3','a1',3,'openai','openai/sol','RUNNING',9)",
            []
        )
        .is_err(),
        "un solo run abierto por agente"
    );
    assert!(
        conn.execute(
            "INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, requested_profile_id, failover_policy, context_mode, created_at, updated_at)
             VALUES ('a2','p1','s1','t1',2,'READY','PROFILE','@no-existe','ANY','BALANCED',0,0)",
            []
        )
        .is_err(),
        "un profile inexistente se rechaza"
    );
}

#[test]
fn provider_health_never_stores_a_quota_percentage_without_certainty() {
    let conn = seeded();
    let ins = |certainty: &str, remaining: &str, id: &str| {
        conn.execute(
            &format!(
                "INSERT INTO provider_health (id, provider_id, state, quota_certainty, quota_remaining, updated_at)
                 VALUES ('{id}','anthropic','HEALTHY','{certainty}',{remaining},0)"
            ),
            [],
        )
    };
    assert!(ins("KNOWN", "0.4", "h1").is_ok());
    assert!(ins("ESTIMATED", "0.4", "h2").is_err());
    assert!(ins("UNKNOWN", "0.4", "h3").is_err());
    // Una sola fila por alcance: el nivel proveedor (model_id NULL) también es único.
    assert!(ins("UNKNOWN", "NULL", "h4").is_err());
    conn.execute("DELETE FROM provider_health", []).unwrap();
    assert!(ins("UNKNOWN", "NULL", "h5").is_ok());
    assert!(ins("UNKNOWN", "NULL", "h6").is_err());
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
