//! P13.S3: un proceso muerto dentro de una transacción no publica escrituras parciales.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use symphony_store::Writer;

const CHILD_DB: &str = "SYMPHONY_WRITER_CRASH_TEST_DB";

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn writer_crash_child() {
    let Some(db) = std::env::var_os(CHILD_DB).map(PathBuf::from) else {
        return;
    };
    let ready = db.with_extension("ready");
    let writer = Writer::start(&db).unwrap();
    writer
        .handle()
        .write(Box::new(move |tx| {
            tx.execute_batch(
                "PRAGMA cache_size = 1;
                 UPDATE agent_runs SET ended_at = 2, status = 'HANDED_OFF' WHERE id = 'r1';
                 INSERT INTO agent_runs (id, agent_id, seq, provider_id, model_id, status, started_at)
                     VALUES ('r2', 'a1', 2, 'fake', 'fake/fast', 'RUNNING', 2);",
            )?;
            // Fuerza páginas fuera del cache antes del kill, sin llegar al commit.
            tx.execute(
                "INSERT INTO checkpoints (id, agent_id, run_id, seq, objective, created_at)
                 VALUES ('c2', 'a1', 'r2', 2, ?1, 2)",
                ["pendiente".repeat(256 * 1024)],
            )?;
            std::fs::write(ready, "transaction in progress").unwrap();
            loop {
                std::thread::park();
            }
        }))
        .await
        .unwrap();
}

#[tokio::test]
async fn killing_writer_mid_transaction_preserves_checkpoint_and_run() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("symphony.db");
    let conn = symphony_store::open(&db).unwrap();
    conn.execute_batch(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at)
             VALUES ('p1', 'demo', '/repo', 'main', 1);
         INSERT INTO sessions (id, project_id, status, started_at)
             VALUES ('s1', 'p1', 'ACTIVE', 1);
         INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at)
             VALUES ('t1', 'p1', 'T1', 'WORK', 'objetivo confirmado', 'RUNNING', 1, 1);
         INSERT INTO providers (id, display_name, cli_name, adapter_mode, setup_state)
             VALUES ('fake', 'fake', 'fake-agent', 'CLI', 'READY');
         INSERT INTO models (id, provider_id, cli_model_id, display_name, discovered_at)
             VALUES ('fake/fast', 'fake', 'fast', 'fast', 1);
         INSERT INTO agents (id, project_id, session_id, task_id, number, state,
             execution_mode, failover_policy, context_mode, created_at, updated_at)
             VALUES ('a1', 'p1', 's1', 't1', 1, 'RUNNING', 'DECIDE_LATER', 'ANY', 'BALANCED', 1, 1);
         INSERT INTO agent_runs (id, agent_id, seq, provider_id, model_id, status, started_at)
             VALUES ('r1', 'a1', 1, 'fake', 'fake/fast', 'RUNNING', 1);
         INSERT INTO checkpoints (id, agent_id, run_id, seq, objective, created_at)
             VALUES ('c1', 'a1', 'r1', 1, 'objetivo confirmado', 1);",
    )
    .unwrap();
    drop(conn);

    let mut child = KillOnDrop(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "writer_crash_child", "--nocapture"])
            .env(CHILD_DB, &db)
            .env_remove("SYMPHONY_LIVE")
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while !db.with_extension("ready").exists() {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "el hijo terminó antes de escribir"
        );
        assert!(
            Instant::now() < deadline,
            "el hijo no llegó a la transacción"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    child.0.kill().unwrap();
    assert!(!child.0.wait().unwrap().success());

    let writer = Writer::start(&db).unwrap();
    let conn = writer.reader().unwrap();
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    let checkpoints: (i64, String) = conn
        .query_row(
            "SELECT COUNT(*), MIN(objective) FROM checkpoints",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(checkpoints, (1, "objetivo confirmado".into()));
    let run: (i64, String, Option<i64>) = conn
        .query_row(
            "SELECT COUNT(*), MIN(id), MAX(ended_at) FROM agent_runs",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(run, (1, "r1".into(), None));
    let duplicate_runs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM (SELECT agent_id FROM agent_runs WHERE ended_at IS NULL
         GROUP BY agent_id HAVING COUNT(*) > 1)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(duplicate_runs, 0);
    let foreign_key_errors: i64 = conn
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(foreign_key_errors, 0);
    writer
        .handle()
        .write(Box::new(|tx| {
            tx.execute(
                "UPDATE checkpoints SET next_step = 'continuar' WHERE id = 'c1'",
                [],
            )?;
            Ok(())
        }))
        .await
        .unwrap();
    let next_step: String = conn
        .query_row(
            "SELECT next_step FROM checkpoints WHERE id = 'c1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(next_step, "continuar");
}
