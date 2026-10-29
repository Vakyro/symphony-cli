//! P11.5.S6: `symphony chat merge` contra un daemon real.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::Command;

mod common;
use common::{DaemonGuard, symphonyd};

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
fn chat_merge_explains_what_is_missing_and_never_merges_without_yes() {
    let dir = tempfile::tempdir().unwrap();
    let (home, repo) = (dir.path().join("home"), dir.path().join("repo"));
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t.local"]);
    git(&repo, &["config", "user.name", "T"]);
    std::fs::write(repo.join("README.md"), "# demo\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    let _daemon = DaemonGuard::new(&home);

    // Sin chat todavía: el mensaje dice qué hacer, no hay merge.
    let (ok, _out, err) = symphony(&home, &repo, &["chat", "merge"]);
    assert!(!ok);
    assert!(
        err.contains("no está registrado") || err.contains("no tiene chat"),
        "{err}"
    );
    let (ok, _out, err) = symphony(&home, &repo, &["chat", "merge", "--yes"]);
    assert!(!ok, "{err}");

    // `--skip-review` y `--review` no tienen sentido sin su compañera.
    let (ok, _, err) = symphony(&home, &repo, &["chat", "merge", "--skip-review"]);
    assert!(!ok);
    assert!(err.contains("--yes"), "{err}");
    let (ok, _, err) = symphony(&home, &repo, &["chat", "merge", "--review", "--yes"]);
    assert!(!ok);
    assert!(err.contains("cannot be used"), "{err}");
    let _ = _daemon.stop();
}
