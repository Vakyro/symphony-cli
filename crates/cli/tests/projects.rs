//! P15.5.S3 (ADR-0018): un daemon y un estado por proyecto. Desde un proyecto no se ve nada de otro.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::Command;

mod common;
use common::{fake_agent, symphonyd};

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

fn repo(parent: &Path, name: &str) -> std::path::PathBuf {
    let dir = parent.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "test@test.local"]);
    git(&dir, &["config", "user.name", "Tester"]);
    std::fs::write(dir.join("README.md"), format!("# {name}\n")).unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    dir
}

/// Sin `SYMPHONY_HOME`: el home sale del directorio, como lo usa una persona.
fn symphony(user_home: &Path, cwd: &Path, args: &[&str]) -> (bool, String) {
    let bin = user_home.parent().unwrap().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let claude = bin.join(format!("claude{}", std::env::consts::EXE_SUFFIX));
    if !claude.exists() {
        std::fs::copy(fake_agent(), &claude).unwrap();
    }
    let rest = std::env::var_os("PATH").unwrap_or_default();
    let path =
        std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&rest))).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_symphony"))
        .current_dir(cwd)
        .args(args)
        .env_remove("SYMPHONY_HOME")
        .env("SYMPHONY_USER_HOME", user_home)
        .env("SYMPHONYD", symphonyd())
        .env("PATH", path)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

#[test]
fn each_project_keeps_its_own_state_and_cannot_see_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("user");
    let (a, b) = (repo(dir.path(), "alfa"), repo(dir.path(), "beta"));

    let (ok, out) = symphony(&user, &a, &["spawn", "Tarea exclusiva de alfa"]);
    assert!(ok, "spawn en alfa: {out}");
    let (ok, out) = symphony(&user, &a, &["agents"]);
    assert!(ok && out.contains("Tarea exclusiva de alfa"), "alfa: {out}");

    // Desde beta no existe nada de alfa; beta arranca su propio daemon.
    let (ok, out) = symphony(&user, &b, &["agents"]);
    assert!(ok, "agents en beta: {out}");
    assert!(
        !out.contains("Tarea exclusiva de alfa"),
        "beta ve a alfa: {out}"
    );
    let (ok, out) = symphony(&user, &b, &["spawn", "Tarea de beta"]);
    assert!(ok, "spawn en beta: {out}");
    let (_, out) = symphony(&user, &a, &["agents"]);
    assert!(!out.contains("Tarea de beta"), "alfa ve a beta: {out}");

    // Cada estado vive en su proyecto: base, worktrees y un .gitignore que lo mantiene fuera de git.
    for (root, other) in [(&a, "beta"), (&b, "alfa")] {
        let state = root.join(".symphony");
        assert!(state.join("symphony.db").is_file(), "{state:?}");
        assert!(state.join("worktrees").is_dir(), "{state:?}");
        let ignore = std::fs::read_to_string(state.join(".gitignore")).unwrap();
        assert!(ignore.contains("!project.toml"), "{ignore}");
        assert!(!other.is_empty());
    }
    // Lo del usuario no guarda estado de proyectos.
    assert!(!user.join("symphony.db").exists());
    assert!(!user.join("worktrees").exists());

    // Dentro de un worktree de alfa se sigue hablando con el daemon de alfa.
    let wt = std::fs::read_dir(a.join(".symphony/worktrees"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let wt = std::fs::read_dir(wt)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let (ok, out) = symphony(&user, &wt, &["agents"]);
    assert!(
        ok && out.contains("Tarea exclusiva de alfa"),
        "worktree: {out}"
    );
    assert!(
        !wt.join(".symphony").exists(),
        "no debe crear estado dentro del worktree"
    );

    for root in [&a, &b] {
        let (ok, out) = symphony(&user, root, &["daemon", "stop"]);
        assert!(ok, "daemon stop: {out}");
    }
}

#[test]
fn migrate_split_moves_a_global_database_into_each_project() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("user");
    let (a, b) = (repo(dir.path(), "alfa"), repo(dir.path(), "beta"));
    {
        let conn = symphony_store::open(&user.join("symphony.db")).unwrap();
        for (id, name, root, at) in [("pa", "alfa", &a, 1), ("pb", "beta", &b, 2)] {
            conn.execute(
                "INSERT INTO projects (id, name, root_path, default_branch, created_at)
                 VALUES (?1, ?2, ?3, 'main', ?4)",
                rusqlite::params![id, name, root.to_string_lossy(), at],
            )
            .unwrap();
        }
        conn.execute_batch(
            "INSERT INTO sessions (id, project_id, status, started_at) VALUES
                 ('sa','pa','ACTIVE',0), ('sb','pb','ACTIVE',0);
             INSERT INTO providers (id, display_name, cli_name, adapter_mode, setup_state)
                 VALUES ('anthropic','Claude','claude','CLI','READY');
             INSERT INTO models (id, provider_id, cli_model_id, display_name, discovered_at)
                 VALUES ('claude/haiku','anthropic','haiku','Haiku',0);",
        )
        .unwrap();
    }
    std::fs::create_dir_all(user.join("objects/ab")).unwrap();
    std::fs::write(user.join("objects/ab/cdef"), b"contenido").unwrap();

    let (ok, out) = symphony(&user, dir.path(), &["migrate", "split"]);
    assert!(ok, "migrate split: {out}");
    assert!(out.contains("base creada"), "{out}");

    for (root, own, other) in [(&a, "pa", "pb"), (&b, "pb", "pa")] {
        let db = rusqlite::Connection::open(root.join(".symphony/symphony.db")).unwrap();
        let sessions = |id: &str| -> i64 {
            db.query_row(
                "SELECT COUNT(*) FROM sessions WHERE project_id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!((sessions(own), sessions(other)), (1, 0), "{root:?}");
        assert_eq!(
            std::fs::read(root.join(".symphony/objects/ab/cdef")).unwrap(),
            b"contenido"
        );
    }
    // Idempotente, y no borra la base global.
    let (ok, out) = symphony(&user, dir.path(), &["migrate", "split"]);
    assert!(ok && out.contains("ya tenía su base"), "{out}");
    assert!(user.join("symphony.db").is_file());
    // El daemon del proyecto abre la base ya repartida.
    let (ok, out) = symphony(&user, &a, &["agents"]);
    assert!(ok, "agents tras migrar: {out}");
    let (ok, out) = symphony(&user, &a, &["daemon", "stop"]);
    assert!(ok, "daemon stop: {out}");
}
