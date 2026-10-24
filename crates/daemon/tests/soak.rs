//! P13.S3: prueba de larga duración con fake-agents y reinicios del runtime sobre la misma base.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use symphony_adapter_common::ProviderAdapter;
use symphony_core::{ContextMode, FailoverPolicy};
use symphony_daemon::bus::EventBus;
use symphony_daemon::providers;
use symphony_daemon::runtime::{CreateAgent, Execution, Runtime};
use symphony_object_store::ObjectStore;
use symphony_store::Writer;
use symphony_testkit::FakeAdapter;
use sysinfo::{Pid, ProcessesToUpdate, System};

/// Lento al arrancar: dar tiempo a detener agentes a media tarea.
const SCRIPT: &str = r#"
startup_delay_ms = 300

[[step]]
kind = "say"
text = "Trabajando"

[[step]]
kind = "edit"
path = "soak.txt"
content = "ok\n"
"#;

fn fake_agent() -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let ok = Command::new(env!("CARGO"))
                .args([
                    "build",
                    "-q",
                    "-p",
                    "symphony-testkit",
                    "--bin",
                    "fake-agent",
                ])
                .status()
                .unwrap()
                .success();
            assert!(ok, "no se pudo construir fake-agent");
            symphony_testkit::pinned_bin(
                &PathBuf::from(env!("CARGO_BIN_EXE_symphonyd"))
                    .with_file_name(format!("fake-agent{}", std::env::consts::EXE_SUFFIX)),
            )
        })
        .clone()
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
}

struct Soak {
    _dir: tempfile::TempDir,
    home: PathBuf,
    repo: PathBuf,
    db: PathBuf,
    adapters: Vec<Arc<dyn ProviderAdapter>>,
    writer: Option<Writer>,
    runtime: Runtime,
}

impl Soak {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@t"]);
        git(&repo, &["config", "user.name", "t"]);
        std::fs::write(repo.join("README.md"), "demo\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "init"]);

        let script = dir.path().join("fake.toml");
        std::fs::write(&script, SCRIPT).unwrap();
        let adapter = FakeAdapter::new(fake_agent(), &script).with_provider("fake");
        let db = home.join("symphony.db");
        let writer = Writer::start(&db).unwrap();
        let detected: Vec<Box<dyn ProviderAdapter>> = vec![Box::new(adapter)];
        providers::save(&writer.handle(), providers::detect_all(&detected), 1)
            .await
            .unwrap();
        let adapters: Vec<Arc<dyn ProviderAdapter>> = vec![Arc::new(
            FakeAdapter::new(fake_agent(), &script).with_provider("fake"),
        )];
        let runtime = Self::open(&home, &writer, &adapters);
        Self {
            _dir: dir,
            home,
            repo,
            db,
            adapters,
            writer: Some(writer),
            runtime,
        }
    }

    fn w(&self) -> &Writer {
        self.writer.as_ref().unwrap()
    }

    fn open(home: &Path, writer: &Writer, adapters: &[Arc<dyn ProviderAdapter>]) -> Runtime {
        let bus = EventBus::new(writer.handle(), 64, ObjectStore::new(home.join("objects")));
        Runtime::new(
            home,
            writer.handle(),
            writer.reader().unwrap(),
            bus,
            adapters.to_vec(),
            None,
        )
    }

    /// Apaga el runtime y vuelve a abrir la misma base.
    async fn restart(&mut self) {
        self.runtime.wait_executors().await;
        self.w().handle().flush().await.unwrap();
        if let Some(w) = self.writer.take() {
            w.shutdown();
        }
        let writer = Writer::start(&self.db).unwrap();
        self.runtime = Self::open(&self.home, &writer, &self.adapters);
        self.writer = Some(writer);
    }

