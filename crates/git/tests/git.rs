//! P04.S1: wrapper de Git contra repos temporales, con espacios y acentos en las rutas.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use symphony_git::{FileStatus, MergePreflight, Repo, agent_branch};

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

/// Repo con un commit inicial en `<tmp>/repo con espacios/`.
fn repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo con espacios");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "Test"),
        ("user.email", "test@example.com"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
    ] {
        git(&root, &["config", k, v]);
    }
    std::fs::write(root.join("README.md"), "hola\n").unwrap();
    std::fs::write(root.join("app.js"), "export const a = 1;\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "init"]);
    (dir, root)
}

#[test]
fn version_and_discover() {
    assert!(
        symphony_git::version()
            .unwrap()
            .chars()
            .next()
            .unwrap()
            .is_ascii_digit()
    );
    let (_d, root) = repo();
    std::fs::create_dir_all(root.join("sub dir")).unwrap();
    let r = Repo::discover(&root.join("sub dir")).unwrap();
    assert_eq!(
        std::fs::canonicalize(r.root()).unwrap(),
        std::fs::canonicalize(&root).unwrap()
    );
    assert_eq!(r.current_branch().unwrap().as_deref(), Some("main"));
    assert_eq!(r.head_commit().unwrap().len(), 40);
    assert!(
        Repo::discover(std::env::temp_dir().as_path()).is_err()
            || std::env::temp_dir().join(".git").exists()
    );
}

#[test]
fn branch_naming() {
    assert_eq!(agent_branch("s01", 3), "symphony/s01/agent-003");
    assert_eq!(agent_branch("s01", 1234), "symphony/s01/agent-1234");
}

#[test]
fn worktree_lifecycle_with_spaces() {
    let (dir, root) = repo();
    let r = Repo::at(&root);
    let wt_path = dir.path().join("worktrees con espacios").join("agent-001");
    let branch = agent_branch("sesión-1", 1);
    let wt = r.worktree_add(&wt_path, &branch, "main").unwrap();
    assert!(wt_path.join("README.md").is_file());
    assert_eq!(
        wt.current_branch().unwrap().as_deref(),
        Some(branch.as_str())
    );

    let list = r.worktree_list().unwrap();
    assert_eq!(list.len(), 2);
    let agent = list
        .iter()
        .find(|w| w.branch.as_deref() == Some(branch.as_str()))
        .unwrap();
    assert_eq!(
        std::fs::canonicalize(&agent.path).unwrap(),
        std::fs::canonicalize(&wt_path).unwrap()
    );
    assert_eq!(
        agent.head.as_deref(),
        Some(r.head_commit().unwrap().as_str())
    );

    // Con cambios sin commitear, remove sin force falla; con force sale.
    std::fs::write(wt_path.join("nuevo.txt"), "x").unwrap();
    assert!(r.worktree_remove(&wt_path, false).is_err());
    r.worktree_remove(&wt_path, true).unwrap();
    assert!(!wt_path.exists());
    assert_eq!(r.worktree_list().unwrap().len(), 1);
    r.delete_branch(&branch, true).unwrap();

    // Un worktree borrado a mano queda "prunable" hasta el prune.
    let wt2 = dir.path().join("wt2");
    r.worktree_add(&wt2, "symphony/s/agent-002", "main")
        .unwrap();
    std::fs::remove_dir_all(&wt2).unwrap();
    assert!(r.worktree_list().unwrap().iter().any(|w| w.prunable));
    r.worktree_prune().unwrap();
    assert_eq!(r.worktree_list().unwrap().len(), 1);
}

#[test]
fn status_porcelain_v2() {
    let (_d, root) = repo();
    let r = Repo::at(&root);
    assert!(r.status().unwrap().is_empty());

    std::fs::write(root.join("README.md"), "hola\nmundo\n").unwrap(); // modificado, sin stage
    std::fs::write(root.join("archivo ñandú.txt"), "nuevo").unwrap(); // sin trackear, con espacio y ñ
    git(&root, &["mv", "app.js", "src main.js"]); // rename en el index
    let st = r.status().unwrap();

    assert!(
        st.contains(&FileStatus::Changed {
            xy: ".M".into(),
            path: "README.md".into(),
            renamed_from: None
        }),
        "{st:?}"
    );
    assert!(
        st.contains(&FileStatus::Untracked {
            path: "archivo ñandú.txt".into()
        }),
        "{st:?}"
    );
    assert!(
        st.iter().any(
            |e| matches!(e, FileStatus::Changed { xy, path, renamed_from: Some(from) }
            if xy.starts_with('R') && path == "src main.js" && from == "app.js")
        ),
        "{st:?}"
    );
}

#[test]
fn diff_and_numstat() {
    let (_d, root) = repo();
    let r = Repo::at(&root);
    std::fs::write(root.join("README.md"), "hola\nmundo\ncon acento: é\n").unwrap();
    std::fs::write(root.join("bin.dat"), [0u8, 159, 146, 150, 0, 1]).unwrap();
    git(&root, &["add", "bin.dat"]);
    git(&root, &["mv", "app.js", "renombrado con espacio.js"]);

    let diff = r.diff("HEAD").unwrap();
    assert!(diff.contains("+mundo"));
    let stats = r.diff_numstat("HEAD").unwrap();
    let readme = stats.iter().find(|s| s.path == "README.md").unwrap();
    assert_eq!((readme.added, readme.deleted), (Some(2), Some(0)));
    let bin = stats.iter().find(|s| s.path == "bin.dat").unwrap();
    assert_eq!((bin.added, bin.deleted), (None, None), "binario: -\\t-");
    let ren = stats.iter().find(|s| s.renamed_from.is_some()).unwrap();
    assert_eq!(
        (ren.path.as_str(), ren.renamed_from.as_deref()),
        ("renombrado con espacio.js", Some("app.js"))
    );
}

#[test]
fn merge_preflight_clean_and_conflict() {
    let (dir, root) = repo();
    let r = Repo::at(&root);
    // Agente 1 toca un archivo distinto que main: limpio.
    let a1 = r
        .worktree_add(&dir.path().join("a1"), "symphony/s/agent-001", "main")
        .unwrap();
    std::fs::write(a1.root().join("feature.js"), "x\n").unwrap();
    git(a1.root(), &["add", "."]);
    git(a1.root(), &["commit", "-q", "-m", "feature"]);
    // Agente 2 y main cambian la misma línea: conflicto.
    let a2 = r
        .worktree_add(&dir.path().join("a2"), "symphony/s/agent-002", "main")
        .unwrap();
    std::fs::write(a2.root().join("README.md"), "hola desde el agente\n").unwrap();
    git(a2.root(), &["commit", "-qam", "agente"]);
    std::fs::write(root.join("README.md"), "hola desde main\n").unwrap();
    git(&root, &["commit", "-qam", "main"]);

    assert!(
        matches!(r.merge_preflight("main", "symphony/s/agent-001").unwrap(), MergePreflight::Clean { tree } if tree.len() == 40)
    );
    assert_eq!(
        r.merge_preflight("main", "symphony/s/agent-002").unwrap(),
        MergePreflight::Conflicts {
            paths: vec!["README.md".into()]
        }
    );
    // El preflight no tocó nada.
    assert!(r.status().unwrap().is_empty());
    assert!(r.merge_preflight("main", "no-existe").is_err());
}
