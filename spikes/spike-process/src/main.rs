//! Gate de ProcessKit (P01.S7, STACK §60): kill sin huérfanos, streaming estable,
//! overhead bajo, límites predecibles y suspend/resume. Usa `node` como carga.

use std::time::{Duration, Instant};

use processkit::prelude::StreamExt;
use processkit::{Command, ProcessGroup, ProcessGroupOptions, ProcessRunner};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Árbol: node -> 3 hijos node -> cada uno 2 nietos node, todos durmiendo.
const TREE_JS: &str = r#"
const { spawn } = require('child_process');
const tag = process.argv[2], depth = Number(process.argv[3] || 0);
if (depth < 2) for (let i = 0; i < (depth === 0 ? 3 : 2); i++)
  spawn(process.execPath, [__filename, tag, String(depth + 1)], { stdio: 'ignore' });
setInterval(() => {}, 1000);
"#;

fn count_marked(sys: &mut System, marker: &str) -> usize {
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    // En Linux sysinfo también lista hilos; un zombie ya está muerto aunque falte cosecharlo.
    sys.processes()
        .values()
        .filter(|p| p.thread_kind().is_none() && p.status() != sysinfo::ProcessStatus::Zombie)
        .filter(|p| p.cmd().iter().any(|a| a.to_string_lossy().contains(marker)))
        .count()
}

/// Crea un grupo con límites; si el OS no los soporta, lo reporta como fila y devuelve None.
fn limited(name: &str, opts: ProcessGroupOptions) -> Option<ProcessGroup> {
    match ProcessGroup::with_options(opts) {
        Ok(g) => Some(g),
        Err(e) => {
            println!("| {name} | ⚠️ no disponible | {e} |");
            None
        }
    }
}

