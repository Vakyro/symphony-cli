//! P09.S6: `symphony context inspect|stats|raw` contra un daemon real.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::Command;

mod common;
use common::symphonyd;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
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
fn context_commands_on_a_fresh_project() {
    let dir = tempfile::tempdir().unwrap();
    let (home, repo) = (dir.path().join("home"), dir.path().join("repo"));
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@test.local"]);
    git(&repo, &["config", "user.name", "Tester"]);
    std::fs::write(repo.join("README.md"), "# Test\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);

    let (ok, out, err) = symphony(&home, &repo, &["spawn", "Tarea"]);
    assert!(ok, "{out}{err}");

    let (ok, out, err) = symphony(&home, &repo, &["context", "inspect", "1"]);
    assert!(ok, "{out}{err}");
    assert!(out.contains("no tiene handoffs"), "{out}");

    let (ok, out, err) = symphony(&home, &repo, &["context", "stats"]);
    assert!(ok, "{out}{err}");
    assert!(out.contains("objetos de contexto"), "{out}");
    assert!(out.contains("recuperaciones: 0 (0 sin resultado)"), "{out}");

    // Lo que no existe y lo que intenta escapar fallan con un mensaje, sin tocar el disco.
    let (ok, _, err) = symphony(&home, &repo, &["context", "raw", "ctx://file/nada.ts"]);
    assert!(!ok && err.contains("no existe"), "{err}");
    let (ok, _, err) = symphony(
        &home,
        &repo,
        &["context", "raw", "ctx://file/../../etc/passwd"],
    );
    assert!(!ok && !err.is_empty(), "{err}");

    symphony(&home, &repo, &["daemon", "stop"]);
}
