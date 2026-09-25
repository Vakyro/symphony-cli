//! P04.S6: worktree → fake-agent adentro → leer su stream → terminate_tree → borrar worktree.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;
use symphony_git::{Repo, agent_branch};
use symphony_process::{OutputLine, ProcessSpec, spawn};

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

#[tokio::test(flavor = "multi_thread")]
async fn agent_in_worktree_is_streamed_killed_and_cleaned_up() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("proyecto con espacios");
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
    std::fs::write(base.join("README.md"), "base\n").unwrap();
    git(&base, &["add", "."]);
    git(&base, &["commit", "-q", "-m", "init"]);

    // 1. Worktree del agente.
    let repo = Repo::at(&base);
    let branch = agent_branch("s1", 1);
    let wt_path = dir.path().join("worktrees").join("agent-001");
    let wt = repo.worktree_add(&wt_path, &branch, "main").unwrap();

    // 2. fake-agent adentro: edita, avisa y se cuelga (como un CLI que sigue trabajando).
    let script = dir.path().join("script.toml");
    std::fs::write(
        &script,
        "[[step]]\nkind = \"edit\"\npath = \"src/nuevo.js\"\ncontent = \"x\\n\"\n[[step]]\nkind = \"say\"\ntext = \"sigo trabajando\"\n[[step]]\nkind = \"hang\"\n",
    )
    .unwrap();
    let mut agent = spawn(
        ProcessSpec::new(env!("CARGO_BIN_EXE_fake-agent"))
            .arg("run")
            .arg("--script")
            .arg(&script)
            .cwd(wt.root())
            .env("SYMPHONY_AGENT_ID", "agent-001"),
    )
    .await
    .unwrap();

    // 3. Leer el stream hasta el mensaje del asistente.
    let mut seen = Vec::new();
    while let Some(line) = tokio::time::timeout(Duration::from_secs(20), agent.next_output())
        .await
        .unwrap()
    {
        let OutputLine::Stdout(l) = line else {
            continue;
        };
        let ev: Value = serde_json::from_str(&l).unwrap();
        let kind = ev["type"].as_str().unwrap_or("").to_string();
        seen.push(kind.clone());
        if kind == "assistant" {
            assert_eq!(ev["text"], "sigo trabajando");
            break;
        }
    }
    assert_eq!(seen, ["system", "tool_use", "tool_result", "assistant"]);
    assert!(wt.root().join("src").join("nuevo.js").is_file());
    let status = wt.status().unwrap();
    assert!(
        status.iter().any(|s| s.path() == "src/nuevo.js"),
        "{status:?}"
    );

    // 4. Matar el árbol: el agente colgado termina.
    assert!(
        agent
            .wait_timeout(Duration::from_millis(300))
            .await
            .is_none(),
        "debería seguir colgado"
    );
    agent.terminate_tree().unwrap();
    assert!(agent.wait_timeout(Duration::from_secs(10)).await.is_some());

    // 5. Borrar el worktree (con cambios) y su rama.
    repo.worktree_remove(&wt_path, true).unwrap();
    repo.delete_branch(&branch, true).unwrap();
    assert!(!wt_path.exists());
    assert_eq!(repo.worktree_list().unwrap().len(), 1);
    assert_eq!(
        std::fs::read_to_string(base.join("README.md")).unwrap(),
        "base\n"
    );
}