fn row(name: &str, ok: bool, detail: String) {
    println!("| {name} | {} | {detail} |", if ok { "✅" } else { "❌" });
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join("spike-process");
    std::fs::create_dir_all(&dir)?;
    let tree = dir.join("tree.js");
    std::fs::write(&tree, TREE_JS)?;
    let mut sys = System::new();

    println!("| Prueba | Resultado | Detalle |\n|---|---|---|");

    // 1a. kill_all del árbol (10 procesos) sin huérfanos.
    let marker = "pk-tree-kill";
    let group = ProcessGroup::new()?;
    println!("| Mecanismo | — | {:?} |", group.mechanism());
    let _root = group
        .start(&Command::new("node").arg(&tree).arg(marker).no_timeout())
        .await?;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let alive = count_marked(&mut sys, marker);
    let t = Instant::now();
    group.kill_all()?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let left = count_marked(&mut sys, marker);
    row(
        "kill_all del árbol",
        alive == 10 && left == 0,
        format!("{alive} vivos → {left} tras kill ({:?})", t.elapsed()),
    );
    drop(group);

    // 1b. drop del grupo (el dueño muere) sin huérfanos, a través de `cmd /c` (como los shims .cmd).
    let marker = "pk-tree-drop";
    {
        let group = ProcessGroup::new()?;
        let cmd = if cfg!(windows) {
            Command::new("cmd")
                .args(["/c", "node"])
                .arg(&tree)
                .arg(marker)
        } else {
            Command::new("sh").args(["-c", &format!("node {} {marker}", tree.display())])
        };
        let _root = group.start(&cmd.no_timeout()).await?;
        tokio::time::sleep(Duration::from_secs(3)).await;
        let alive = count_marked(&mut sys, marker);
        drop(group);
        tokio::time::sleep(Duration::from_millis(500)).await;
        let left = count_marked(&mut sys, marker);
        // En Windows el propio cmd.exe lleva el marcador en su línea de comando: 10 node + 1 cmd.
        let expected = if cfg!(windows) { 11 } else { 10 };
        row(
            "drop del grupo vía cmd /c",
            alive == expected && left == 0,
            format!("{alive} vivos → {left} tras drop"),
        );
    }

    // 2. Streaming: 200k líneas en orden, sin pérdida.
    let n = 200_000;
    let script = format!("for(let i=0;i<{n};i++)process.stdout.write(i+'\\n')");
    let t = Instant::now();
    let mut run = Command::new("node").args(["-e", &script]).start().await?;
    let mut lines = run.stdout_lines()?;
    let (mut got, mut in_order) = (0usize, true);
    while let Some(line) = lines.next().await {
        in_order &= line.trim().parse::<usize>().ok() == Some(got);
        got += 1;
    }
    drop(lines);
    let _finished = run.finish().await?;
    row(
        "streaming 200k líneas",
        got == n && in_order,
        format!("{got}/{n} en orden={in_order} en {:?}", t.elapsed()),
    );

    // 3. Overhead: 30 spawns cortos con processkit vs tokio::process.
    let reps = 30;
    let t = Instant::now();
    for _ in 0..reps {
        Command::new("node").args(["-e", "0"]).run().await?;
    }
    let pk = t.elapsed() / reps;
    let t = Instant::now();
    for _ in 0..reps {
        tokio::process::Command::new("node")
            .args(["-e", "0"])
            .output()
            .await?;
    }
    let tk = t.elapsed() / reps;
    let overhead = pk.as_secs_f64() / tk.as_secs_f64() - 1.0;
    row(
        "overhead de spawn",
        overhead < 0.25,
        format!(
            "processkit {pk:?} vs tokio {tk:?} por proceso ({:+.0} %)",
            overhead * 100.0
        ),
    );

    // 4a. Límite de memoria: 256 MB, el hijo intenta 1 GB.
    if let Some(group) = limited(
        "max_memory 256 MB",
        ProcessGroupOptions::default().max_memory(256 * 1024 * 1024),
    ) {
        let t = Instant::now();
        let res = group
        .output_string(&Command::new("node").args([
            "-e",
            "const a=[];for(let i=0;i<64;i++){a.push(Buffer.alloc(16*1024*1024,1))};console.log('alloc ok')",
        ]))
        .await;
        let blocked = !matches!(&res, Ok(s) if s.stdout().contains("alloc ok"));
        let stats = group.stats().ok();
        row(
            "max_memory 256 MB (pide 1 GB)",
            blocked,
            format!(
                "{} en {:?}; pico {:?} MB; evidencia {:?}",
                if blocked { "bloqueado" } else { "NO bloqueado" },
                t.elapsed(),
                stats
                    .as_ref()
                    .and_then(|s| s.peak_memory_bytes)
                    .map(|b| b / 1_048_576),
                group.limit_evidence()
            ),
        );
    }

    // 4b. Límite de CPU: 0.5 núcleos, carga de 4 hilos durante 4 s.
    let burn = "const {Worker}=require('worker_threads');for(let i=0;i<4;i++)new Worker('const e=Date.now()+4000;while(Date.now()<e){}',{eval:true})";
    for quota in [None, Some(0.5)] {
        let opts = quota.map_or(ProcessGroupOptions::default(), |q| {
            ProcessGroupOptions::default().cpu_quota(q)
        });
        let name = format!("cpu_quota {quota:?}");
        let Some(group) = limited(&name, opts) else {
            continue;
        };
        let t = Instant::now();
        let _ = group
            .output_string(&Command::new("node").args(["-e", burn]))
            .await;
        let wall = t.elapsed();
        let cpu = group
            .stats()
            .ok()
            .and_then(|s| s.total_cpu_time)
            .unwrap_or_default();
        let cores = cpu.as_secs_f64() / wall.as_secs_f64();
        let ok = quota.is_none_or(|q| cores <= q * 1.3);
        row(
            &name,
            ok,
            format!("{cores:.2} núcleos efectivos (CPU {cpu:?} / pared {wall:?})"),
        );
    }

    // 4c. Límite de procesos: máximo 4, el árbol quiere 10.
    let marker = "pk-tree-maxproc";
    if let Some(group) = limited(
        "max_processes 4",
        ProcessGroupOptions::default().max_processes(4),
    ) {
        let _root = group
            .start(&Command::new("node").arg(&tree).arg(marker).no_timeout())
            .await?;
        tokio::time::sleep(Duration::from_secs(3)).await;
        let alive = count_marked(&mut sys, marker);
        row(
            "max_processes 4 (el árbol quiere 10)",
            alive <= 4,
            format!("{alive} procesos vivos"),
        );
    }

    // 5. suspend/resume: el CPU del grupo no avanza mientras está suspendido.
    let group = ProcessGroup::new()?;
    let _run = group
        .start(
            &Command::new("node")
                .args(["-e", "const e=Date.now()+6000;while(Date.now()<e){}"])
                .no_timeout(),
        )
        .await?;
    tokio::time::sleep(Duration::from_millis(800)).await;
    let cpu = |g: &ProcessGroup| g.stats().ok().and_then(|s| s.total_cpu_time);
    match group.suspend() {
        Err(e) => println!("| suspend/resume | ⚠️ no disponible | {e} |"),
        Ok(()) => {
            let c0 = cpu(&group);
            tokio::time::sleep(Duration::from_secs(2)).await;
            let c1 = cpu(&group);
            group.resume()?;
            tokio::time::sleep(Duration::from_millis(800)).await;
            let c2 = cpu(&group);
            match (c0, c1, c2) {
                (Some(c0), Some(c1), Some(c2)) => {
                    let frozen = c1.saturating_sub(c0) < Duration::from_millis(50);
                    row(
                        "suspend/resume",
                        frozen && c2 > c1,
                        format!(
                            "CPU suspendido +{:?}, tras resume +{:?}",
                            c1.saturating_sub(c0),
                            c2.saturating_sub(c1)
                        ),
                    );
                }
                _ => println!(
                    "| suspend/resume | ⚠️ sin stats de CPU | suspend y resume no fallaron |"
                ),
            }
        }
    }
    drop(group);

    Ok(())
}
