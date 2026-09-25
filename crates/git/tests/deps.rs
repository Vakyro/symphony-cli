//! P04.S2: estrategia de dependencias por worktree.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::Command;
use std::time::Instant;

use symphony_git::Repo;
use symphony_git::deps::{self, DepsStrategy, PackageManager};

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

#[test]
fn detection_by_lockfile() {
    let dir = tempfile::tempdir().unwrap();
    assert!(deps::detect(dir.path()).is_none());
    for (file, pm) in [
        ("Cargo.lock", PackageManager::Cargo),
        ("bun.lockb", PackageManager::Bun),
        ("yarn.lock", PackageManager::Yarn),
        ("package-lock.json", PackageManager::Npm),
        ("pnpm-lock.yaml", PackageManager::Pnpm),
    ] {
        // Cada lockfile nuevo tiene más prioridad que los anteriores (orden de STACK §12.2).
        write(&dir.path().join(file), "x");
        assert_eq!(deps::detect(dir.path()).unwrap().0, pm, "{file}");
    }
}

#[test]
fn plans_per_manager() {
    let base = tempfile::tempdir().unwrap();
    let wt = tempfile::tempdir().unwrap();

    write(&wt.path().join("pnpm-lock.yaml"), "lock");
    let p = deps::plan(wt.path(), base.path()).unwrap();
    assert_eq!(p.strategy, DepsStrategy::PnpmStore);
    assert_eq!(
        p.command.unwrap()[..2],
        ["pnpm".to_string(), "install".to_string()]
    );
    assert_eq!(p.lock_hash.unwrap().len(), 64);

    let wt = tempfile::tempdir().unwrap();
    write(&wt.path().join("Cargo.lock"), "lock");
    assert_eq!(
        deps::plan(wt.path(), base.path()).unwrap().strategy,
        DepsStrategy::None
    );

    // npm: mismo lockfile y node_modules en el base → LINK; si no, INSTALL.
    let wt = tempfile::tempdir().unwrap();
    write(&wt.path().join("package-lock.json"), "{\"v\":1}");
    write(&base.path().join("package-lock.json"), "{\"v\":1}");
    assert_eq!(
        deps::plan(wt.path(), base.path()).unwrap().strategy,
        DepsStrategy::Install,
        "sin node_modules en el base"
    );
    std::fs::create_dir_all(base.path().join("node_modules")).unwrap();
    assert_eq!(
        deps::plan(wt.path(), base.path()).unwrap().strategy,
        DepsStrategy::Link
    );
    write(&wt.path().join("package-lock.json"), "{\"v\":2}");
    let p = deps::plan(wt.path(), base.path()).unwrap();
    assert_eq!(
        p.strategy,
        DepsStrategy::Install,
        "el agente tocó el lockfile"
    );
    assert_eq!(
        p.command.unwrap()[..2],
        ["npm".to_string(), "ci".to_string()]
    );
}

fn git(dir: &Path, args: &[&str]) {
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
}

#[test]
fn linked_node_modules_survive_worktree_removal() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base repo");
    std::fs::create_dir_all(&base).unwrap();
    git(&base, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "T"),
        ("user.email", "t@e.com"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
    ] {
        git(&base, &["config", k, v]);
    }
    write(&base.join(".gitignore"), "node_modules/\n");
    write(&base.join("package-lock.json"), "{}");
    git(&base, &["add", "."]);
    git(&base, &["commit", "-q", "-m", "init"]);
    write(
        &base.join("node_modules").join("left-pad").join("index.js"),
        "module.exports = 1;",
    );

    let repo = Repo::at(&base);
    let wt_path = dir.path().join("worktrees con espacios").join("agent-001");
    repo.worktree_add(&wt_path, "symphony/s/agent-001", "main")
        .unwrap();
    assert_eq!(
        deps::plan(&wt_path, &base).unwrap().strategy,
        DepsStrategy::Link
    );
    deps::link_node_modules(&wt_path, &base).unwrap();
    deps::link_node_modules(&wt_path, &base).unwrap(); // idempotente
    let via_link = std::fs::read_to_string(
        wt_path
            .join("node_modules")
            .join("left-pad")
            .join("index.js"),
    )
    .unwrap();
    assert_eq!(via_link, "module.exports = 1;");

    // Lo crítico: quitar el worktree (con --force) no puede borrar el node_modules del base.
    repo.worktree_remove(&wt_path, true).unwrap();
    assert!(!wt_path.exists());
    assert!(
        base.join("node_modules")
            .join("left-pad")
            .join("index.js")
            .is_file(),
        "se borró el node_modules del repo base"
    );
}

/// Mide el segundo worktree con pnpm (store compartido). Necesita pnpm y red:
/// `cargo nextest run -p symphony-git --run-ignored only pnpm`.
#[test]
#[ignore = "necesita pnpm y red"]
fn pnpm_second_worktree_reuses_store() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base");
    std::fs::create_dir_all(&base).unwrap();
    git(&base, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "T"),
        ("user.email", "t@e.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&base, &["config", k, v]);
    }
    write(
        &base.join("package.json"),
        r#"{"name":"p","version":"1.0.0","dependencies":{"is-number":"7.0.0","picocolors":"1.1.1"}}"#,
    );
    write(&base.join(".gitignore"), "node_modules/\n");
    let pnpm = if cfg!(windows) { "pnpm.cmd" } else { "pnpm" };
    assert!(
        Command::new(pnpm)
            .arg("install")
            .current_dir(&base)
            .status()
            .unwrap()
            .success()
    );
    git(&base, &["add", "."]);
    git(&base, &["commit", "-q", "-m", "init"]);

    let repo = Repo::at(&base);
    let mut times = Vec::new();
    for n in 1..=2 {
        let wt = dir.path().join(format!("wt{n}"));
        repo.worktree_add(&wt, &format!("symphony/s/agent-00{n}"), "main")
            .unwrap();
        let plan = deps::plan(&wt, &base).unwrap();
        assert_eq!(plan.strategy, DepsStrategy::PnpmStore);
        let t = Instant::now();
        assert!(
            deps::run_command(&wt, &plan.command.unwrap())
                .unwrap()
                .success()
        );
        times.push(t.elapsed());
        assert!(wt.join("node_modules").join("is-number").exists());
    }
    eprintln!("pnpm install por worktree (store ya caliente por el base): {times:?}");
}
