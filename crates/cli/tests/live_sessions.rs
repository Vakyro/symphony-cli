//! Criterio de salida de P05 (L3, gasta cuota): una sesión real corta de cada CLI,
//! con hooks de Symphony, deja eventos canónicos en `events`.
//!
//!   SYMPHONY_LIVE=1 cargo nextest run -p symphony-cli --no-capture live_
//!
//! Solo con permiso de Leo. Modelos baratos y un comando trivial.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::symphonyd;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use symphony_adapter_claude::ClaudeAdapter;
use symphony_adapter_codex::CodexAdapter;
use symphony_adapter_common::{HookCommand, ProviderAdapter, SpawnRequest};

fn live() -> bool {
    std::env::var("SYMPHONY_LIVE").as_deref() == Ok("1")
}

fn git(dir: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

fn run_live(adapter: &dyn ProviderAdapter, model: &str) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "demo\n").unwrap();

    // Agente sembrado (el runtime que crea agentes llega en P06).
    let db = home.join("symphony.db");
    symphony_store::open(&db)
        .unwrap()
        .execute_batch(
            "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','live','/live','main',0);
             INSERT INTO sessions (id, project_id, status, started_at) VALUES ('s1','p1','ACTIVE',0);
             INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at) VALUES ('t1','p1','T-1','WORK','live','RUNNING',0,0);
             INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, failover_policy, context_mode, created_at, updated_at)
                 VALUES ('a1','p1','s1','t1',1,'RUNNING','DECIDE_LATER','ANY','BALANCED',0,0);",
        )
        .unwrap();
    let symphony = env!("CARGO_BIN_EXE_symphony");
    let start = Command::new(symphony)
        .args(["daemon", "start"])
        .env("SYMPHONY_HOME", &home)
        .env("SYMPHONYD", symphonyd())
        .status()
        .unwrap();
    assert!(start.success());

    let req = SpawnRequest {
        worktree: repo.clone(),
        model: model.into(),
        prompt:
            "Use the shell tool to run exactly: git status . Then reply with the single word: done"
                .into(),
        session_id: None,
        hook: Some(HookCommand {
            program: symphony.into(),
            args: vec!["hook".into(), "emit".into()],
        }),
        env: vec![
            ("SYMPHONY_AGENT_ID".into(), "a1".into()),
            ("SYMPHONY_PROJECT_ID".into(), "p1".into()),
            ("SYMPHONY_HOME".into(), home.display().to_string()),
        ],
    };
    let spec = adapter.spawn_spec(&req).unwrap();
    let mut child = Command::new(&spec.program)
        .args(&spec.args)
        .current_dir(&repo)
        .envs(spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(&adapter.encode_prompt(&req.prompt))
        .unwrap();
    drop(stdin); // un turno y listo
    let out = child.wait_with_output().unwrap();
    let stream: Vec<&str> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .flat_map(|l| adapter.parse_stream_line(l))
        .map(|e| e.type_name())
        .collect::<Vec<_>>();
    eprintln!("[{}] stream → {stream:?}", adapter.provider_id());

    Command::new(symphony)
        .args(["daemon", "stop"])
        .env("SYMPHONY_HOME", &home)
        .status()
        .unwrap();
    let conn = symphony_store::open_reader(&db).unwrap();
    let types: Vec<String> = conn
        .prepare("SELECT type FROM events WHERE agent_id = 'a1' ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    eprintln!("[{}] events → {types:?}", adapter.provider_id());
    for want in [
        "AgentStarted",
        "TurnStarted",
        "CommandRequested",
        "CommandFinished",
        "TurnFinished",
    ] {
        assert!(
            types.iter().any(|t| t == want),
            "[{}] falta {want} en events: {types:?}",
            adapter.provider_id()
        );
    }
    assert!(
        stream.contains(&"AgentStarted"),
        "el stream no trajo la sesión: {stream:?}"
    );
}

#[test]
fn live_claude_session_produces_canonical_events() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    run_live(
        &ClaudeAdapter {
            permission_mode: "acceptEdits".into(),
            ..ClaudeAdapter::default()
        },
        "haiku",
    );
}

#[test]
fn live_codex_session_produces_canonical_events() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    run_live(&CodexAdapter::default(), "gpt-5.6-luna");
}
