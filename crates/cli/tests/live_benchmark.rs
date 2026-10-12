//! P08.S9 · Benchmark principal en vivo (L3, gasta cuota): todos los proveedores a la vez, cada
//! uno con dos mensajes mínimos (turno 1 por `spawn`, turno 2 por `send`). Mide, sobre el árbol
//! de procesos del daemon: RAM, procesos y CPU de la máquina, y cuánto tarda cada turno.
//!
//!   SYMPHONY_LIVE=1 cargo nextest run -p symphony-cli --test live_benchmark --no-capture
//!
//! Solo con permiso de Leo (dado el 2026-10-11: todos los proveedores, dos mensajes sencillos,
//! el mínimo de tokens posible). El plan por fases se apaga (`[plan] enabled = false`): son
//! mensajes sueltos, no tareas que planificar.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::symphonyd;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessesToUpdate, System};

const MODELS: [&str; 6] = [
    "claude/haiku",
    "openai/gpt-5.6-luna",
    "moonshot/default",
    "google/gemini-3.8-flash-low",
    "github/auto",
    "opencode/big-pickle",
];

const TURN_1: &str = "Responde solo con la palabra: ok. No uses herramientas.";
const TURN_2: &str = "Ahora responde solo con la palabra: listo. No uses herramientas.";

fn live() -> bool {
    std::env::var("SYMPHONY_LIVE").as_deref() == Ok("1")
}

fn git(dir: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

fn symphony(home: &Path, cwd: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_symphony"))
        .current_dir(cwd)
        .args(args)
        .env("SYMPHONY_HOME", home)
        .env("SYMPHONYD", symphonyd())
        .output()
        .unwrap();
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

struct DaemonGuard {
    home: PathBuf,
    repo: PathBuf,
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        symphony(&self.home, &self.repo, &["daemon", "stop"]);
    }
}

fn query(home: &Path, sql: &str) -> Option<String> {
    let conn = symphony_store::open_reader(&home.join("symphony.db")).ok()?;
    conn.query_row(sql, [], |r| {
        use rusqlite::types::ValueRef;
        Ok(match r.get_ref(0)? {
            ValueRef::Integer(i) => i.to_string(),
            ValueRef::Real(f) => f.to_string(),
            ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
            _ => String::new(),
        })
    })
    .ok()
}

/// Filas de una consulta, cada columna como texto.
fn rows(home: &Path, sql: &str) -> Vec<Vec<String>> {
    let Ok(conn) = symphony_store::open_reader(&home.join("symphony.db")) else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare(sql) else {
        return Vec::new();
    };
    let cols = stmt.column_count();
    stmt.query_map([], |r| {
        use rusqlite::types::ValueRef;
        (0..cols)
            .map(|i| {
                Ok(match r.get_ref(i)? {
                    ValueRef::Integer(n) => n.to_string(),
                    ValueRef::Real(f) => f.to_string(),
                    ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
                    _ => String::new(),
                })
            })
            .collect()
    })
    .map(|it| it.flatten().collect())
    .unwrap_or_default()
}

fn states(home: &Path) -> Vec<(i64, String)> {
    let Ok(conn) = symphony_store::open_reader(&home.join("symphony.db")) else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare("SELECT number, state FROM agents ORDER BY number") else {
        return Vec::new();
    };
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
}

#[derive(Default)]
struct Samples {
    cpu: Vec<f64>,
    machine_used: Vec<u64>,
    tree_rss: Vec<u64>,
    tree_procs: Vec<usize>,
}

fn sampler(daemon_pid: u32, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<Samples> {
    std::thread::spawn(move || {
        let root = Pid::from_u32(daemon_pid);
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        let mut out = Samples::default();
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(250));
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            sys.refresh_processes(ProcessesToUpdate::All, true);
            let mut tree: HashSet<Pid> = HashSet::from([root]);
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
            out.cpu.push(f64::from(sys.global_cpu_usage()));
            out.machine_used.push(sys.used_memory());
            out.tree_rss.push(
                tree.iter()
                    .filter_map(|p| sys.process(*p))
                    .map(sysinfo::Process::memory)
                    .sum(),
            );
            out.tree_procs.push(tree.len());
        }
        out
    })
}

