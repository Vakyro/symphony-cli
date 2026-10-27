//! P08.S9 / P13.S4 · Benchmark E2E completo (STACK §25.2), nivel L2 con fake-agents.
//!
//! Escenarios: 1 agente · 3 agentes en 3 worktrees · 3 agentes con una prueba pesada cada uno
//! · forced failover · daemon restart · large logs · 10k events · 100k events.
//!
//! Mide, con la máquina real: RAM (delta sobre la base y del daemon), CPU (media, p50 y p95),
//! procesos hijos simultáneos, latencia de arranque (startup), cola del scheduler, eventos,
//! checkpoints, armado de handoff y failover.
//!
//!   cargo nextest run -p symphony-daemon --test benchmark --no-capture
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use symphony_adapter_common::ProviderAdapter;
use symphony_core::{AgentId, ContextMode, FailoverPolicy, PerformanceProfile, ProjectId};
use symphony_daemon::bus::EventBus;
use symphony_daemon::providers;
use symphony_daemon::runtime::{CreateAgent, Execution, Runtime, WatchdogTiming};
use symphony_daemon::scheduling::Scheduling;
use symphony_daemon::views;
use symphony_object_store::ObjectStore;
use symphony_scheduler::Limits;
use symphony_store::{NewEvent, Writer, repo};
use symphony_testkit::FakeAdapter;
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio_util::sync::CancellationToken;

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

/// Lo que se muestrea de la máquina mientras corre un escenario.
#[derive(Default)]
struct Samples {
    cpu: Vec<f64>,
    used_mem: Vec<u64>,
    children: Vec<usize>,
    self_rss: Vec<u64>,
}

/// Muestrea CPU, RAM y procesos hijos de este proceso cada 100 ms hasta que se apague `stop`.
fn sampler(stop: Arc<AtomicBool>) -> std::thread::JoinHandle<Samples> {
    std::thread::spawn(move || {
        let me = Pid::from_u32(std::process::id());
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        let mut out = Samples::default();
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            sys.refresh_processes(ProcessesToUpdate::All, true);
            out.cpu.push(f64::from(sys.global_cpu_usage()));
            out.used_mem.push(sys.used_memory());
            // Descendientes (hijos, nietos…) de este proceso.
            let mut tree: std::collections::HashSet<Pid> = std::collections::HashSet::from([me]);
            loop {
                let before = tree.len();
                for (pid, p) in sys.processes() {
                    if p.parent().is_some_and(|pp| tree.contains(&pp)) {
                        tree.insert(*pid);
                    }
                }
                if tree.len() == before {
                    break;
                }
            }
            out.children.push(tree.len() - 1);
            out.self_rss
                .push(sys.process(me).map_or(0, sysinfo::Process::memory));
        }
        out
    })
}

fn rss_now_mb() -> f64 {
    let me = Pid::from_u32(std::process::id());
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[me]), true);
    sys.process(me).map_or(0, sysinfo::Process::memory) as f64 / 1_048_576.0
}

fn percentile(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let idx = ((v.len() - 1) as f64 * p).round() as usize;
    v[idx]
}

struct Env {
    _dir: tempfile::TempDir,
    home: PathBuf,
    repo: PathBuf,
    db: PathBuf,
    adapters: Vec<Arc<dyn ProviderAdapter>>,
    writer: Writer,
    bus: EventBus,
    runtime: Runtime,
    shutdown: CancellationToken,
}

const WORK: &str = r#"
[[step]]
kind = "say"
text = "Trabajando"

[[step]]
kind = "sleep"
ms = 400

[[step]]
kind = "edit"
path = "fix.txt"
content = "ok"
"#;

/// Un repo con una «prueba pesada» (`npm test` quema CPU ~1,2 s) y la validación del gate.
async fn env(heavy_validation: bool) -> Env {
    env_with(&[("fake", WORK)], heavy_validation).await
}

