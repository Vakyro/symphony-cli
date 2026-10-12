//! P08.S3: un `PreToolUse` pesado pasa por el scheduler del daemon. Corre si hay slot, espera
//! su turno (agente en `WAITING_RESOURCE`, `tool_call` en `QUEUED` con quién la bloquea) y,
//! si el turno no llega, se rechaza con la razón en la salida del hook.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod common;
use common::symphonyd;

/// Id (ULID) del proyecto sembrado: el daemon parsea los ids de verdad.
fn project() -> &'static str {
    static P: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    P.get_or_init(|| symphony_core::ProjectId::new().to_string())
}

fn symphony(home: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_symphony"));
    c.env("SYMPHONY_HOME", home).env("SYMPHONYD", symphonyd());
    c
}

/// Corre `symphony hook emit` como lo haría un CLI y devuelve (stdout, duración).
fn pre_tool(home: &Path, agent: &str, run: &str, use_id: &str, cmd: &str) -> (String, Duration) {
    let payload = format!(
        r#"{{"hook_event_name":"PreToolUse","session_id":"s","tool_use_id":"{use_id}","tool_name":"Bash","tool_input":{{"command":"{cmd}"}}}}"#
    );
    emit(home, agent, run, &payload)
}

fn post_tool(home: &Path, agent: &str, run: &str, use_id: &str) {
    let payload = format!(
        r#"{{"hook_event_name":"PostToolUse","session_id":"s","tool_use_id":"{use_id}","tool_name":"Bash","tool_input":{{"command":"x"}},"tool_response":{{"exit_code":0}}}}"#
    );
    emit(home, agent, run, &payload);
}

fn emit(home: &Path, agent: &str, run: &str, payload: &str) -> (String, Duration) {
    let t = Instant::now();
    let mut child = symphony(home)
        .args(["hook", "emit"])
        .env("SYMPHONY_AGENT_ID", agent)
        .env("SYMPHONY_PROJECT_ID", project())
        .env("SYMPHONY_RUN_ID", run)
        .env("SYMPHONY_PROVIDER", "anthropic")
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
    assert!(out.status.success(), "un hook nunca falla");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        t.elapsed(),
    )
}

type Call = (String, Option<String>, Option<String>, String, i64);

/// (status, queue_reason, blocked_by, enforcement, op_class) de la tool call de `command`.
fn call_of(db: &Path, command: &str) -> Call {
    let conn = symphony_store::open_reader(db).unwrap();
    conn.query_row(
        "SELECT status, queue_reason, blocked_by_tool_call_id, enforcement, op_class
         FROM tool_calls WHERE command = ?1 ORDER BY requested_at DESC LIMIT 1",
        [command],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )
    .unwrap()
}

fn agent_state(db: &Path, agent: &str) -> String {
    symphony_store::open_reader(db)
        .unwrap()
        .query_row("SELECT state FROM agents WHERE id = ?1", [agent], |r| {
            r.get(0)
        })
        .unwrap()
}

