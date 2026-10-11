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

/// Tablas que añade la migración 003 (contexto, hechos, skills y MCP). `context_fts` es virtual
/// y SQLite crea sus tablas de apoyo (`context_fts_data`, `_idx`, `_docsize`, `_config`).
const PHASE3_TABLES: [&str; 7] = [
    "context_chunks",
    "context_fts",
    "handoff_items",
    "context_retrievals",
    "project_facts",
    "skills",
    "mcp_servers",
];

/// Tablas que añade la migración 004 (runtime multiagente).
const PHASE2_TABLES: [&str; 9] = [
    "milestones",
    "task_dependencies",
    "processes",
    "resource_samples",
    "validation_runs",
    "validation_checks",
    "merge_requests",
    "merge_conflicts",
    "reviews",
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
    // Las tablas de apoyo de FTS5 no son del esquema propio.
    tables.retain(|t| !t.starts_with("context_fts_"));
    let mut expected: Vec<String> = PHASE1_TABLES
        .iter()
        .chain(PHASE4_TABLES.iter())
        .chain(PHASE3_TABLES.iter())
        .chain(PHASE2_TABLES.iter())
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
    assert_eq!(pragma("user_version"), "4");
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
        4
    );
}

#[test]
fn context_search_finds_chunks_and_follows_inserts_updates_and_deletes() {
    let conn = seeded();
    conn.execute_batch(
        "INSERT INTO blobs (hash, size_bytes, stored_bytes, created_at) VALUES ('h1', 10, 5, 0);
         INSERT INTO context_objects (id, uri, project_id, kind, blob_hash, created_at)
             VALUES ('o1', 'ctx://file/auth.ts', 'p1', 'FILE', 'h1', 0);
         INSERT INTO context_chunks (object_id, seq, start_line, end_line, text) VALUES
             ('o1', 0, 1, 20, 'export function refresh(token) { return rotate(token); }'),
             ('o1', 1, 21, 40, 'class SessionStore { constructor() { this.cache = new Map(); } }');",
    )
    .unwrap();
    let hits = |q: &str| -> Vec<i64> {
        conn.prepare(
            "SELECT rowid FROM context_fts WHERE context_fts MATCH ?1 ORDER BY bm25(context_fts)",
        )
        .unwrap()
        .query_map([q], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    };
    assert_eq!(hits("refresh").len(), 1);
    assert_eq!(hits("SessionStore").len(), 1);
    assert!(hits("inexistente").is_empty());

    // Actualizar el texto reindexa; borrar el chunk lo saca del índice.
    conn.execute(
        "UPDATE context_chunks SET text = 'handler del login con cookies' WHERE seq = 0",
        [],
    )
    .unwrap();
    assert!(hits("refresh").is_empty());
    assert_eq!(hits("cookies").len(), 1);
    conn.execute("DELETE FROM context_chunks WHERE seq = 0", [])
        .unwrap();
    assert!(hits("cookies").is_empty());
    // El índice sigue íntegro.
    conn.execute(
        "INSERT INTO context_fts(context_fts) VALUES ('integrity-check')",
        [],
    )
    .unwrap();
}

#[test]
fn only_one_current_fact_per_key_and_a_superseded_one_can_coexist() {
    let conn = seeded();
    let ins = |id: &str, status: &str| {
        conn.execute(
            &format!("INSERT INTO project_facts (id, project_id, key, value, kind, status, created_at) VALUES ('{id}','p1','auth_storage','v','DECISION','{status}',0)"),
            [],
        )
    };
    assert!(ins("f1", "CURRENT").is_ok());
    assert!(
        ins("f2", "CURRENT").is_err(),
        "un solo valor vigente por clave"
    );
    assert!(ins("f3", "SUPERSEDED").is_ok());
    assert!(ins("f4", "CONFLICT").is_ok());
    assert!(ins("f5", "HECHO").is_err());
}

#[test]
fn skills_and_mcp_servers_are_unique_per_scope_and_project_scope_needs_a_project() {
    let conn = seeded();
    let skill = |id: &str, scope: &str, project: &str| {
        conn.execute(
            &format!("INSERT INTO skills (id, name, scope, project_id, path, updated_at) VALUES ('{id}','review','{scope}',{project},'/x',0)"),
            [],
        )
    };
    assert!(skill("s1", "USER", "NULL").is_ok());
    assert!(
        skill("s2", "USER", "NULL").is_err(),
        "mismo nombre y ámbito"
    );
    assert!(
        skill("s3", "PROJECT", "'p1'").is_ok(),
        "un PROJECT con el mismo nombre convive con el USER"
    );
    assert!(
        skill("s4", "PROJECT", "NULL").is_err(),
        "PROJECT exige proyecto"
    );
    assert!(
        conn.execute(
            "INSERT INTO mcp_servers (id, name, scope, transport, share_mode) VALUES ('m1','github','USER','STDIO','SHARED')",
            []
        )
        .is_ok()
    );
    assert!(
        conn.execute(
            "INSERT INTO mcp_servers (id, name, scope, transport, share_mode) VALUES ('m2','github','USER','STDIO','SHARED')",
            []
        )
        .is_err()
    );
}

#[test]
fn migration_003_leaves_a_populated_v2_database_intact() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    MIGRATIONS.to_version(&mut conn, 2).unwrap();
    conn.execute_batch(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','demo','/repo','main',0);
         INSERT INTO blobs (hash, size_bytes, stored_bytes, created_at) VALUES ('h1', 1, 1, 0);
         INSERT INTO context_objects (id, uri, project_id, kind, blob_hash, created_at) VALUES ('o1','ctx://file/a','p1','FILE','h1',0);
         INSERT INTO sessions (id, project_id, status, started_at) VALUES ('s1','p1','ACTIVE',0);
         INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at) VALUES ('t1','p1','T-1','WORK','uno','RUNNING',0,0);
         INSERT INTO providers (id, display_name, cli_name, adapter_mode, setup_state) VALUES ('anthropic','Claude','claude','CLI','READY');
         INSERT INTO models (id, provider_id, cli_model_id, display_name, discovered_at) VALUES ('claude/sonnet','anthropic','sonnet','Sonnet',0);
         INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, requested_model_id, failover_policy, context_mode, created_at, updated_at)
             VALUES ('a1','p1','s1','t1',1,'RUNNING','EXACT','claude/sonnet','ANY','BALANCED',0,0);
         INSERT INTO messages (id, agent_id, role, content_object_id, created_at) VALUES ('m1','a1','USER','o1',0);",
    )
    .unwrap();
    // Con una fila hija que apunta al objeto: la reconstrucción de `context_objects` tiene
    // que dejarla enlazada (con `RENAME` el COMMIT fallaba solo con datos reales).
    MIGRATIONS.to_latest(&mut conn).unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM context_objects", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
    // La misma dirección en otro proyecto ya no choca; en el mismo, sí.
    conn.execute_batch(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p2','otro','/otro','main',0);
         INSERT INTO context_objects (id, uri, project_id, kind, blob_hash, created_at) VALUES ('o2','ctx://file/a','p2','FILE','h1',0);",
    )
    .unwrap();
    assert!(conn
        .execute("INSERT INTO context_objects (id, uri, project_id, kind, blob_hash, created_at) VALUES ('o3','ctx://file/a','p1','FILE','h1',0)", [])
        .is_err());
    let fk: i64 = conn
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(fk, 0);
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

