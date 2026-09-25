//! P05.S3: `symphony hook emit` lleva el hook al bus del daemon.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod common;
use common::symphonyd;

fn symphony(home: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_symphony"));
    c.env("SYMPHONY_HOME", home).env("SYMPHONYD", symphonyd());
    c
}

fn hook(home: &Path, agent: &str, payload: &str) -> (bool, String, Duration) {
    let t = Instant::now();
    let mut child = symphony(home)
        .args(["hook", "emit"])
        .env("SYMPHONY_AGENT_ID", agent)
        .env("SYMPHONY_PROJECT_ID", "p1")
        .env("SYMPHONY_PROVIDER", "fake")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
        t.elapsed(),
    )
}

fn event_types(db: &Path, agent: &str) -> Vec<String> {
    let conn = symphony_store::open_reader(db).unwrap();
    let mut stmt = conn
        .prepare("SELECT type FROM events WHERE agent_id = ?1 ORDER BY id")
        .unwrap();
    stmt.query_map([agent], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn hook_reaches_the_bus_and_never_breaks_the_cli() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let db = home.join("symphony.db");
    symphony_store::open(&db)
        .unwrap()
        .execute_batch(
            "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','p','/p','main',0);
             INSERT INTO sessions (id, project_id, status, started_at) VALUES ('s1','p1','ACTIVE',0);
             INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at) VALUES ('t1','p1','T-1','WORK','t','RUNNING',0,0);
             INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, failover_policy, context_mode, created_at, updated_at)
                 VALUES ('a1','p1','s1','t1',1,'RUNNING','DECIDE_LATER','ANY','BALANCED',0,0);",
        )
        .unwrap();

    // Con el daemon caído: exit 0 y sin salida (el hook nunca rompe al CLI ni lo arranca).
    let pre = r#"{"hook_event_name":"PreToolUse","session_id":"s","tool_name":"Bash","tool_input":{"command":"npm test"}}"#;
    let (ok, out, _) = hook(&home, "a1", pre);
    assert!(ok && out.is_empty(), "{out}");
    assert!(
        !symphony_protocol::transport::daemon_lock_held(&home),
        "un hook no debe arrancar el daemon"
    );

    let started = symphony(&home).args(["daemon", "start"]).output().unwrap();
    assert!(
        started.status.success(),
        "daemon start: {}{}",
        String::from_utf8_lossy(&started.stdout),
        String::from_utf8_lossy(&started.stderr)
    );
    // Nota: el daemon marcó INTERRUPTED la sesión sembrada; el agente sigue existiendo.

    let mut times = Vec::new();
    for _ in 0..20 {
        let (ok, out, t) = hook(&home, "a1", pre);
        assert!(ok && out.is_empty(), "{out}");
        times.push(t);
    }
    let post = r#"{"hook_event_name":"PostToolUse","tool_name":"Write","tool_input":{"file_path":"a.js"}}"#;
    assert!(hook(&home, "a1", post).0);
    // Agente desconocido: el daemon lo rechaza, el hook sigue saliendo 0 y en silencio.
    let (ok, out, _) = hook(&home, "agente-fantasma", pre);
    assert!(ok && out.is_empty(), "{out}");

    assert!(
        symphony(&home)
            .args(["daemon", "stop"])
            .status()
            .unwrap()
            .success()
    );
    let types = event_types(&db, "a1");
    assert_eq!(
        types.iter().filter(|t| *t == "CommandRequested").count(),
        20
    );
    assert_eq!(&types[20..], ["ToolFinished", "FileModified"]);
    assert!(event_types(&db, "agente-fantasma").is_empty());

    times.sort();
    eprintln!(
        "latencia de `symphony hook emit` (proceso completo): mediana {:?}, p90 {:?}, máx {:?}",
        times[10], times[18], times[19]
    );
}
