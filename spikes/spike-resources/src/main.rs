//! Test A (P01.S3): lanza N CLIs en paralelo, uno por worktree, y muestrea
//! RAM, CPU y número de procesos del árbol de cada uno con sysinfo.
//!
//! uso: spike-resources <claude|codex> <worktree>...

use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

const TASK: &str = "Read package.json and reply with only the project name. Do not modify any file.";
const SAMPLE: Duration = Duration::from_millis(500);
const TIMEOUT: Duration = Duration::from_secs(300);

struct Agent {
    wt: PathBuf,
    out: PathBuf,
    child: Child,
    first_output: Option<Duration>,
    exit: Option<(Duration, i32)>,
    rss_peak: u64,
    rss_sum: u64,
    cpu_peak: f32,
    cpu_sum: f32,
    procs_peak: usize,
    names_at_peak: Vec<String>,
    samples: u32,
}

fn command(provider: &str) -> Result<Command, String> {
    // Los CLIs están instalados como shims .cmd de npm (cmd.exe -> node).
    let exe = |n: &str| if cfg!(windows) { format!("{n}.cmd") } else { n.to_string() };
    let mut c = match provider {
        "claude" => {
            let mut c = Command::new(exe("claude"));
            c.args(["-p", TASK, "--output-format", "stream-json", "--verbose"])
                .args(["--model", "haiku", "--allowedTools", "Read"]);
            c
        }
        "codex" => {
            let mut c = Command::new(exe("codex"));
            c.args(["exec", "--json", "-m", "gpt-5.6-luna", "-s", "read-only"])
                .args(["--skip-git-repo-check", TASK]);
            c
        }
        other => return Err(format!("proveedor desconocido: {other}")),
    };
    c.stdin(Stdio::null());
    Ok(c)
}

/// Suma memoria, CPU y cuenta procesos de `root` y todos sus descendientes.
fn tree(sys: &System, root: Pid) -> (u64, f32, usize, Vec<String>) {
    let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
    for (pid, p) in sys.processes() {
        if let Some(parent) = p.parent() {
            children.entry(parent).or_default().push(*pid);
        }
    }
    let (mut rss, mut cpu, mut n, mut names) = (0, 0.0, 0, Vec::new());
    let mut stack = vec![root];
    while let Some(pid) = stack.pop() {
        if let Some(p) = sys.process(pid) {
            rss += p.memory();
            cpu += p.cpu_usage();
            n += 1;
            names.push(p.name().to_string_lossy().into_owned());
        }
        if let Some(kids) = children.get(&pid) {
            stack.extend(kids);
        }
    }
    names.sort();
    (rss, cpu, n, names)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (provider, wts) = args.split_first().ok_or("uso: spike-resources <claude|codex> <worktree>...")?;
    if wts.is_empty() {
        return Err("falta al menos un worktree".into());
    }

    let refresh = ProcessRefreshKind::nothing().with_memory().with_cpu();
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_usage();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);
    let base_used = sys.used_memory();

    let start = Instant::now();
    let mut agents = Vec::new();
    for wt in wts {
        let wt = PathBuf::from(wt);
        let out = wt.join(format!("../{}-{provider}.jsonl", wt.file_name().and_then(|s| s.to_str()).unwrap_or("wt")));
        let mut c = command(provider)?;
        // Stdout a archivo, no a pipe: evita que el hijo herede el pipe del padre (LEARNINGS H4).
        c.current_dir(&wt).stdout(File::create(&out)?).stderr(Stdio::null());
        let child = c.spawn()?;
        agents.push(Agent {
            wt, out, child, first_output: None, exit: None,
            rss_peak: 0, rss_sum: 0, cpu_peak: 0.0, cpu_sum: 0.0, procs_peak: 0, names_at_peak: Vec::new(), samples: 0,
        });
    }

    let (mut sys_peak_delta, mut total_rss_peak, mut gcpu_sum, mut gsamples) = (0u64, 0u64, 0f32, 0u32);
    while agents.iter().any(|a| a.exit.is_none()) && start.elapsed() < TIMEOUT {
        std::thread::sleep(SAMPLE);
        sys.refresh_memory();
        sys.refresh_cpu_usage();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);
        let t = start.elapsed();
        let mut total_rss = 0;
        for a in agents.iter_mut().filter(|a| a.exit.is_none()) {
            if a.first_output.is_none() && std::fs::metadata(&a.out).map(|m| m.len() > 0).unwrap_or(false) {
                a.first_output = Some(t);
            }
            let (rss, cpu, n, names) = tree(&sys, Pid::from_u32(a.child.id()));
            total_rss += rss;
            a.rss_peak = a.rss_peak.max(rss);
            a.rss_sum += rss;
            a.cpu_peak = a.cpu_peak.max(cpu);
            a.cpu_sum += cpu;
            if n > a.procs_peak {
                a.procs_peak = n;
                a.names_at_peak = names;
            }
            a.samples += 1;
            if let Some(status) = a.child.try_wait()? {
                a.exit = Some((t, status.code().unwrap_or(-1)));
            }
        }
        total_rss_peak = total_rss_peak.max(total_rss);
        sys_peak_delta = sys_peak_delta.max(sys.used_memory().saturating_sub(base_used));
        gcpu_sum += sys.global_cpu_usage();
        gsamples += 1;
    }
    for a in agents.iter_mut().filter(|a| a.exit.is_none()) {
        a.child.kill()?;
    }

    let mb = |b: u64| b as f64 / 1_048_576.0;
    println!("| Proveedor | N | Worktree | 1.ª salida (s) | Duración (s) | RSS pico (MB) | RSS medio (MB) | CPU pico (%) | CPU media (%) | Procesos pico | Exit |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    for a in &agents {
        let s = a.samples.max(1);
        println!(
            "| {provider} | {} | {} | {} | {} | {:.0} | {:.0} | {:.0} | {:.0} | {} | {} |",
            agents.len(),
            a.wt.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
            a.first_output.map_or("—".into(), |d| format!("{:.1}", d.as_secs_f32())),
            a.exit.map_or("timeout".into(), |(d, _)| format!("{:.1}", d.as_secs_f32())),
            mb(a.rss_peak),
            mb(a.rss_sum / u64::from(s)),
            a.cpu_peak,
            a.cpu_sum / s as f32,
            a.procs_peak,
            a.exit.map_or("—".into(), |(_, c)| c.to_string()),
        );
    }
    println!(
        "\nTotal N={}: RSS pico sumado de los árboles {:.0} MB · RAM usada del sistema +{:.0} MB sobre la base · CPU global media {:.0}% · duración {:.1} s",
        agents.len(),
        mb(total_rss_peak),
        mb(sys_peak_delta),
        gcpu_sum / gsamples.max(1) as f32,
        start.elapsed().as_secs_f32()
    );
    for a in &agents {
        let mut counts: std::collections::BTreeMap<&str, u32> = Default::default();
        for n in &a.names_at_peak {
            *counts.entry(n.as_str()).or_default() += 1;
        }
        let list: Vec<String> = counts.iter().map(|(n, c)| format!("{n}×{c}")).collect();
        println!("\nProcesos de {} en el pico: {}", a.wt.display(), list.join(", "));
    }
    Ok(())
}
