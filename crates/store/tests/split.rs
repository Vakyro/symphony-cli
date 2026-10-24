//! Partir la base global en una por proyecto (P15.5.S3, ADR-0018).
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use symphony_store::split::{list_projects, split_project};

fn project_rows(project: &str, n: u8) -> String {
    format!(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('{project}','{project}','/r/{project}','main',{n});
         INSERT INTO sessions (id, project_id, status, started_at) VALUES ('s-{project}','{project}','ACTIVE',0);
         INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at) VALUES ('t-{project}','{project}','T-1','WORK','x','RUNNING',0,0);
         INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, failover_policy, context_mode, created_at, updated_at)
             VALUES ('a-{project}','{project}','s-{project}','t-{project}',1,'RUNNING','DECIDE_LATER','ANY','BALANCED',0,0);"
    )
}

#[test]
fn a_split_keeps_one_project_and_everything_global() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join("symphony.db");
    {
        let conn = symphony_store::open(&legacy).unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO providers (id, display_name, cli_name, adapter_mode, setup_state) VALUES ('anthropic','Claude','claude','CLI','READY');
             INSERT INTO models (id, provider_id, cli_model_id, display_name, discovered_at) VALUES ('claude/haiku','anthropic','haiku','Haiku',0);
             {} {}",
            project_rows("alfa", 1),
            project_rows("beta", 2)
        ))
        .unwrap();
    }
    let projects = list_projects(&legacy).unwrap();
    assert_eq!(
        projects,
        [
            ("alfa".into(), "/r/alfa".into()),
            ("beta".into(), "/r/beta".into())
        ]
    );

    let target = dir.path().join("alfa.db");
    let removed = split_project(&legacy, &target, "alfa").unwrap();
    assert!(removed >= 4, "{removed}");

    let conn = rusqlite::Connection::open(&target).unwrap();
    let count = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!(count("SELECT COUNT(*) FROM projects WHERE id = 'alfa'"), 1);
    assert_eq!(count("SELECT COUNT(*) FROM projects WHERE id <> 'alfa'"), 0);
    for table in ["sessions", "tasks", "agents"] {
        assert_eq!(
            count(&format!("SELECT COUNT(*) FROM {table}")),
            1,
            "{table}"
        );
    }
    // Lo global se queda, y el esquema sigue siendo el de la última migración.
    assert_eq!(count("SELECT COUNT(*) FROM models"), 1);
    let version = |c: &rusqlite::Connection| {
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap()
    };
    assert_eq!(
        version(&conn),
        version(&rusqlite::Connection::open(&legacy).unwrap())
    );
    // La original no se toca.
    let original = rusqlite::Connection::open(&legacy).unwrap();
    assert_eq!(
        original
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    // La base partida se abre con el almacén normal (migraciones al día, sin FKs rotas).
    drop(conn);
    symphony_store::open(&target).unwrap();
}
