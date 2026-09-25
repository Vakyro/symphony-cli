//! E2E P02.S7: el CLI arranca el daemon, se mata el daemon sin aviso y el
//! siguiente comando lo detecta y lo vuelve a arrancar.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::Command;

mod common;
use common::symphonyd;

fn symphony(home: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_symphony"))
        .args(args)
        .env("SYMPHONY_HOME", home)
        .env("SYMPHONYD", symphonyd())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "symphony {args:?} falló:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

fn pid_of(status_output: &str) -> u32 {
    status_output
        .lines()
        .find_map(|l| l.trim().strip_prefix("pid:"))
        .and_then(|p| p.trim().parse().ok())
        .unwrap_or_else(|| panic!("sin pid en:\n{status_output}"))
}

/// Mata sin cleanup, como un crash.
fn kill_hard(pid: u32) {
    let status = if cfg!(windows) {
        Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .output()
            .unwrap()
            .status
    } else {
        Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output()
            .unwrap()
            .status
    };
    assert!(status.success(), "no se pudo matar el pid {pid}");
}

#[test]
fn cli_restarts_a_daemon_that_died_without_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");

    // En frío: `status` arranca el daemon.
    let first = pid_of(&symphony(&home, &["status"]));
    assert!(symphony(&home, &["daemon", "status"]).contains(&format!("pid:      {first}")));

    kill_hard(first);
    // Esperar a que el SO lo termine de bajar (libera lock y socket/pipe).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !symphony(&home, &["daemon", "status"]).contains("detenido") {
        assert!(
            std::time::Instant::now() < deadline,
            "el daemon muerto sigue respondiendo"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Queda el lock y (en Unix) el socket del muerto: el CLI arranca uno nuevo igual.
    let second = pid_of(&symphony(&home, &["status"]));
    assert_ne!(first, second);
    assert!(symphony(&home, &["daemon", "status"]).contains("corriendo"));

    assert!(symphony(&home, &["daemon", "stop"]).contains("detenido"));
}
