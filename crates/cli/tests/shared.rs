//! P09.S9: `symphony skills` y `symphony mcp list|enable|disable` contra un daemon real.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::Command;

mod common;
use common::{path_with_fake_claude, stop_daemon, symphonyd};

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
        .env("PATH", path_with_fake_claude(home))
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn skills_and_mcp_servers_are_listed_and_toggled() {
    let dir = tempfile::tempdir().unwrap();
    let (home, repo) = (dir.path().join("home"), dir.path().join("repo"));
    std::fs::create_dir_all(repo.join(".symphony/skills/revisar")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@test.local"]);
    git(&repo, &["config", "user.name", "Tester"]);
    std::fs::write(
        repo.join(".symphony/skills/revisar/SKILL.md"),
        "---\nname: revisar\ndescription: Revisa un diff\n---\nPasos.\n",
    )
    .unwrap();
    std::fs::write(
        repo.join(".symphony/mcp.toml"),
        "[servers.fs]\ncommand = \"npx\"\nargs = [\"-y\", \"server-fs\"]\n\n[servers.roto]\ntransport = \"http\"\n",
    )
    .unwrap();
    std::fs::write(repo.join("README.md"), "# t\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    // El proyecto se registra con el primer agente.
    let (ok, out, err) = symphony(&home, &repo, &["spawn", "Tarea"]);
    assert!(ok, "{out}{err}");

    let (ok, out, err) = symphony(&home, &repo, &["skills"]);
    assert!(ok, "{out}{err}");
    assert!(
        out.contains("on  revisar") && out.contains("PROJECT") && out.contains("Revisa un diff"),
        "{out}"
    );

    let (ok, out, err) = symphony(&home, &repo, &["skills", "disable", "revisar"]);
    assert!(ok && out.contains("apagado"), "{out}{err}");
    let (_, out, _) = symphony(&home, &repo, &["skills", "list"]);
    assert!(out.contains("off revisar"), "{out}");
    // El interruptor sobrevive a releer los archivos.
    let (_, out, _) = symphony(&home, &repo, &["skills"]);
    assert!(out.contains("off revisar"), "{out}");
    let (ok, _, err) = symphony(&home, &repo, &["skills", "enable", "no-existe"]);
    assert!(!ok && err.contains("no hay una skill"), "{err}");

    let (ok, out, err) = symphony(&home, &repo, &["mcp", "list"]);
    assert!(ok, "{out}{err}");
    assert!(out.contains("on  fs") && out.contains("npx"), "{out}");
    assert!(
        !out.contains("roto"),
        "el servidor sin `url` se descarta: {out}"
    );
    assert!(err.contains("aviso") && err.contains("roto"), "{err}");

    let (ok, out, err) = symphony(&home, &repo, &["mcp", "disable", "fs"]);
    assert!(ok && out.contains("apagado"), "{out}{err}");
    let (_, out, _) = symphony(&home, &repo, &["mcp", "list"]);
    assert!(out.contains("off fs"), "{out}");

    assert!(stop_daemon(&home).status.success());
}
