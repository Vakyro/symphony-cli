//! P07.5.S1 (L3, gasta cuota): un mensaje después del turno retoma la sesión del
//! CLI real y este recuerda lo dicho antes. Daemon real, `symphony spawn` y `symphony send`.
//!
//!   SYMPHONY_LIVE=1 cargo nextest run -p symphony-cli --no-capture live_resume
//!
//! Solo con permiso de Leo. Modelos baratos y prompts mínimos.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::symphonyd;

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

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

fn symphony(home: &Path, cwd: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_symphony"))
        .current_dir(cwd)
        .args(args)
        .env("SYMPHONY_HOME", home)
        .env("SYMPHONYD", symphonyd())
        .output()
        .unwrap();
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn agent_state(home: &Path) -> String {
    let conn = symphony_store::open_reader(&home.join("symphony.db")).unwrap();
    conn.query_row("SELECT state FROM agents WHERE number = 1", [], |r| {
        r.get(0)
    })
    .unwrap_or_default()
}

fn wait_for_state(home: &Path, want: &str, what: &str) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(240) {
        let state = agent_state(home);
        if state == want {
            return;
        }
        assert!(
            !matches!(state.as_str(), "FAILED" | "CANCELLED" | "BLOCKED"),
            "{what}: el agente quedó {state}"
        );
        std::thread::sleep(Duration::from_secs(1));
    }
    panic!(
        "{what}: el agente no llegó a {want} (estado: {})",
        agent_state(home)
    );
}

fn run_live(model: &str) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t.local"]);
    git(&repo, &["config", "user.name", "T"]);
    std::fs::write(repo.join("README.md"), "demo\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);

    let (ok, out) = symphony(
        &home,
        &repo,
        &[
            "spawn",
            "Juego: el codigo del juego es 7431. Confirma repitiendolo en una frase corta. No uses herramientas.",
            "--model",
            model,
        ],
    );
    assert!(ok, "spawn falló: {out}");
    wait_for_state(&home, "COMPLETED", "turno 1");

    let (ok, out) = symphony(
        &home,
        &repo,
        &[
            "send",
            "1",
            "Dentro del juego, cual era el codigo? Responde solo el numero. No uses herramientas.",
        ],
    );
    assert!(ok, "send tras el turno falló: {out}");
    wait_for_state(&home, "COMPLETED", "turno 2");

    let conn = symphony_store::open_reader(&home.join("symphony.db")).unwrap();
    let runs: i64 = conn
        .query_row("SELECT COUNT(*) FROM agent_runs", [], |r| r.get(0))
        .unwrap();
    let sessions: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT cli_session_id) FROM agent_runs",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let answers: Vec<String> = conn
        .prepare("SELECT content FROM messages WHERE role = 'ASSISTANT' ORDER BY rowid")
        .unwrap()
        .query_map([], |r| r.get::<_, Option<String>>(0))
        .unwrap()
        .filter_map(|m| m.unwrap())
        .collect();
    eprintln!("[{model}] runs={runs} sesiones={sessions} respuestas={answers:?}");
    symphony(&home, &repo, &["daemon", "stop"]);

    assert_eq!(runs, 2, "debe haber un run por turno");
    assert_eq!(
        sessions, 1,
        "los dos runs deben compartir la sesión del CLI"
    );
    assert!(
        answers.last().is_some_and(|a| a.contains("7431")),
        "el CLI no recordó el dato del turno anterior: {answers:?}"
    );
}

#[test]
fn live_claude_remembers_after_a_message_past_the_turn() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    run_live("claude/haiku");
}

#[test]
fn live_codex_remembers_after_a_message_past_the_turn() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    run_live("openai/gpt-5.6-luna");
}

#[test]
fn live_kimi_remembers_after_a_message_past_the_turn() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    run_live("moonshot/default");
}

#[test]
fn live_antigravity_remembers_after_a_message_past_the_turn() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    run_live("google/gemini-3.8-flash-low");
}

#[test]
fn live_copilot_remembers_after_a_message_past_the_turn() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    run_live("github/auto");
}
