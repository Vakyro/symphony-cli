//! P11.S5 (L3, gasta cuota): handoff cruzado con CLIs reales. El proveedor A empieza una tarea
//! larga; `symphony switch` lo corta a media tarea (termina su árbol de procesos, como un forced
//! kill) y el proveedor B la termina con solo el handoff. Daemon real, `spawn` y `switch`.
//!
//!   SYMPHONY_LIVE=1 cargo nextest run -p symphony-cli --no-capture live_handoff
//!
//! Solo con permiso de Leo. Modelos baratos y una tarea mínima de 6 archivos encadenados.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::symphonyd;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const TASK: &str = "Crea 6 archivos de texto en el directorio actual, en orden. n1.txt contiene el numero 1. \
Para k de 2 a 6: lee n(k-1).txt y crea nk.txt con ese numero mas 1. Hazlo paso a paso, \
sin saltarte la lectura de cada archivo. Al terminar responde solo: hecho";

/// Tiempo máximo que se deja trabajar a A esperando su primer archivo antes del corte.
const CUT_AFTER: Duration = Duration::from_secs(45);

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

/// Para el daemon aunque el test falle: un `symphonyd.exe` huérfano rompe el build siguiente.
struct DaemonGuard {
    home: PathBuf,
    repo: PathBuf,
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        symphony(&self.home, &self.repo, &["daemon", "stop"]);
    }
}

fn query(home: &Path, sql: &str) -> Option<String> {
    let conn = symphony_store::open_reader(&home.join("symphony.db")).ok()?;
    conn.query_row(sql, [], |r| r.get(0)).ok()
}

fn query_n(home: &Path, sql: &str) -> Option<i64> {
    let conn = symphony_store::open_reader(&home.join("symphony.db")).ok()?;
    conn.query_row(sql, [], |r| r.get(0)).ok()
}

fn agent_state(home: &Path) -> String {
    query(home, "SELECT state FROM agents WHERE number = 1").unwrap_or_default()
}

fn count_files(wt: &Path) -> usize {
    (1..=6)
        .filter(|k| wt.join(format!("n{k}.txt")).is_file())
        .count()
}

fn run_pair(from: &str, to: &str, to_provider: &str) {
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
    let _guard = DaemonGuard {
        home: home.clone(),
        repo: repo.clone(),
    };
    let start = Instant::now();

    let (ok, out) = symphony(&home, &repo, &["spawn", TASK, "--model", from]);
    assert!(ok, "spawn falló: {out}");
    let wt: PathBuf = query(&home, "SELECT path FROM worktrees LIMIT 1")
        .expect("el agente debe tener worktree")
        .into();

    // A trabaja hasta que crea su primer archivo o pasa CUT_AFTER; entonces se lo corta.
    let cut_start = Instant::now();
    while cut_start.elapsed() < CUT_AFTER && count_files(&wt) == 0 {
        let state = agent_state(&home);
        assert!(
            !matches!(state.as_str(), "COMPLETED" | "FAILED" | "CANCELLED"),
            "[{from} -> {to}] A quedó {state} antes del corte"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    let state = agent_state(&home);
    assert!(
        matches!(state.as_str(), "RUNNING" | "READY"),
        "[{from} -> {to}] A terminó antes del corte (estado {state}): la tarea fue demasiado corta"
    );
    let files_at_cut = count_files(&wt);
    let cut_at = start.elapsed().as_secs();

    let (ok, out) = symphony(&home, &repo, &["switch", "1", "--model", to]);
    assert!(ok, "[{from} -> {to}] switch falló: {out}");

    let wait = Instant::now();
    loop {
        let state = agent_state(&home);
        if state == "COMPLETED" {
            break;
        }
        assert!(
            !matches!(state.as_str(), "FAILED" | "CANCELLED" | "BLOCKED"),
            "[{from} -> {to}] B dejó al agente {state}"
        );
        assert!(
            wait.elapsed() < Duration::from_secs(300),
            "[{from} -> {to}] B no terminó (estado {state})"
        );
        std::thread::sleep(Duration::from_secs(1));
    }

    let runs = query_n(&home, "SELECT COUNT(*) FROM agent_runs").unwrap();
    let first_end =
        query(&home, "SELECT end_reason FROM agent_runs WHERE seq = 1").unwrap_or_default();
    let second_provider =
        query(&home, "SELECT provider_id FROM agent_runs WHERE seq = 2").unwrap_or_default();
    let prompt = query(
        &home,
        "SELECT content FROM messages WHERE role = 'USER' ORDER BY created_at DESC LIMIT 1",
    )
    .unwrap_or_default();
    let contents: Vec<String> = (1..=6)
        .map(|k| {
            std::fs::read_to_string(wt.join(format!("n{k}.txt")))
                .unwrap_or_default()
                .trim()
                .to_string()
        })
        .collect();
    eprintln!(
        "[{from} -> {to}] corte a los {cut_at}s con {files_at_cut}/6 archivos; B tardó {}s; total {}s; \
         runs={runs} fin_A={first_end} proveedor_B={second_provider} archivos={contents:?}",
        wait.elapsed().as_secs(),
        start.elapsed().as_secs()
    );

    assert_eq!(runs, 2, "debe haber un run por proveedor");
    assert_eq!(first_end, "USER_SWITCH", "A debe terminar por el corte");
    assert_eq!(second_provider, to_provider);
    assert!(
        prompt.contains("n1.txt") && prompt.contains("sin saltarte"),
        "el primer mensaje de B debe ser el handoff con el objetivo: {prompt}"
    );
    assert_eq!(
        contents,
        ["1", "2", "3", "4", "5", "6"],
        "la tarea debe quedar completa y correcta"
    );
}

#[test]
fn live_handoff_claude_to_kimi() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    run_pair("claude/haiku", "moonshot/default", "moonshot");
}

#[test]
fn live_handoff_codex_to_copilot() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    run_pair("openai/gpt-5.6-luna", "github/auto", "github");
}

#[test]
fn live_handoff_antigravity_to_claude() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    run_pair("google/gemini-3.8-flash-low", "claude/haiku", "anthropic");
}