/// Una base en la 003 con tasks, agentes y runs pasa a la 004 sin perder nada, con la FK
/// `tasks.milestone_id`, `PHASE_DONE` aceptado y el DAG sin auto-dependencias.
#[test]
fn migration_004_keeps_data_and_adds_milestones_and_phase_done() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    MIGRATIONS.to_version(&mut conn, 3).unwrap();
    conn.execute_batch(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','demo','/repo','main',0);
         INSERT INTO sessions (id, project_id, status, started_at) VALUES ('s1','p1','ACTIVE',0);
         INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at)
             VALUES ('t1','p1','T-1','WORK','uno','RUNNING',0,0), ('t2','p1','T-2','WORK','dos','READY',0,0);
         INSERT INTO providers (id, display_name, cli_name, adapter_mode, setup_state) VALUES ('anthropic','Claude','claude','CLI','READY');
         INSERT INTO models (id, provider_id, cli_model_id, display_name, discovered_at) VALUES ('claude/sonnet','anthropic','sonnet','Sonnet',0);
         INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, requested_model_id, failover_policy, context_mode, created_at, updated_at)
             VALUES ('a1','p1','s1','t1',1,'RUNNING','EXACT','claude/sonnet','ANY','BALANCED',0,0);
         INSERT INTO agent_runs (id, agent_id, seq, provider_id, model_id, status, end_reason, started_at, ended_at)
             VALUES ('r1','a1',1,'anthropic','claude/sonnet','EXITED','COMPLETED',0,5);
         INSERT INTO agent_runs (id, agent_id, seq, provider_id, model_id, status, started_at)
             VALUES ('r2','a1',2,'anthropic','claude/sonnet','RUNNING',6);
         INSERT INTO tool_calls (id, agent_id, run_id, tool_name, op_class, status, requested_at)
             VALUES ('tc1','a1','r2','bash',3,'RUNNING',7);
         INSERT INTO messages (id, agent_id, run_id, role, content, created_at) VALUES ('m1','a1','r2','USER','hola',7);",
    )
    .unwrap();

    MIGRATIONS.to_latest(&mut conn).unwrap();

    let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
    assert_eq!(count("SELECT COUNT(*) FROM tasks"), 2);
    assert_eq!(count("SELECT COUNT(*) FROM agents"), 1);
    assert_eq!(count("SELECT COUNT(*) FROM agent_runs"), 2);
    assert_eq!(count("SELECT COUNT(*) FROM tool_calls"), 1);
    assert_eq!(count("SELECT COUNT(*) FROM messages"), 1);
    assert_eq!(count("SELECT COUNT(*) FROM pragma_foreign_key_check"), 0);

    // `PHASE_DONE` es un motivo válido; uno inventado, no. Un solo run abierto por agente.
    conn.execute_batch(
        "UPDATE agent_runs SET status='HANDED_OFF', end_reason='PHASE_DONE', ended_at=9 WHERE id='r2';",
    )
    .unwrap();
    assert!(
        conn.execute(
            "UPDATE agent_runs SET end_reason='PHASE_OVER' WHERE id='r2'",
            []
        )
        .is_err()
    );
    conn.execute(
        "INSERT INTO agent_runs (id, agent_id, seq, provider_id, model_id, status, started_at) VALUES ('r3','a1',3,'anthropic','claude/sonnet','RUNNING',10)",
        [],
    )
    .unwrap();
    assert!(
        conn.execute(
            "INSERT INTO agent_runs (id, agent_id, seq, provider_id, model_id, status, started_at) VALUES ('r4','a1',4,'anthropic','claude/sonnet','RUNNING',11)",
            []
        )
        .is_err()
    );

    // `tasks.milestone_id` ahora es FK; el DAG rechaza auto-dependencias y duplicados.
    assert!(
        conn.execute("UPDATE tasks SET milestone_id='nada' WHERE id='t2'", [])
            .is_err()
    );
    conn.execute_batch(
        "INSERT INTO milestones (id, project_id, name, status) VALUES ('m1','p1','Auth','OPEN');
         UPDATE tasks SET milestone_id='m1' WHERE id='t2';
         INSERT INTO task_dependencies (task_id, depends_on_task_id, created_at) VALUES ('t2','t1',0);",
    )
    .unwrap();
    assert!(
        conn.execute(
            "INSERT INTO task_dependencies (task_id, depends_on_task_id, created_at) VALUES ('t1','t1',0)",
            []
        )
        .is_err()
    );
    assert!(
        conn.execute(
            "INSERT INTO task_dependencies (task_id, depends_on_task_id, created_at) VALUES ('t2','t1',0)",
            []
        )
        .is_err()
    );
    // Las tablas nuevas apuntan a las tablas reconstruidas.
    assert!(
        conn.execute(
            "INSERT INTO merge_requests (id, agent_id, task_id, target_branch, status, created_at) VALUES ('mr1','a1','nadie','main','PREFLIGHT',0)",
            []
        )
        .is_err()
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

#[test]
fn a_retrieval_marks_the_handoff_and_a_normal_end_does_not_undo_it() {
    let conn = seeded();
    let (agent, run) = (
        symphony_core::AgentId::new().to_string(),
        symphony_core::RunId::new(),
    );
    insert_agent(&conn, &agent, "t1", 1, "RUNNING").unwrap();
    insert_run(&conn, &run.to_string(), &agent, 1, None).unwrap();
    conn.execute(
        "INSERT INTO handoffs (id, agent_id, to_run_id, mode, created_at) VALUES ('h1', ?1, ?2, 'BALANCED', 0)",
        params![agent, run.to_string()],
    )
    .unwrap();
    let outcome = |c: &Connection| {
        c.query_row("SELECT outcome FROM handoffs WHERE id = 'h1'", [], |r| {
            r.get::<_, Option<String>>(0)
        })
        .unwrap()
    };
    assert_eq!(outcome(&conn), None);
    crate::repo::mark_needed_retrieval(&conn, run).unwrap();
    assert_eq!(outcome(&conn).as_deref(), Some("NEEDED_RETRIEVAL"));
    // El cierre normal del run no lo pisa con CONTINUED…
    crate::repo::set_handoff_outcome(&conn, run, "CONTINUED").unwrap();
    assert_eq!(outcome(&conn).as_deref(), Some("NEEDED_RETRIEVAL"));
    // …pero un fallo sí manda, y una recuperación posterior no lo cambia.
    crate::repo::set_handoff_outcome(&conn, run, "FAILED_TO_CONTINUE").unwrap();
    crate::repo::mark_needed_retrieval(&conn, run).unwrap();
    assert_eq!(outcome(&conn).as_deref(), Some("FAILED_TO_CONTINUE"));
}

#[test]
fn shared_sync_keeps_the_user_switch_and_only_prunes_when_the_scan_is_trusted() {
    use crate::shared::{FoundMcp, list_mcp, set_mcp_enabled, sync_mcp};
    let conn = seeded();
    let server = |name: &str| FoundMcp {
        name: name.into(),
        scope: "PROJECT",
        transport: "STDIO",
        share_mode: "PER_CLIENT",
        command: Some("npx".into()),
        url: None,
        enabled_default: true,
    };
    sync_mcp(&conn, "p1", &[server("a"), server("b")], true).unwrap();
    set_mcp_enabled(&conn, "p1", "b", false).unwrap();
    // Releer no devuelve el interruptor a su valor por omisión.
    sync_mcp(&conn, "p1", &[server("a"), server("b")], true).unwrap();
    let rows = list_mcp(&conn, "p1").unwrap();
    assert_eq!(
        rows.iter()
            .map(|r| (r.name.as_str(), r.enabled))
            .collect::<Vec<_>>(),
        [("a", true), ("b", false)]
    );
    // Un escaneo vacío por un archivo roto no borra nada…
    sync_mcp(&conn, "p1", &[], false).unwrap();
    assert_eq!(list_mcp(&conn, "p1").unwrap().len(), 2);
    // …uno de fiar sí quita lo que ya no está.
    sync_mcp(&conn, "p1", &[server("a")], true).unwrap();
    assert_eq!(list_mcp(&conn, "p1").unwrap().len(), 1);
}