    /// Una tanda: 3 agentes; el primero se detiene a media tarea, los demás terminan.
    async fn batch(&self, n: usize) {
        let mut ids = Vec::new();
        for i in 0..3 {
            let created = self
                .runtime
                .create_agent(CreateAgent {
                    project_root: self.repo.clone(),
                    title: format!("soak {n}.{i}"),
                    description: None,
                    execution: Execution::Exact("fake/fast".into()),
                    failover: FailoverPolicy::Any,
                    context_mode: ContextMode::Balanced,
                    priority: 0,
                    chat: false,
                    depends_on: Vec::new(),
                    review: false,
                })
                .await
                .unwrap();
            ids.push(created.agent_id);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        self.runtime.stop(ids[0]).await.unwrap();
        self.runtime.wait_executors().await;
        self.w().handle().flush().await.unwrap();
    }

    fn one(&self, sql: &str) -> String {
        symphony_store::open_reader(&self.db)
            .unwrap()
            .query_row(sql, [], |r| r.get::<_, rusqlite::types::Value>(0))
            .map(|v| match v {
                rusqlite::types::Value::Integer(i) => i.to_string(),
                rusqlite::types::Value::Text(t) => t,
                other => format!("{other:?}"),
            })
            .unwrap()
    }

    fn check(&self) {
        let two_open = self.one(
            "SELECT COUNT(*) FROM (SELECT agent_id FROM agent_runs WHERE ended_at IS NULL \
             GROUP BY agent_id HAVING COUNT(*) > 1)",
        );
        assert_eq!(two_open, "0", "agentes con dos runs abiertos");
        let terminal_open = self.one(
            "SELECT COUNT(*) FROM agent_runs r JOIN agents a ON a.id = r.agent_id \
             WHERE r.ended_at IS NULL AND a.state IN ('CANCELLED','COMPLETED','FAILED','MERGED')",
        );
        assert_eq!(terminal_open, "0", "runs abiertos de agentes terminales");
        assert_eq!(self.one("PRAGMA integrity_check"), "ok");
    }
}

fn rss_mb() -> u64 {
    let pid = Pid::from_u32(std::process::id());
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid).map_or(0, |p| p.memory() / (1024 * 1024))
}

/// Tandas durante `secs`, reiniciando cada `restart_every`; devuelve (reinicios, tandas).
async fn soak(secs: u64, restart_every: Duration, min_restarts: usize) -> (usize, usize, u64, u64) {
    let mut s = Soak::new().await;
    let start = Instant::now();
    let mut last_restart = start;
    let (mut restarts, mut batches) = (0, 0);
    let (mut mem_start, mut sampled) = (0, false);
    while start.elapsed() < Duration::from_secs(secs) || restarts < min_restarts {
        s.batch(batches).await;
        batches += 1;
        if !sampled && start.elapsed() >= Duration::from_secs(60) {
            mem_start = rss_mb();
            sampled = true;
        }
        if last_restart.elapsed() >= restart_every {
            s.restart().await;
            last_restart = Instant::now();
            restarts += 1;
        }
    }
    s.runtime.wait_executors().await;
    s.w().handle().flush().await.unwrap();
    s.check();
    let mem_end = rss_mb();
    if let Some(w) = s.writer.take() {
        w.shutdown();
    }
    (restarts, batches, mem_start, mem_end)
}

#[tokio::test(flavor = "multi_thread")]
async fn soak_with_two_restarts() {
    // Reinicios cada ~3 s: 2 reinicios en unos 8 s.
    let (restarts, batches, _, _) = soak(0, Duration::from_secs(3), 2).await;
    assert!(
        restarts >= 2 && batches >= 2,
        "{restarts} reinicios, {batches} tandas"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "larga duración: SOAK_SECS (60 por defecto)"]
async fn soak_long_with_restarts() {
    let secs = std::env::var("SOAK_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let (restarts, batches, mem_start, mem_end) = soak(secs, Duration::from_secs(20), 0).await;
    println!(
        "soak: {secs} s, {batches} tandas, {restarts} reinicios, RSS {mem_start} -> {mem_end} MB"
    );
    // Solo hay línea base si la corrida pasó del minuto 1.
    if mem_start > 0 {
        assert!(
            mem_end <= mem_start + 100,
            "memoria creció {mem_start} -> {mem_end} MB"
        );
    }
}