fn percentile(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

#[test]
fn live_main_benchmark_all_providers_two_messages() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(home.join("config.toml"), "[plan]\nenabled = false\n").unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t.local"]);
    git(&repo, &["config", "user.name", "T"]);
    std::fs::write(repo.join("README.md"), "demo\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    let _guard = DaemonGuard {
        home: home.clone(),
        repo: repo.clone(),
    };

    let (ok, out) = symphony(&home, &repo, &["daemon", "start"]);
    assert!(ok, "daemon start: {out}");
    let (_, status) = symphony(&home, &repo, &["daemon", "status"]);
    let daemon_pid: u32 = status
        .lines()
        .find_map(|l| l.trim().strip_prefix("pid:"))
        .and_then(|p| p.trim().parse().ok())
        .expect("pid del daemon");

    // La máquina en reposo (con el daemon ya arriba) es la base.
    let mut base = System::new();
    base.refresh_memory();
    base.refresh_processes(ProcessesToUpdate::All, true);
    let base_used = base.used_memory();
    let daemon_idle = base
        .process(Pid::from_u32(daemon_pid))
        .map_or(0, sysinfo::Process::memory);

    let stop = Arc::new(AtomicBool::new(false));
    let samples = sampler(daemon_pid, stop.clone());
    let start = Instant::now();

    // Turno 1: un agente por proveedor, todos a la vez.
    for model in MODELS {
        let (ok, out) = symphony(&home, &repo, &["spawn", TURN_1, "--model", model]);
        eprintln!("spawn {model}: {}", if ok { "ok" } else { out.trim() });
    }
    let n = i64::try_from(MODELS.len()).unwrap();
    let wait = |label: &str, limit: Duration| -> Vec<(i64, String, u64)> {
        let t = Instant::now();
        let mut done_at: Vec<(i64, String, u64)> = Vec::new();
        loop {
            let now = states(&home);
            for (num, state) in &now {
                let terminal = matches!(
                    state.as_str(),
                    "COMPLETED" | "FAILED" | "CANCELLED" | "BLOCKED"
                );
                if terminal && !done_at.iter().any(|(k, _, _)| k == num) {
                    done_at.push((*num, state.clone(), t.elapsed().as_secs()));
                }
            }
            if i64::try_from(done_at.len()).unwrap() >= n.min(i64::try_from(now.len()).unwrap())
                && !now.is_empty()
            {
                break;
            }
            if t.elapsed() > limit {
                eprintln!("{label}: tiempo agotado con {done_at:?}");
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        done_at
    };
    let turn1 = wait("turno 1", Duration::from_secs(240));
    let turn1_secs = start.elapsed().as_secs();

    // Turno 2: un mensaje más a cada agente que terminó bien (misma sesión del CLI).
    let t2 = Instant::now();
    let mut sent = 0;
    for (num, state, _) in &turn1 {
        if state == "COMPLETED" {
            let (ok, out) = symphony(&home, &repo, &["send", &num.to_string(), TURN_2]);
            eprintln!("send #{num}: {}", if ok { "ok" } else { out.trim() });
            sent += usize::from(ok);
        }
    }
    // Espera a que cada agente vuelva a COMPLETED con su segundo run.
    let deadline = Instant::now() + Duration::from_secs(480);
    loop {
        let live_runs: i64 = query(
            &home,
            "SELECT COUNT(*) FROM agent_runs WHERE ended_at IS NULL",
        )
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1);
        let second_turns: i64 = query(&home, "SELECT COUNT(*) FROM agent_runs WHERE seq >= 2")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if live_runs == 0 && second_turns >= i64::try_from(sent).unwrap() {
            break;
        }
        if Instant::now() > deadline {
            eprintln!("turno 2: tiempo agotado");
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let turn2_secs = t2.elapsed().as_secs();
    let total_secs = start.elapsed().as_secs();
    stop.store(true, Ordering::Relaxed);
    let mut s = samples.join().unwrap();

    // Resultados por agente.
    eprintln!();
    for r in rows(
        &home,
        "SELECT a.number, r.seq, r.provider_id, r.status, COALESCE(r.end_reason,''),
                (COALESCE(r.ended_at, 0) - r.started_at) / 1000, COALESCE(a.state_reason,'')
         FROM agent_runs r JOIN agents a ON a.id = r.agent_id ORDER BY a.number, r.seq",
    ) {
        eprintln!(
            "  #{} run {} {:<10} {:<10} {:<14} {:>4} s  {}",
            r[0], r[1], r[2], r[3], r[4], r[5], r[6]
        );
    }
    eprintln!(
        "{:<4} {:<28} {:<10} {:>8} {:>6}",
        "#", "modelo", "estado", "turno 1", "runs"
    );
    for (num, state) in states(&home) {
        let model = query(
            &home,
            &format!("SELECT model_id FROM agent_runs r JOIN agents a ON a.id = r.agent_id WHERE a.number = {num} ORDER BY r.seq LIMIT 1"),
        )
        .unwrap_or_default();
        let runs = query(
            &home,
            &format!("SELECT COUNT(*) FROM agent_runs r JOIN agents a ON a.id = r.agent_id WHERE a.number = {num}"),
        )
        .unwrap_or_default();
        let secs = turn1
            .iter()
            .find(|(k, _, _)| *k == num)
            .map_or_else(|| "—".to_string(), |(_, _, s)| format!("{s} s"));
        eprintln!("{num:<4} {model:<28} {state:<10} {secs:>8} {runs:>6}");
    }
    let mb = |b: u64| b as f64 / 1_048_576.0;
    let peak_tree = s.tree_rss.iter().copied().max().unwrap_or(0);
    let peak_machine = s.machine_used.iter().copied().max().unwrap_or(base_used);
    let avg_cpu = if s.cpu.is_empty() {
        0.0
    } else {
        s.cpu.iter().sum::<f64>() / s.cpu.len() as f64
    };
    eprintln!();
    eprintln!("daemon en reposo: {:.0} MB", mb(daemon_idle));
    eprintln!(
        "RAM del árbol del daemon: pico {:.0} MB · RAM de la máquina: +{:.0} MB sobre la base",
        mb(peak_tree),
        mb(peak_machine.saturating_sub(base_used))
    );
    eprintln!(
        "CPU de la máquina: media {:.0}% · p50 {:.0}% · p95 {:.0}%",
        avg_cpu,
        percentile(&mut s.cpu, 0.5),
        percentile(&mut s.cpu, 0.95)
    );
    eprintln!(
        "procesos en el árbol: pico {}",
        s.tree_procs.iter().copied().max().unwrap_or(0)
    );
    eprintln!("turno 1: {turn1_secs} s · turno 2: {turn2_secs} s · total: {total_secs} s");
    let slots =
        query(&home, "SELECT MAX(heavy_slots_used) FROM resource_samples").unwrap_or_default();
    let queue = query(
        &home,
        "SELECT COALESCE(MAX(started_at - requested_at), 0) FROM tool_calls WHERE status IN ('DONE','FAILED')",
    )
    .unwrap_or_default();
    eprintln!("slots pesados usados (máx): {slots} · mayor espera de cola: {queue} ms");

    let completed = states(&home)
        .iter()
        .filter(|(_, s)| s == "COMPLETED")
        .count();
    assert!(
        completed >= 3,
        "al menos tres proveedores tienen que haber respondido ({completed}/{})",
        MODELS.len()
    );
}
