//! P04.S3: árbol de procesos → terminate_tree → 0 huérfanos, en los 3 OS.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::Duration;

use symphony_process::{ExitStatus, OutputLine, ProcessSpec, spawn};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// node → 3 hijos → 2 nietos cada uno (10 procesos), todos durmiendo.
const TREE_JS: &str = r#"
const { spawn } = require('child_process');
const tag = process.argv[2], depth = Number(process.argv[3] || 0);
if (depth < 2) for (let i = 0; i < (depth === 0 ? 3 : 2); i++)
  spawn(process.execPath, [__filename, tag, String(depth + 1)], { stdio: 'ignore' });
if (depth === 0) console.log('tree-ready');
setInterval(() => {}, 1000);
"#;

fn tree_script(dir: &tempfile::TempDir) -> PathBuf {
    let p = dir.path().join("tree.js");
    std::fs::write(&p, TREE_JS).unwrap();
    p
}

fn alive_with(marker: &str) -> usize {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    sys.processes()
        .values()
        .filter(|p| p.thread_kind().is_none() && p.status() != sysinfo::ProcessStatus::Zombie)
        .filter(|p| p.cmd().iter().any(|a| a.to_string_lossy().contains(marker)))
        .count()
}

async fn wait_count(marker: &str, want: usize) -> usize {
    for _ in 0..100 {
        let n = alive_with(marker);
        if n == want {
            return n;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    alive_with(marker)
}

#[tokio::test(flavor = "multi_thread")]
async fn terminate_tree_leaves_no_orphans() {
    let dir = tempfile::tempdir().unwrap();
    let marker = format!("tree-kill-{}", std::process::id());
    let mut p = spawn(ProcessSpec::new("node").arg(tree_script(&dir)).arg(&marker))
        .await
        .unwrap();
    assert_eq!(
        p.next_output().await,
        Some(OutputLine::Stdout("tree-ready".into()))
    );
    assert_eq!(wait_count(&marker, 10).await, 10);
    let stats = p.stats();
    assert!(stats.active_processes >= 10, "{stats:?}");
    p.terminate_tree().unwrap();
    assert!(!p.wait().await.success());
    assert_eq!(wait_count(&marker, 0).await, 0, "quedaron huérfanos");
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_the_handle_kills_the_tree() {
    let dir = tempfile::tempdir().unwrap();
    let marker = format!("tree-drop-{}", std::process::id());
    let mut p = spawn(ProcessSpec::new("node").arg(tree_script(&dir)).arg(&marker))
        .await
        .unwrap();
    p.next_output().await;
    assert_eq!(wait_count(&marker, 10).await, 10);
    drop(p);
    assert_eq!(
        wait_count(&marker, 0).await,
        0,
        "quedaron huérfanos al soltar el handle"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn streams_stdout_and_stderr_in_order_and_reports_exit_code() {
    let js = "for (let i = 0; i < 2000; i++) console.log('o' + i); console.error('e-final'); process.exit(3)";
    let mut p = spawn(ProcessSpec::new("node").args(["-e", js]))
        .await
        .unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    while let Some(line) = p.next_output().await {
        match line {
            OutputLine::Stdout(l) => out.push(l),
            OutputLine::Stderr(l) => err.push(l),
        }
    }
    assert_eq!(out.len(), 2000);
    assert!(out.iter().enumerate().all(|(i, l)| *l == format!("o{i}")));
    assert_eq!(err, vec!["e-final".to_string()]);
    assert_eq!(p.wait().await, ExitStatus::Exited(3));
}

#[tokio::test(flavor = "multi_thread")]
async fn stdin_can_be_written_while_running() {
    // Eco por línea, como un CLI que recibe mensajes por stdin (ADR-0005).
    let js = "process.stdin.setEncoding('utf8'); let b=''; process.stdin.on('data', d => { b += d; let i; while ((i = b.indexOf('\\n')) >= 0) { console.log('echo:' + b.slice(0, i)); b = b.slice(i + 1); } }); process.stdin.on('end', () => process.exit(0));";
    let mut p = spawn(ProcessSpec::new("node").args(["-e", js]).with_stdin())
        .await
        .unwrap();
    p.write_stdin(b"hola\n").await.unwrap();
    assert_eq!(
        p.next_output().await,
        Some(OutputLine::Stdout("echo:hola".into()))
    );
    p.write_stdin("segundo mensaje con ñ\n".as_bytes())
        .await
        .unwrap();
    assert_eq!(
        p.next_output().await,
        Some(OutputLine::Stdout("echo:segundo mensaje con ñ".into()))
    );
    p.close_stdin().await.unwrap();
    assert_eq!(p.wait().await, ExitStatus::Exited(0));
}

#[tokio::test(flavor = "multi_thread")]
async fn cwd_env_and_missing_program() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = spawn(
        ProcessSpec::new("node")
            .args(["-e", "console.log(process.cwd().endsWith('con espacios') + ' ' + process.env.SYMPHONY_AGENT_ID)"])
            .cwd({
                let d = dir.path().join("con espacios");
                std::fs::create_dir_all(&d).unwrap();
                d
            })
            .env("SYMPHONY_AGENT_ID", "agent-7"),
    )
    .await
    .unwrap();
    assert_eq!(
        p.next_output().await,
        Some(OutputLine::Stdout("true agent-7".into()))
    );
    assert!(p.wait().await.success());
    assert!(
        spawn(ProcessSpec::new("no-existe-este-programa-xyz"))
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn suspend_resume_and_guarantees() {
    let js = "setInterval(() => {}, 1000)";
    let mut p = spawn(ProcessSpec::new("node").args(["-e", js]))
        .await
        .unwrap();
    let g = p.guarantees().clone();
    assert!(g.limits_enforced, "sin límites pedidos siempre se cumplen");
    if cfg!(windows) {
        assert_eq!(g.mechanism, "JOB_OBJECT");
    }
    // Suspender y reanudar no fallan donde el SO lo soporta.
    if p.suspend().is_ok() {
        p.resume().unwrap();
    }
    assert!(
        p.wait_timeout(Duration::from_millis(200)).await.is_none(),
        "sigue corriendo"
    );
    p.terminate_tree().unwrap();
    assert!(p.wait_timeout(Duration::from_secs(10)).await.is_some());
}
