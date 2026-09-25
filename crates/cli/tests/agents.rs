//! Tests de integración para comandos de agente (P06.S7).
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::Command;

mod common;
use common::symphonyd;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn symphony(home: &Path, cwd: &Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_symphony"))
        .current_dir(cwd)
        .args(args)
        .env("SYMPHONY_HOME", home)
        .env("SYMPHONYD", symphonyd())
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn agent_lifecycle_cli_commands() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@test.local"]);
    git(&repo, &["config", "user.name", "Tester"]);
    std::fs::write(repo.join("README.md"), "# Test Project\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);

    // 1. Spawn agente con decide_later (por defecto si no hay modelo)
    let (ok, stdout, stderr) = symphony(&home, &repo, &["spawn", "Agregar autenticación"]);
    assert!(ok, "spawn falló: {stdout}\n{stderr}");
    assert!(stdout.contains("agente #1 creado"), "salida: {stdout}");
    assert!(stdout.contains("READY"), "salida: {stdout}");

    // 2. Listar agentes
    let (ok, stdout, stderr) = symphony(&home, &repo, &["agents"]);
    assert!(ok, "agents falló: {stdout}\n{stderr}");
    assert!(stdout.contains("#1"), "salida: {stdout}");
    assert!(stdout.contains("Agregar autenticación"), "salida: {stdout}");

    // 3. Inspeccionar agente
    let (ok, stdout, stderr) = symphony(&home, &repo, &["inspect", "1"]);
    assert!(ok, "inspect falló: {stdout}\n{stderr}");
    assert!(stdout.contains("AGENTE #1"), "salida: {stdout}");
    assert!(stdout.contains("Agregar autenticación"), "salida: {stdout}");
    assert!(stdout.contains("worktree:"), "salida: {stdout}");

    // 4. Logs del agente
    let (ok, stdout, stderr) = symphony(&home, &repo, &["logs", "1"]);
    assert!(ok, "logs falló: {stdout}\n{stderr}");

    // 5. Diff del agente
    let (ok, stdout, stderr) = symphony(&home, &repo, &["diff", "1"]);
    assert!(ok, "diff falló: {stdout}\n{stderr}");

    // 6. Pause en un agente sin executor corriendo debe fallar amigablemente
    let (ok, stdout, stderr) = symphony(&home, &repo, &["pause", "1"]);
    assert!(!ok, "pause debía fallar sin executor: {stdout}\n{stderr}");
    assert!(stderr.contains("no tiene un executor") || stdout.contains("no tiene un executor"));

    // 7. Stop agente 1 (READY -> COMPLETED)
    let (ok, stdout, stderr) = symphony(&home, &repo, &["stop", "1"]);
    assert!(ok, "stop falló: {stdout}\n{stderr}");
    assert!(stdout.contains("detenido"), "salida: {stdout}");

    // 8. Spawn agente 2 y terminarlo con kill (READY -> CANCELLED)
    let (ok, stdout, stderr) = symphony(&home, &repo, &["spawn", "Segunda tarea"]);
    assert!(ok, "spawn #2 falló: {stdout}\n{stderr}");
    assert!(stdout.contains("agente #2 creado"), "salida: {stdout}");

    let (ok, stdout, stderr) = symphony(&home, &repo, &["kill", "2"]);
    assert!(ok, "kill falló: {stdout}\n{stderr}");
    assert!(stdout.contains("terminado"), "salida: {stdout}");

    // 9. Listar activos (debe estar vacío) vs all (debe incluir 1 y 2)
    let (ok, stdout, stderr) = symphony(&home, &repo, &["agents"]);
    assert!(ok, "agents falló: {stdout}\n{stderr}");
    assert!(
        stdout.contains("No hay agentes activos."),
        "salida: {stdout}"
    );

    let (ok, stdout, stderr) = symphony(&home, &repo, &["agents", "--all"]);
    assert!(ok, "agents --all falló: {stdout}\n{stderr}");
    assert!(stdout.contains("#1"), "salida: {stdout}");
    assert!(stdout.contains("#2"), "salida: {stdout}");

    // 10. Cleanup daemon
    let (ok, stdout, stderr) = symphony(&home, &repo, &["daemon", "stop"]);
    assert!(ok, "daemon stop falló: {stdout}\n{stderr}");
}
