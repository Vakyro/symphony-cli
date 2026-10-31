//! P18.S3 · verifica live de compactación con Codex (máximo dos turnos).
//!
//!   SYMPHONY_LIVE=1 cargo nextest run -p symphony-cli --test live_compact --no-capture
//!
//! Solo con autorización de Leo. Usa un repo temporal, modelo exacto y failover desactivado.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{DaemonGuard, symphonyd};
use serde_json::json;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};
use symphony_protocol::{Message, Outcome, Request, transport};

const MODEL: &str = "openai/gpt-6-luna";

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
            .success(),
        "git {args:?} falló en {}",
        dir.display()
    );
}

fn symphony(home: &Path, cwd: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_symphony"))
        .current_dir(cwd)
        .args(args)
        .env("SYMPHONY_HOME", home)
        .env("SYMPHONYD", symphonyd())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "symphony {args:?}: {text}");
    text
}

fn state(home: &Path) -> String {
    symphony_store::open_reader(&home.join("symphony.db"))
        .unwrap()
        .query_row("SELECT state FROM agents WHERE number = 1", [], |r| {
            r.get(0)
        })
        .unwrap_or_default()
}

fn wait_for(home: &Path, want: &str) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(240) {
        let current = state(home);
        if current == want {
            return;
        }
        assert!(
            !matches!(
                current.as_str(),
                "FAILED" | "CANCELLED" | "BLOCKED" | "WAITING_PROVIDER"
            ),
            "el agente quedó {current} esperando {want}"
        );
        std::thread::sleep(Duration::from_secs(1));
    }
    panic!(
        "el agente no llegó a {want}; estado actual: {}",
        state(home)
    );
}

async fn next_phase(home: &Path, repo: &Path) {
    let mut conn = transport::connect(home).await.unwrap();
    conn.send(&Message::Request(Request::new(
        "p18-compact",
        "agent.next_phase",
        json!({ "agent": "1", "project_root": repo.display().to_string() }),
    )))
    .await
    .unwrap();
    let Some(Message::Response(response)) = conn.recv().await.unwrap() else {
        panic!("respuesta IPC inesperada al confirmar la fase")
    };
    assert_eq!(response.id, "p18-compact");
    assert!(
        matches!(response.outcome, Outcome::Ok(_)),
        "{:?}",
        response.outcome
    );
}

#[tokio::test]
async fn codex_keeps_the_three_task_handoff_in_a_new_session() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "live@example.invalid"]);
    git(&repo, &["config", "user.name", "Symphony live test"]);
    std::fs::write(repo.join("README.md"), "compact test\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "initial"]);
    let _daemon = DaemonGuard::new(&home);

    symphony(&home, &repo, &["init"]);
    let task = "Crea un plan de dos fases, con tres tareas en cada fase. Antes de la fase 1 anota en el plan: decisión=violeta-731; restricción=no usar red; archivos tocados=compact-a.txt, compact-b.txt, compact-c.txt. En fase 1 crea esos tres archivos con una línea breve cada uno, marca cada tarea [x] y termina la sesión. No empieces la fase 2. Usa las herramientas de archivos del harness, no shell.";
    symphony(
        &home,
        &repo,
        &["spawn", task, "--model", MODEL, "--failover", "none"],
    );
    wait_for(&home, "READY");

    let db = home.join("symphony.db");
    let conn = symphony_store::open_reader(&db).unwrap();
    let first = conn
        .query_row(
            "SELECT id, status, end_reason FROM agent_runs WHERE seq = 1",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(first.1, "HANDED_OFF");
    assert_eq!(first.2, "PHASE_DONE");
    let reason: String = conn
        .query_row(
            "SELECT state_reason FROM agents WHERE number = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(reason.contains("3 tareas"), "{reason}");
    drop(conn);

    next_phase(&home, &repo).await;
    wait_for(&home, "COMPLETED");
    let conn = symphony_store::open_reader(&db).unwrap();
    let (second, first_session, second_session): (String, String, String) = conn
        .query_row(
            "SELECT r2.id, r1.cli_session_id, r2.cli_session_id
             FROM agent_runs r1 JOIN agent_runs r2 ON r2.seq = 2
             WHERE r1.seq = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_ne!(
        first_session, second_session,
        "Codex no admite /compact: se esperaba una sesión nueva"
    );
    let prompt: String = conn
        .query_row(
            "SELECT content FROM messages WHERE run_id = ?1 AND role = 'USER' ORDER BY rowid LIMIT 1",
            [&second],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        prompt.contains("violeta-731"),
        "se perdió la decisión: {prompt}"
    );
    assert!(
        prompt.contains("no usar red"),
        "se perdió la restricción: {prompt}"
    );
    assert!(
        prompt.contains("compact-a.txt"),
        "se perdió el archivo tocado: {prompt}"
    );
    let (tokens_in, tokens_out, sources): (i64, i64, String) = conn
        .query_row(
            "SELECT COALESCE(SUM(tokens_in), 0), COALESCE(SUM(tokens_out), 0),
                    COALESCE(group_concat(DISTINCT source), 'NO_USAGE_REPORTED')
             FROM usage_records WHERE run_id IN (?1, ?2)",
            [&first.0, &second],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    eprintln!(
        "P18.S3 live {MODEL}: runs=2, sesiones distintas=true, tokens_in={tokens_in}, tokens_out={tokens_out}, fuente={sources}"
    );
    assert_eq!(state(&home), "COMPLETED");
}