fn id_of(db: &Path, command: &str) -> String {
    symphony_store::open_reader(db)
        .unwrap()
        .query_row(
            "SELECT id FROM tool_calls WHERE command = ?1 ORDER BY requested_at DESC LIMIT 1",
            [command],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn heavy_tool_waits_for_its_turn_and_is_denied_with_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    // Un slot pesado y un tope de retención corto para no esperar 100 s en el test.
    std::fs::write(
        home.join("config.toml"),
        "[performance]\nprofile = \"BALANCED\"\nmax_heavy = 1\nhold_max_secs = 2\n",
    )
    .unwrap();
    let db = home.join("symphony.db");
    // Ids reales (ULID): el daemon los parsea como tales.
    let (a1, a2) = (
        symphony_core::AgentId::new().to_string(),
        symphony_core::AgentId::new().to_string(),
    );
    let (r1, r2) = (
        symphony_core::RunId::new().to_string(),
        symphony_core::RunId::new().to_string(),
    );
    let p = project();
    let s = symphony_core::SessionId::new().to_string();
    let (t1, t2) = (
        symphony_core::TaskId::new().to_string(),
        symphony_core::TaskId::new().to_string(),
    );
    symphony_store::open(&db)
        .unwrap()
        .execute_batch(&format!(
            "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('{p}','p','/p','main',0);
             INSERT INTO sessions (id, project_id, status, started_at) VALUES ('{s}','{p}','ACTIVE',0);
             INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at)
                 VALUES ('{t1}','{p}','T-1','WORK','t','RUNNING',0,0), ('{t2}','{p}','T-2','WORK','t','RUNNING',0,0);
             INSERT INTO providers (id, display_name, cli_name, adapter_mode, setup_state) VALUES ('anthropic','Claude','claude','CLI','READY');
             INSERT INTO models (id, provider_id, cli_model_id, display_name, discovered_at) VALUES ('claude/sonnet','anthropic','sonnet','Sonnet',0);
             INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, requested_model_id, failover_policy, context_mode, created_at, updated_at)
                 VALUES ('{a1}','{p}','{s}','{t1}',1,'RUNNING','EXACT','claude/sonnet','ANY','BALANCED',0,0),
                        ('{a2}','{p}','{s}','{t2}',2,'RUNNING','EXACT','claude/sonnet','ANY','BALANCED',0,0);
             INSERT INTO agent_runs (id, agent_id, seq, provider_id, model_id, status, started_at)
                 VALUES ('{r1}','{a1}',1,'anthropic','claude/sonnet','RUNNING',0), ('{r2}','{a2}',1,'anthropic','claude/sonnet','RUNNING',0);",
        ))
        .unwrap();

    let started = symphony(&home).args(["daemon", "start"]).output().unwrap();
    assert!(started.status.success());

    // 1. Con el slot libre corre al instante y queda registrada con `HOOK_HOLD`.
    let (out, _) = pre_tool(&home, &a1, &r1, "u1", "pnpm build");
    assert!(out.is_empty(), "no se imprime allow: {out}");
    let c = call_of(&db, "pnpm build");
    assert_eq!(
        (c.0.as_str(), c.3.as_str(), c.4),
        ("RUNNING", "HOOK_HOLD", 4)
    );

    // 2. Con el slot ocupado, el segundo espera; al pasar el tope se rechaza con la razón.
    let (out, took) = pre_tool(&home, &a2, &r2, "u2", "pnpm test");
    assert!(took >= Duration::from_secs(2), "esperó {took:?}");
    let v: serde_json::Value = serde_json::from_str(out.trim()).expect(&out);
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
    let why = v["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(
        why.contains("Agent #1") && why.contains("pnpm build"),
        "{why}"
    );
    let c = call_of(&db, "pnpm test");
    assert_eq!(c.0, "DENIED");
    assert!(c.1.as_deref().is_some_and(|r| r.contains("Agent #1")));
    assert_eq!(agent_state(&db, &a2), "RUNNING", "vuelve a RUNNING");

    // 3. Mientras espera: `QUEUED`, con quién la bloquea y el agente en `WAITING_RESOURCE`.
    // Un hilo hace el hook de a2; se libera el slot de a1 antes del tope.
    let waiter = {
        let home = home.clone();
        let (a2c, r2c) = (a2.clone(), r2.clone());
        std::thread::spawn(move || pre_tool(&home, &a2c, &r2c, "u3", "pnpm test --filter api"))
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    // La tool call se inserta antes de pasar el agente a `WAITING_RESOURCE`: se espera a ambas.
    let mut trail: Vec<(Option<String>, String)> = Vec::new();
    loop {
        let state = agent_state(&db, &a2);
        let seen = symphony_store::open_reader(&db)
            .unwrap()
            .query_row(
                "SELECT status FROM tool_calls WHERE command = 'pnpm test --filter api'",
                [],
                |r| r.get::<_, String>(0),
            )
            .ok();
        if seen.as_deref() == Some("QUEUED") && state == "WAITING_RESOURCE" {
            break;
        }
        if trail.last() != Some(&(seen.clone(), state.clone())) {
            trail.push((seen.clone(), state.clone()));
        }
        if Instant::now() >= deadline {
            dump_logs(&home);
        }
        assert!(Instant::now() < deadline, "nunca quedó en cola: {trail:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
    let c = call_of(&db, "pnpm test --filter api");
    assert_eq!(c.2.as_deref(), Some(id_of(&db, "pnpm build").as_str()));
    assert!(c.1.unwrap().contains("slot pesado"));

    post_tool(&home, &a1, &r1, "u1");
    let (out, _) = waiter.join().unwrap();
    assert!(out.is_empty(), "al tener turno no imprime nada: {out}");
    let c = call_of(&db, "pnpm test --filter api");
    assert_eq!((c.0.as_str(), c.1), ("RUNNING", None));
    assert_eq!(agent_state(&db, &a2), "RUNNING");
    assert_eq!(call_of(&db, "pnpm build").0, "DONE");

    assert!(
        symphony(&home)
            .args(["daemon", "stop"])
            .status()
            .unwrap()
            .success()
    );
}

fn dump_logs(home: &Path) {
    if let Ok(dir) = std::fs::read_dir(home.join("logs")) {
        for f in dir.flatten() {
            eprintln!("--- {:?}", f.path());
            eprintln!("{}", std::fs::read_to_string(f.path()).unwrap_or_default());
        }
    }
}
