//! P08.S9 · Benchmark principal (STACK §25.2), nivel L2 con fake-agents.
//!
//! Escenarios: 1 agente · 3 agentes en 3 worktrees · 3 agentes con una prueba pesada cada uno
//! (el gate de validación corre `npm test`, clase 3, por el scheduler con 1 slot).
//!
//! Mide, con la máquina real: RAM (delta sobre la base), CPU (media, p50 y p95 de la máquina),
//! procesos hijos simultáneos, latencia de cola del scheduler y latencia de eventos del bus.
//! Imprime una tabla (`--no-capture`) y comprueba las invariantes: todos terminan y nunca
//! hay más de un pesado a la vez.
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
use symphony_core::{ContextMode, FailoverPolicy, PerformanceProfile};
use symphony_daemon::bus::EventBus;
use symphony_daemon::providers;
use symphony_daemon::runtime::{CreateAgent, Execution, Runtime, WatchdogTiming};
use symphony_daemon::scheduling::Scheduling;
use symphony_object_store::ObjectStore;
use symphony_scheduler::Limits;
use symphony_store::Writer;
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
    repo: PathBuf,
    db: PathBuf,
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
    let script = dir.path().join("fake.toml");
    std::fs::write(&script, WORK).unwrap();
    let detected: Vec<Box<dyn ProviderAdapter>> = vec![Box::new(
        FakeAdapter::new(fake_agent(), &script).with_provider("fake"),
    )];
    let used: Vec<Arc<dyn ProviderAdapter>> = vec![Arc::new(
        FakeAdapter::new(fake_agent(), &script).with_provider("fake"),
    )];
    providers::save(&writer.handle(), providers::detect_all(&detected), 1)
        .await
        .unwrap();
    let bus = EventBus::new(writer.handle(), 256, ObjectStore::new(home.join("objects")));
    let runtime = Runtime::new_with_watchdog(
        &home,
        writer.handle(),
        writer.reader().unwrap(),
        bus.clone(),
        used,
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
        repo,
        db,
        writer,
        bus,
        runtime,
        shutdown,
    }
}

fn req(e: &Env, title: &str) -> CreateAgent {
    CreateAgent {
        project_root: e.repo.clone(),
        title: title.into(),
        description: None,
        execution: Execution::Exact("fake/fast".into()),
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
    queue_p50_ms: f64,
    queue_p95_ms: f64,
    event_p50_ms: f64,
    event_p95_ms: f64,
    event_p99_ms: f64,
    heavy_overlap: bool,
}

/// Corre `n` agentes en paralelo y mide.
async fn scenario(name: &'static str, n: usize, heavy: bool) -> Report {
    let e = env(heavy).await;
    // Latencia de eventos: del `occurred_at` del evento a su llegada a un suscriptor del bus.
    let latencies: Arc<Mutex<Vec<f64>>> = Arc::default();
    let sink = latencies.clone();
    let mut rx = e.bus.subscribe();
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
                }
            }
        }
    });

    // Base: la máquina antes de lanzar nada.
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
        handles.push(tokio::spawn(
            async move { runtime.create_agent(request).await },
        ));
    }
    let mut ids = Vec::new();
    for h in handles {
        ids.push(h.await.unwrap().unwrap().agent_id);
    }
    // Esperar a que todos terminen (incluida la validación pesada).
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
            // Con 1 slot: el intervalo [started, finished] de dos pesados no puede solaparse.
            if a.1 < b_end && b.1 < a_end && a.1.min(b.1) > 0 {
                overlap = true;
            }
        }
    }
    let mut events = latencies.lock().unwrap().clone();
    let cpu_avg = if s.cpu.is_empty() {
        0.0
    } else {
        s.cpu.iter().sum::<f64>() / s.cpu.len() as f64
    };
    let peak_mem = s.used_mem.iter().copied().max().unwrap_or(base_mem);
    let report = Report {
        scenario: name,
        agents: n,
        wall_ms: wall.as_millis(),
        ram_peak_mb: peak_mem.saturating_sub(base_mem) as f64 / 1_048_576.0,
        ram_daemon_mb: s.self_rss.iter().copied().max().unwrap_or(0) as f64 / 1_048_576.0,
        cpu_avg,
        cpu_p50: percentile(&mut s.cpu, 0.5),
        cpu_p95: percentile(&mut s.cpu, 0.95),
        max_children: s.children.iter().copied().max().unwrap_or(0),
        queue_p50_ms: percentile(&mut queue, 0.5),
        queue_p95_ms: percentile(&mut queue, 0.95),
        event_p50_ms: percentile(&mut events, 0.5),
        event_p95_ms: percentile(&mut events, 0.95),
        event_p99_ms: percentile(&mut events, 0.99),
        heavy_overlap: overlap,
    };
    e.shutdown.cancel();
    e.writer.shutdown();
    let _ = ids;
    report
}

fn print(reports: &[Report]) {
    println!();
    println!(
        "{:<28} {:>6} {:>8} {:>10} {:>11} {:>9} {:>8} {:>8} {:>7} {:>10} {:>10} {:>9} {:>9}",
        "escenario",
        "agent",
        "tiempo",
        "RAM Δ MB",
        "RAM proc MB",
        "CPU avg",
        "CPU p50",
        "CPU p95",
        "hijos",
        "cola p50",
        "cola p95",
        "evt p50",
        "evt p95"
    );
    for r in reports {
        println!(
            "{:<28} {:>6} {:>6}ms {:>10.0} {:>11.0} {:>8.0}% {:>7.0}% {:>7.0}% {:>7} {:>8.0}ms {:>8.0}ms {:>7.0}ms {:>7.0}ms",
            r.scenario,
            r.agents,
            r.wall_ms,
            r.ram_peak_mb,
            r.ram_daemon_mb,
            r.cpu_avg,
            r.cpu_p50,
            r.cpu_p95,
            r.max_children,
            r.queue_p50_ms,
            r.queue_p95_ms,
            r.event_p50_ms,
            r.event_p95_ms
        );
    }
    // CONSTRAINTS R4: la latencia de evento se acota por p99 (objetivo < 50 ms).
    for r in reports {
        println!(
            "  {:<28} latencia de evento p99: {:.0} ms",
            r.scenario, r.event_p99_ms
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
    ];
    print(&reports);

    for r in &reports {
        assert!(
            !r.heavy_overlap,
            "{}: dos pruebas pesadas a la vez",
            r.scenario
        );
        assert!(r.event_p95_ms < 2_000.0, "{}: eventos lentos", r.scenario);
    }
    // Con una prueba pesada cada uno y un solo slot, el último espera a los dos anteriores.
    let heavy = &reports[2];
    assert!(
        heavy.queue_p95_ms >= 1_000.0,
        "la cola del scheduler no se nota: p95 {} ms",
        heavy.queue_p95_ms
    );
}