async fn env_with(providers_: &[(&'static str, &str)], heavy_validation: bool) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(repo.join("scripts")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("README.md"), "demo\n").unwrap();
    std::fs::write(
        repo.join("package.json"),
        "{\"name\":\"bench\",\"version\":\"1.0.0\",\"scripts\":{\"test\":\"node scripts/heavy.js\"}}\n",
    )
    .unwrap();
    std::fs::write(
        repo.join("scripts/heavy.js"),
        "const end = Date.now() + 1200;\nwhile (Date.now() < end) { Math.sqrt(Math.random()); }\n",
    )
    .unwrap();
    if heavy_validation {
        std::fs::create_dir_all(repo.join(".symphony")).unwrap();
        std::fs::write(
            repo.join(".symphony/project.toml"),
            "[project]\nname = \"bench\"\ndefault_branch = \"main\"\n\n[validation]\ntier2 = [\"npm test\"]\ngate = 2\n",
        )
        .unwrap();
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);

    std::fs::create_dir_all(&home).unwrap();
    let db = home.join("symphony.db");
    let writer = Writer::start(&db).unwrap();
    let mut detected: Vec<Box<dyn ProviderAdapter>> = Vec::new();
    let mut used: Vec<Arc<dyn ProviderAdapter>> = Vec::new();
    for (provider, script_body) in providers_ {
        let script = dir.path().join(format!("{provider}.toml"));
        std::fs::write(&script, script_body).unwrap();
        detected.push(Box::new(
            FakeAdapter::new(fake_agent(), &script).with_provider(provider),
        ));
        used.push(Arc::new(
            FakeAdapter::new(fake_agent(), &script).with_provider(provider),
        ));
    }
    providers::save(&writer.handle(), providers::detect_all(&detected), 1)
        .await
        .unwrap();
    let bus = EventBus::new(writer.handle(), 256, ObjectStore::new(home.join("objects")));
    let runtime = Runtime::new_with_watchdog(
        &home,
        writer.handle(),
        writer.reader().unwrap(),
        bus.clone(),
        used.clone(),
        None,
        WatchdogTiming {
            heartbeat_every: Duration::from_secs(5),
            stale_after: Duration::from_secs(600),
        },
    );
    let shutdown = CancellationToken::new();
    let scheduling = Arc::new(Scheduling::new(Limits::balanced()));
    scheduling.spawn(writer.handle(), shutdown.clone());
    runtime.set_scheduling(scheduling.clone());
    symphony_daemon::resources::spawn(
        writer.handle(),
        PerformanceProfile::Balanced,
        Duration::from_millis(200),
        shutdown.clone(),
        move |cpu, ram| scheduling.set_pressure(cpu, ram),
    );
    Env {
        _dir: dir,
        home,
        repo,
        db,
        adapters: used,
        writer,
        bus,
        runtime,
        shutdown,
    }
}

fn req(e: &Env, title: &str) -> CreateAgent {
    req_model(e, title, "fake/fast")
}

fn req_model(e: &Env, title: &str, model: &str) -> CreateAgent {
    CreateAgent {
        project_root: e.repo.clone(),
        title: title.into(),
        description: None,
        execution: Execution::Exact(model.into()),
        failover: FailoverPolicy::Any,
        context_mode: ContextMode::Balanced,
        priority: 0,
        chat: false,
        depends_on: Vec::new(),
        review: false,
    }
}

struct Report {
    scenario: &'static str,
    agents: usize,
    wall_ms: u128,
    ram_peak_mb: f64,
    ram_daemon_mb: f64,
    cpu_avg: f64,
    cpu_p50: f64,
    cpu_p95: f64,
    max_children: usize,
    startup_ms: Option<f64>,
    queue_p50_ms: Option<f64>,
    queue_p95_ms: Option<f64>,
    event_p50_ms: Option<f64>,
    event_p95_ms: Option<f64>,
    event_p99_ms: Option<f64>,
    checkpoint_ms: Option<f64>,
    handoff_ms: Option<f64>,
    failover_ms: Option<f64>,
    heavy_overlap: bool,
}

type BusListener = (
    Arc<Mutex<Vec<f64>>>,
    Arc<Mutex<Vec<(AgentId, i64)>>>,
    CancellationToken,
    tokio::task::JoinHandle<()>,
);

/// Suscriptor al bus que registra la latencia `now - occurred_at` de cada evento y el
/// instante de cada checkpoint tomado.
fn spawn_bus_listener(bus: &EventBus) -> BusListener {
    let latencies: Arc<Mutex<Vec<f64>>> = Arc::default();
    let checkpoints: Arc<Mutex<Vec<(AgentId, i64)>>> = Arc::default();
    let sink = latencies.clone();
    let cp_sink = checkpoints.clone();
    let mut rx = bus.subscribe();
    let mut cp_rx = bus.checkpoints_taken();
    let stop_events = CancellationToken::new();
    let listen = stop_events.clone();
    let listener = tokio::spawn(async move {
        loop {
            tokio::select! {
                () = listen.cancelled() => break,
                ev = rx.recv() => match ev {
                    Ok(ev) => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |d| d.as_millis() as i64);
                        if let Ok(mut s) = sink.lock() {
                            s.push((now - ev.occurred_at).max(0) as f64);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                },
                cp = cp_rx.recv() => match cp {
                    Ok(agent) => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |d| d.as_millis() as i64);
                        if let Ok(mut c) = cp_sink.lock() {
                            c.push((agent, now));
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => {}
                }
            }
        }
    });
    (latencies, checkpoints, stop_events, listener)
}

/// Extrae de la base las métricas de checkpoint, handoff y failover si existen.
fn db_latencies(db: &Path, cp_times: &[(AgentId, i64)]) -> (Option<f64>, Option<f64>, Option<f64>) {
    let conn = symphony_store::open_reader(db).unwrap();
    // Checkpoint latency: desde el último evento significativo antes del checkpoint hasta su creación.
    let mut cp_deltas: Vec<f64> = Vec::new();
    for (agent, taken_at) in cp_times {
        let ev_at: Option<i64> = conn
            .query_row(
                "SELECT MAX(occurred_at) FROM events WHERE agent_id = ?1 AND occurred_at <= ?2
                 AND type IN ('FileModified','ToolFinished','TurnFinished')",
                rusqlite::params![agent.to_string(), taken_at],
                |r| r.get(0),
            )
            .ok()
            .flatten();
        if let Some(ev) = ev_at {
            cp_deltas.push((taken_at - ev).max(0) as f64);
        }
    }
    if cp_deltas.is_empty() {
        let mut stmt = conn
            .prepare(
                "SELECT c.created_at - (
                    SELECT MAX(e.occurred_at) FROM events e
                    WHERE e.agent_id = c.agent_id AND e.occurred_at <= c.created_at
                      AND e.type IN ('FileModified','ToolFinished','TurnFinished')
                 )
                 FROM checkpoints c WHERE c.seq > 1",
            )
            .unwrap();
        for d in stmt
            .query_map([], |r| r.get::<_, Option<i64>>(0))
            .unwrap()
            .flatten()
            .flatten()
        {
            cp_deltas.push(d.max(0) as f64);
        }
    }
    let checkpoint_ms = (!cp_deltas.is_empty()).then(|| percentile(&mut cp_deltas, 0.95));

    // Handoff build latency: de `handoffs.build_ms` cuando usa checkpoint (cambio de executor).
    let mut handoffs: Vec<f64> = conn
        .prepare("SELECT build_ms FROM handoffs WHERE checkpoint_id IS NOT NULL")
        .unwrap()
        .query_map([], |r| r.get::<_, i64>(0))
        .unwrap()
        .map(|r| r.unwrap().max(0) as f64)
        .collect();
    let handoff_ms = (!handoffs.is_empty()).then(|| percentile(&mut handoffs, 0.95));

    // Failover latency: desde el fallo de proveedor hasta el `started_at` del run sucesor.
    let mut failovers: Vec<f64> = conn
        .prepare(
            "SELECT r.started_at - COALESCE(f.occurred_at, ec.occurred_at)
             FROM executor_changes ec
             JOIN agent_runs r ON r.id = ec.to_run_id
             LEFT JOIN provider_failures f ON f.id = ec.failure_id
             WHERE ec.reason = 'FAILOVER'",
        )
        .unwrap()
        .query_map([], |r| r.get::<_, i64>(0))
        .unwrap()
        .map(|r| r.unwrap().max(0) as f64)
        .collect();
    let failover_ms = (!failovers.is_empty()).then(|| percentile(&mut failovers, 0.95));

    (checkpoint_ms, handoff_ms, failover_ms)
}

/// Corre `n` agentes en paralelo y mide.
async fn scenario(name: &'static str, n: usize, heavy: bool) -> Report {
    let e = env(heavy).await;
    let (latencies, cp_times, stop_events, listener) = spawn_bus_listener(&e.bus);

    let mut base = System::new();
    base.refresh_memory();
    let base_mem = base.used_memory();
    let stop = Arc::new(AtomicBool::new(false));
    let samples = sampler(stop.clone());

    let started = Instant::now();
    let mut handles = Vec::new();
    for i in 0..n {
        let runtime = e.runtime.clone();
        let request = req(&e, &format!("Tarea {i}"));
        handles.push(tokio::spawn(async move {
            let t0 = Instant::now();
            let c = runtime.create_agent(request).await.unwrap();
            (c.agent_id, t0.elapsed().as_secs_f64() * 1_000.0)
        }));
    }
    let mut ids = Vec::new();
    let mut startups = Vec::new();
    for h in handles {
        let (id, ms) = h.await.unwrap();
        ids.push(id);
        startups.push(ms);
    }
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let done: usize = {
            let conn = symphony_store::open_reader(&e.db).unwrap();
            conn.query_row(
                "SELECT COUNT(*) FROM agents WHERE state = 'COMPLETED'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap() as usize
        };
        if done == n {
            break;
        }
        assert!(Instant::now() < deadline, "no terminaron: {done}/{n}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    e.runtime.wait_executors().await;
    e.bus.checkpoints_idle().await;
    let wall = started.elapsed();
    stop.store(true, Ordering::Relaxed);
    let mut s = samples.join().unwrap();
    stop_events.cancel();
    let _ = listener.await;
    e.writer.handle().flush().await.unwrap();

    // Latencia de cola y solapamiento de pesados, desde `tool_calls`.
    let calls: Vec<(i64, i64, Option<i64>)> = {
        let conn = symphony_store::open_reader(&e.db).unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT requested_at, COALESCE(started_at, requested_at), finished_at FROM tool_calls
                 WHERE tool_name LIKE 'validation:%' AND op_class >= 3",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let mut queue: Vec<f64> = calls
        .iter()
        .map(|(r, st, _)| (st - r).max(0) as f64)
        .collect();
    let mut overlap = false;
    for (i, a) in calls.iter().enumerate() {
        for b in calls.iter().skip(i + 1) {
            let (a_end, b_end) = (a.2.unwrap_or(i64::MAX), b.2.unwrap_or(i64::MAX));
            if a.1 < b_end && b.1 < a_end && a.1.min(b.1) > 0 {
                overlap = true;
            }
        }
    }
    let mut events = latencies.lock().unwrap().clone();
    let cp_list = cp_times.lock().unwrap().clone();
    let (checkpoint_ms, handoff_ms, failover_ms) = db_latencies(&e.db, &cp_list);
    let cpu_avg = if s.cpu.is_empty() {
        0.0
    } else {
        s.cpu.iter().sum::<f64>() / s.cpu.len() as f64
    };
    let peak_mem = s.used_mem.iter().copied().max().unwrap_or(base_mem);
    let ram_daemon_mb = s
        .self_rss
        .iter()
        .copied()
        .max()
        .map_or_else(rss_now_mb, |b| b as f64 / 1_048_576.0);
    let report = Report {
        scenario: name,
        agents: n,
        wall_ms: wall.as_millis(),
        ram_peak_mb: peak_mem.saturating_sub(base_mem) as f64 / 1_048_576.0,
        ram_daemon_mb,
        cpu_avg,
        cpu_p50: percentile(&mut s.cpu, 0.5),
        cpu_p95: percentile(&mut s.cpu, 0.95),
        max_children: s.children.iter().copied().max().unwrap_or(0),
        startup_ms: Some(percentile(&mut startups, 0.95)),
        queue_p50_ms: (!queue.is_empty()).then(|| percentile(&mut queue, 0.5)),
        queue_p95_ms: (!queue.is_empty()).then(|| percentile(&mut queue, 0.95)),
        event_p50_ms: Some(percentile(&mut events, 0.5)),
        event_p95_ms: Some(percentile(&mut events, 0.95)),
        event_p99_ms: Some(percentile(&mut events, 0.99)),
        checkpoint_ms,
        handoff_ms,
        failover_ms,
        heavy_overlap: overlap,
    };
    e.shutdown.cancel();
    e.writer.shutdown();
    let _ = ids;
    report
}

/// Escenario `forced failover`: `alpha` agota su cuota tras editar y `beta` retoma el mismo agente.
async fn scenario_failover() -> Report {
    let first = r#"
[[step]]
kind = "edit"
path = "from-alpha.txt"
content = "kept\n"
[[step]]
kind = "say"
text = "Next: seguir con beta"
[[step]]
kind = "quota_exhausted"
resets_at = 1890300000
"#;
    let second = r#"
[[step]]
kind = "edit"
path = "from-beta.txt"
content = "continued\n"
"#;
    let e = env_with(&[("alpha", first), ("beta", second)], false).await;
    let (latencies, cp_times, stop_events, listener) = spawn_bus_listener(&e.bus);

    let mut base = System::new();
    base.refresh_memory();
    let base_mem = base.used_memory();
    let stop = Arc::new(AtomicBool::new(false));
    let samples = sampler(stop.clone());

    let started = Instant::now();
    let t0 = Instant::now();
    let created = e
        .runtime
        .create_agent(req_model(&e, "Failover de cuota", "alpha/fast"))
        .await
        .unwrap();
    let startup_ms = t0.elapsed().as_secs_f64() * 1_000.0;

    e.runtime.wait_executors().await;
    e.bus.checkpoints_idle().await;
    let wall = started.elapsed();
    stop.store(true, Ordering::Relaxed);
    let mut s = samples.join().unwrap();
    stop_events.cancel();
    let _ = listener.await;
    e.writer.handle().flush().await.unwrap();

    assert!(created.worktree.join("from-alpha.txt").is_file());
    assert!(created.worktree.join("from-beta.txt").is_file());

    let mut events = latencies.lock().unwrap().clone();
    let cp_list = cp_times.lock().unwrap().clone();
    let (checkpoint_ms, handoff_ms, failover_ms) = db_latencies(&e.db, &cp_list);
    assert!(failover_ms.is_some(), "debe registrar failover");
    assert!(
        handoff_ms.is_some(),
        "debe registrar handoff con checkpoint"
    );

    let cpu_avg = if s.cpu.is_empty() {
        0.0
    } else {
        s.cpu.iter().sum::<f64>() / s.cpu.len() as f64
    };
    let peak_mem = s.used_mem.iter().copied().max().unwrap_or(base_mem);
    let ram_daemon_mb = s
        .self_rss
        .iter()
        .copied()
        .max()
        .map_or_else(rss_now_mb, |b| b as f64 / 1_048_576.0);
    e.shutdown.cancel();
    e.writer.shutdown();

    Report {
        scenario: "forced failover",
        agents: 1,
        wall_ms: wall.as_millis(),
        ram_peak_mb: peak_mem.saturating_sub(base_mem) as f64 / 1_048_576.0,
        ram_daemon_mb,
        cpu_avg,
        cpu_p50: percentile(&mut s.cpu, 0.5),
        cpu_p95: percentile(&mut s.cpu, 0.95),
        max_children: s.children.iter().copied().max().unwrap_or(0),
        startup_ms: Some(startup_ms),
        queue_p50_ms: None,
        queue_p95_ms: None,
        event_p50_ms: Some(percentile(&mut events, 0.5)),
        event_p95_ms: Some(percentile(&mut events, 0.95)),
        event_p99_ms: Some(percentile(&mut events, 0.99)),
        checkpoint_ms,
        handoff_ms,
        failover_ms,
        heavy_overlap: false,
    }
}

/// Escenario `daemon restart`: cierra y reabre el runtime y el writer a media sesión y reanuda
/// el agente desde su checkpoint.
async fn scenario_daemon_restart() -> Report {
    let e = env(false).await;
    let second_req = req(&e, "Después del reinicio");
    let (latencies, cp_times, stop_events, listener) = spawn_bus_listener(&e.bus);

    let mut base = System::new();
    base.refresh_memory();
    let base_mem = base.used_memory();
    let stop = Arc::new(AtomicBool::new(false));
    let samples = sampler(stop.clone());

    let started = Instant::now();
    let created = e
        .runtime
        .create_agent(req(&e, "Antes del reinicio"))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.bus.checkpoints_idle().await;
    stop_events.cancel();
    let _ = listener.await;
    e.writer.handle().flush().await.unwrap();
    e.shutdown.cancel();
    e.writer.shutdown();

    // Reinicio del daemon sobre la misma base (mismo patrón que `soak.rs`).
    let restart_t0 = Instant::now();
    let writer2 = Writer::start(&e.db).unwrap();
    let bus2 = EventBus::new(
        writer2.handle(),
        256,
        ObjectStore::new(e.home.join("objects")),
    );
    let runtime2 = Runtime::new(
        &e.home,
        writer2.handle(),
        writer2.reader().unwrap(),
        bus2.clone(),
        e.adapters.clone(),
        None,
    );
    let restart_ms = restart_t0.elapsed().as_secs_f64() * 1_000.0;

    let (latencies2, cp_times2, stop_events2, listener2) = spawn_bus_listener(&bus2);
    let handoff = runtime2
        .prepare_handoff(created.agent_id, "Reinicio del daemon")
        .await
        .unwrap();
    let second = runtime2.create_agent(second_req).await.unwrap();
    runtime2.wait_executors().await;
    bus2.checkpoints_idle().await;
    let wall = started.elapsed();
    stop.store(true, Ordering::Relaxed);
    let mut s = samples.join().unwrap();
    stop_events2.cancel();
    let _ = listener2.await;
    writer2.handle().flush().await.unwrap();

    let mut events = latencies.lock().unwrap().clone();
    events.extend(latencies2.lock().unwrap().iter().copied());
    let mut cp_list = cp_times.lock().unwrap().clone();
    cp_list.extend(cp_times2.lock().unwrap().iter().copied());
    let (checkpoint_ms, db_handoff_ms, _) = db_latencies(&e.db, &cp_list);
    let handoff_ms = db_handoff_ms.or(Some(handoff.build_ms.max(0) as f64));
    let _ = second;

    let cpu_avg = if s.cpu.is_empty() {
        0.0
    } else {
        s.cpu.iter().sum::<f64>() / s.cpu.len() as f64
    };
    let peak_mem = s.used_mem.iter().copied().max().unwrap_or(base_mem);
    let ram_daemon_mb = s
        .self_rss
        .iter()
        .copied()
        .max()
        .map_or_else(rss_now_mb, |b| b as f64 / 1_048_576.0);
    writer2.shutdown();

    Report {
        scenario: "daemon restart",
        agents: 2,
        wall_ms: wall.as_millis(),
        ram_peak_mb: peak_mem.saturating_sub(base_mem) as f64 / 1_048_576.0,
        ram_daemon_mb,
        cpu_avg,
        cpu_p50: percentile(&mut s.cpu, 0.5),
        cpu_p95: percentile(&mut s.cpu, 0.95),
        max_children: s.children.iter().copied().max().unwrap_or(0),
        startup_ms: Some(restart_ms),
        queue_p50_ms: None,
        queue_p95_ms: None,
        event_p50_ms: Some(percentile(&mut events, 0.5)),
        event_p95_ms: Some(percentile(&mut events, 0.95)),
        event_p99_ms: Some(percentile(&mut events, 0.99)),
        checkpoint_ms,
        handoff_ms,
        failover_ms: None,
        heavy_overlap: false,
    }
}

/// Escenario `large logs`: un fake-agent que escupe ~4 MB de salida (mensajes grandes al object store).
async fn scenario_large_logs() -> Report {
    let chunk = "L".repeat(256 * 1024);
    let mut script = String::new();
    for _ in 0..16 {
        script.push_str(&format!("[[step]]\nkind = \"say\"\ntext = \"{chunk}\"\n\n"));
    }
    script.push_str("[[step]]\nkind = \"edit\"\npath = \"done.txt\"\ncontent = \"ok\"\n");

    let e = env_with(&[("fake", &script)], false).await;
    let (latencies, cp_times, stop_events, listener) = spawn_bus_listener(&e.bus);

    let mut base = System::new();
    base.refresh_memory();
    let base_mem = base.used_memory();
    let stop = Arc::new(AtomicBool::new(false));
    let samples = sampler(stop.clone());

    let started = Instant::now();
    let t0 = Instant::now();
    let created = e
        .runtime
        .create_agent(req(&e, "Salida de 4 MB"))
        .await
        .unwrap();
    let startup_ms = t0.elapsed().as_secs_f64() * 1_000.0;

    e.runtime.wait_executors().await;
    e.bus.checkpoints_idle().await;
    let handoff = e
        .runtime
        .prepare_handoff(created.agent_id, "Tras 4 MB de salida")
        .await
        .unwrap();
    let wall = started.elapsed();
    stop.store(true, Ordering::Relaxed);
    let mut s = samples.join().unwrap();
    stop_events.cancel();
    let _ = listener.await;
    e.writer.handle().flush().await.unwrap();

    let mut events = latencies.lock().unwrap().clone();
    let cp_list = cp_times.lock().unwrap().clone();
    let (checkpoint_ms, _, _) = db_latencies(&e.db, &cp_list);
    let cpu_avg = if s.cpu.is_empty() {
        0.0
    } else {
        s.cpu.iter().sum::<f64>() / s.cpu.len() as f64
    };
    let peak_mem = s.used_mem.iter().copied().max().unwrap_or(base_mem);
    let ram_daemon_mb = s
        .self_rss
        .iter()
        .copied()
        .max()
        .map_or_else(rss_now_mb, |b| b as f64 / 1_048_576.0);
    e.shutdown.cancel();
    e.writer.shutdown();

    Report {
        scenario: "large logs (4 MB)",
        agents: 1,
        wall_ms: wall.as_millis(),
        ram_peak_mb: peak_mem.saturating_sub(base_mem) as f64 / 1_048_576.0,
        ram_daemon_mb,
        cpu_avg,
        cpu_p50: percentile(&mut s.cpu, 0.5),
        cpu_p95: percentile(&mut s.cpu, 0.95),
        max_children: s.children.iter().copied().max().unwrap_or(0),
        startup_ms: Some(startup_ms),
        queue_p50_ms: None,
        queue_p95_ms: None,
        event_p50_ms: Some(percentile(&mut events, 0.5)),
        event_p95_ms: Some(percentile(&mut events, 0.95)),
        event_p99_ms: Some(percentile(&mut events, 0.99)),
        checkpoint_ms,
        handoff_ms: Some(handoff.build_ms.max(0) as f64),
        failover_ms: None,
        heavy_overlap: false,
    }
}

/// Escenario `10k / 100k events`: inserta `count` eventos por el writer (en lotes de hasta
/// `BATCH_MAX` en vuelo) y mide la latencia de escritura por lote y de lectura de las vistas
/// (`list_agents_rows`, `views::tasks`, `views::activity`).
async fn scenario_events(name: &'static str, count: usize) -> Report {
    let e = env(false).await;
    let created = e
        .runtime
        .create_agent(req(&e, "Agente con muchos eventos"))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    let (project_str, agent_str, run_str): (String, String, String) = {
        let conn = symphony_store::open_reader(&e.db).unwrap();
        conn.query_row(
            "SELECT a.project_id, a.id, r.id FROM agents a JOIN agent_runs r ON r.agent_id = a.id LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    };
    let project_id: ProjectId = project_str.parse().unwrap();
    let _ = created;

    let mut base = System::new();
    base.refresh_memory();
    let base_mem = base.used_memory();
    let stop = Arc::new(AtomicBool::new(false));
    let samples = sampler(stop.clone());

    let handle = e.writer.handle();
    let started = Instant::now();
    let batch_size = 1_000usize;
    let mut write_latencies = Vec::with_capacity(count / batch_size + 1);
    for chunk_start in (0..count).step_by(batch_size) {
        let chunk_end = (chunk_start + batch_size).min(count);
        let t_batch = Instant::now();
        for i in chunk_start..chunk_end {
            handle
                .event(NewEvent {
                    project_id: project_str.clone(),
                    agent_id: Some(agent_str.clone()),
                    run_id: Some(run_str.clone()),
                    event_type: "CommandFinished".into(),
                    source: "HOOK".into(),
                    payload_json: Some(format!(r#"{{"cmd":"cargo test","exit":0,"i":{i}}}"#)),
                    occurred_at: i as i64,
                })
                .await
                .unwrap();
        }
        assert_eq!(handle.flush().await.unwrap().events_failed, 0);
        // Latencia real de persistencia por evento amortizada en el tramo de 1 000 eventos.
        let per_ev_ms =
            t_batch.elapsed().as_secs_f64() * 1_000.0 / (chunk_end - chunk_start) as f64;
        write_latencies.push(per_ev_ms);
    }

    // Medir lectura de las vistas principales con 10k / 100k eventos en la base.
    let conn = symphony_store::open_reader(&e.db).unwrap();
    let mut view_latencies = Vec::new();
    for _ in 0..5 {
        let t_view = Instant::now();
        let rows = repo::list_agents_rows(&conn, Some(project_id), true).unwrap();
        let _ = repo::home_rows(&conn, project_id).unwrap();
        let _ = views::tasks(&conn, &project_str).unwrap();
        let _ = views::activity(&conn, &agent_str, 50).unwrap();
        assert_eq!(rows.len(), 1);
        view_latencies.push(t_view.elapsed().as_secs_f64() * 1_000.0);
    }
    let wall = started.elapsed();
    stop.store(true, Ordering::Relaxed);
    let mut s = samples.join().unwrap();

    let cpu_avg = if s.cpu.is_empty() {
        0.0
    } else {
        s.cpu.iter().sum::<f64>() / s.cpu.len() as f64
    };
    let peak_mem = s.used_mem.iter().copied().max().unwrap_or(base_mem);
    let ram_daemon_mb = s
        .self_rss
        .iter()
        .copied()
        .max()
        .map_or_else(rss_now_mb, |b| b as f64 / 1_048_576.0);
    e.shutdown.cancel();
    e.writer.shutdown();

    // En los escenarios de volumen de eventos, `event_p*` refleja la latencia de escritura por
    // evento y `queue_p*` la latencia de lectura de vistas (Home + Tasks + Activity).
    Report {
        scenario: name,
        agents: 1,
        wall_ms: wall.as_millis(),
        ram_peak_mb: peak_mem.saturating_sub(base_mem) as f64 / 1_048_576.0,
        ram_daemon_mb,
        cpu_avg,
        cpu_p50: percentile(&mut s.cpu, 0.5),
        cpu_p95: percentile(&mut s.cpu, 0.95),
        max_children: s.children.iter().copied().max().unwrap_or(0),
        startup_ms: None,
        queue_p50_ms: Some(percentile(&mut view_latencies, 0.5)),
        queue_p95_ms: Some(percentile(&mut view_latencies, 0.95)),
        event_p50_ms: Some(percentile(&mut write_latencies, 0.5)),
        event_p95_ms: Some(percentile(&mut write_latencies, 0.95)),
        event_p99_ms: Some(percentile(&mut write_latencies, 0.99)),
        checkpoint_ms: None,
        handoff_ms: None,
        failover_ms: None,
        heavy_overlap: false,
    }
}

fn fmt_opt(v: Option<f64>) -> String {
    match v {
        Some(x) if x < 1.0 && x > 0.0 => format!("{x:>6.2}ms"),
        Some(x) => format!("{x:>6.0}ms"),
        None => "      — ".into(),
    }
}

fn print(reports: &[Report]) {
    println!();
    println!(
        "{:<26} {:>5} {:>8} {:>8} {:>9} {:>7} {:>7} {:>7} {:>5} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "escenario",
        "agent",
        "tiempo",
        "RAM Δ",
        "RAM proc",
        "CPU avg",
        "CPU p50",
        "CPU p95",
        "hijos",
        "startup",
        "cola p50",
        "cola p95",
        "evt p50",
        "evt p95",
        "evt p99",
        "ckpt p95",
        "handoff",
        "failover"
    );
    for r in reports {
        println!(
            "{:<26} {:>5} {:>6}ms {:>6.0}MB {:>7.0}MB {:>6.0}% {:>6.0}% {:>6.0}% {:>5} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
            r.scenario,
            r.agents,
            r.wall_ms,
            r.ram_peak_mb,
            r.ram_daemon_mb,
            r.cpu_avg,
            r.cpu_p50,
            r.cpu_p95,
            r.max_children,
            fmt_opt(r.startup_ms),
            fmt_opt(r.queue_p50_ms),
            fmt_opt(r.queue_p95_ms),
            fmt_opt(r.event_p50_ms),
            fmt_opt(r.event_p95_ms),
            fmt_opt(r.event_p99_ms),
            fmt_opt(r.checkpoint_ms),
            fmt_opt(r.handoff_ms),
            fmt_opt(r.failover_ms),
        );
    }
    println!();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn main_benchmark_three_agents_and_one_heavy_test_at_a_time() {
    let reports = vec![
        scenario("1 agente", 1, false).await,
        scenario("3 agentes / 3 worktrees", 3, false).await,
        scenario("3 agentes + prueba pesada", 3, true).await,
        scenario_failover().await,
        scenario_daemon_restart().await,
        scenario_large_logs().await,
        scenario_events("10k events (write/views)", 10_000).await,
        scenario_events("100k events (write/views)", 100_000).await,
    ];
    print(&reports);

    for r in &reports {
        assert!(
            !r.heavy_overlap,
            "{}: dos pruebas pesadas a la vez (CONSTRAINTS A5)",
            r.scenario
        );
        // CONSTRAINTS R2: RAM del daemon < 100 MB.
        assert!(
            r.ram_daemon_mb < 100.0,
            "{}: RAM del daemon {:.1} MB excede el tope de 100 MB (CONSTRAINTS R2)",
            r.scenario,
            r.ram_daemon_mb
        );
        assert!(
            r.wall_ms < 120_000,
            "{}: tardó {} ms (> 2 min)",
            r.scenario,
            r.wall_ms
        );
    }
    // Con una prueba pesada cada uno y un solo slot, el último espera a los dos anteriores.
    let heavy = &reports[2];
    let heavy_q95 = heavy.queue_p95_ms.unwrap_or(0.0);
    assert!(
        heavy_q95 >= 1_000.0,
        "la cola del scheduler no se nota: p95 {heavy_q95} ms"
    );
}
