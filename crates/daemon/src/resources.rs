//! Resource Manager (P08.S2, IDEA §5.4): muestrea CPU y RAM de la máquina cada pocos
//! segundos hacia `resource_samples` y agrega por minuto lo que pasa de 24 h.
//!
//! Los slots pesados llegan con el scheduler (P08.S3): por ahora se registra el total que
//! implica el perfil y cero en uso.
//!
//! Disco (dogfooding 2026-10-27): una vez por minuto se mira el espacio libre del disco del
//! proyecto; por debajo de [`DISK_LOW`] queda un aviso `MACHINE_PRESSURE` en el Recovery Center
//! (uno solo mientras siga abierto). Con el disco lleno los agentes se cuelgan sin error.

use std::path::{Path, PathBuf};
use std::time::Duration;

use symphony_core::PerformanceProfile;
use symphony_store::WriterHandle;
use symphony_store::resources::{NewSample, compact_samples, insert_sample};
use sysinfo::System;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Cada cuánto se muestrea (IDEA §5.4: 5–10 s).
pub const SAMPLE_EVERY: Duration = Duration::from_secs(5);
/// Las muestras más viejas que esto se agregan por minuto.
const KEEP_RAW_MS: i64 = 24 * 60 * 60 * 1000;
const BUCKET_MS: i64 = 60 * 1000;
/// Cada cuántas muestras se compacta.
const COMPACT_EVERY: u32 = 120;

/// Por debajo de esto el disco del proyecto está casi lleno.
/// ponytail: tope fijo; hacerlo configurable si alguien lo pide.
pub const DISK_LOW: u64 = 5 * 1024 * 1024 * 1024;
/// Cada cuántas muestras se mira el disco (12 × 5 s = 1 min).
const DISK_EVERY: u32 = 12;

/// El punto de montaje que contiene `path`: el prefijo más largo (sin distinguir mayúsculas en
/// Windows, y sin el `\\?\` de una ruta canónica).
pub fn mount_for<'a>(path: &Path, mounts: &'a [PathBuf]) -> Option<&'a PathBuf> {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        let s = s.strip_prefix("//?/").map(str::to_string).unwrap_or(s);
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    let target = norm(path);
    mounts
        .iter()
        .filter(|m| target.starts_with(&norm(m)))
        .max_by_key(|m| norm(m).len())
}

/// Bytes libres del disco donde está `path`.
fn disk_free(path: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mounts: Vec<PathBuf> = disks
        .iter()
        .map(|d| d.mount_point().to_path_buf())
        .collect();
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mount = mount_for(&path, &mounts)?;
    disks
        .iter()
        .find(|d| d.mount_point() == mount.as_path())
        .map(sysinfo::Disk::available_space)
}

/// Procesos pesados simultáneos que permite cada perfil (IDEA §5.4; el scheduler real
/// los toma de la config en P08.S3).
pub fn heavy_slots(profile: PerformanceProfile) -> u32 {
    match profile {
        PerformanceProfile::Performance => 3,
        PerformanceProfile::Eco | PerformanceProfile::Balanced | PerformanceProfile::Custom => 1,
    }
}

use crate::runtime::now_ms;

/// Lee CPU global y RAM usada (porcentajes 0–100). El primer CPU tras crear el `System`
/// es 0: quien llama descarta la primera lectura.
fn read(sys: &mut System) -> (f64, f64) {
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    let total = sys.total_memory();
    let ram = if total == 0 {
        0.0
    } else {
        sys.used_memory() as f64 / total as f64 * 100.0
    };
    (f64::from(sys.global_cpu_usage()), ram)
}

/// Arranca el muestreo. Termina cuando `shutdown` se cancela.
pub fn spawn(
    writer: WriterHandle,
    home: PathBuf,
    profile: PerformanceProfile,
    every: Duration,
    shutdown: CancellationToken,
    on_sample: impl Fn(f64, f64) + Send + 'static,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut sys = System::new();
        // Lectura de calentamiento: el CPU necesita dos refrescos para tener sentido.
        read(&mut sys);
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tick.tick().await;
        let mut n: u32 = 0;
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                _ = tick.tick() => {}
            }
            let (cpu_pct, ram_pct) = read(&mut sys);
            on_sample(cpu_pct, ram_pct);
            let sample = NewSample {
                cpu_pct,
                ram_pct,
                heavy_slots_used: 0,
                heavy_slots_total: heavy_slots(profile),
                process_count: None,
                profile,
                sampled_at: now_ms(),
            };
            n = n.wrapping_add(1);
            if n.is_multiple_of(DISK_EVERY) {
                let h = home.clone();
                let free = tokio::task::spawn_blocking(move || disk_free(&h))
                    .await
                    .ok()
                    .flatten();
                if let Some(free) = free.filter(|f| *f < DISK_LOW) {
                    warn_low_disk(&writer, free).await;
                }
            }
            let compact = n.is_multiple_of(COMPACT_EVERY);
            let res = writer
                .write(Box::new(move |t| {
                    insert_sample(t, &sample).map_err(into_sqlite)?;
                    if compact {
                        compact_samples(t, now_ms() - KEEP_RAW_MS, BUCKET_MS)
                            .map_err(into_sqlite)?;
                    }
                    Ok(())
                }))
                .await;
            if let Err(e) = res {
                tracing::warn!(error = %e, "no se pudo guardar la muestra de recursos");
            }
        }
    })
}

/// Abre el aviso de disco casi lleno si no hay uno abierto.
async fn warn_low_disk(writer: &WriterHandle, free: u64) {
    let gb = free as f64 / (1024.0 * 1024.0 * 1024.0);
    tracing::warn!(
        libre_gb = gb,
        "disco casi lleno: los agentes pueden colgarse"
    );
    let detail = format!(
        "Quedan {gb:.1} GB libres en el disco del proyecto: los agentes y las compilaciones pueden fallar o colgarse. Libera espacio (p. ej. `cargo clean`)."
    );
    let res = writer
        .write(Box::new(move |t| {
            let open: i64 = t.query_row(
                "SELECT COUNT(*) FROM recovery_items WHERE kind = 'MACHINE_PRESSURE' AND status = 'OPEN'",
                [],
                |r| r.get(0),
            )?;
            let project: Option<String> = t
                .query_row("SELECT id FROM projects LIMIT 1", [], |r| r.get(0))
                .ok();
            if open == 0
                && let Some(project) = project.and_then(|p| p.parse().ok())
            {
                symphony_store::repo::open_recovery_item(
                    t,
                    project,
                    None,
                    None,
                    "MACHINE_PRESSURE",
                    &detail,
                    now_ms(),
                )
                .map_err(into_sqlite)?;
            }
            Ok(())
        }))
        .await;
    if let Err(e) = res {
        tracing::warn!(error = %e, "no se pudo abrir el aviso de disco");
    }
}

fn into_sqlite(e: symphony_store::repo::RepoError) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_free_space_of_a_real_directory_is_found() {
        assert!(disk_free(&std::env::temp_dir()).is_some_and(|b| b > 0));
    }

    #[test]
    fn the_longest_mount_that_contains_the_path_wins() {
        let mounts = if cfg!(windows) {
            vec![PathBuf::from("C:\\"), PathBuf::from("D:\\")]
        } else {
            vec![PathBuf::from("/"), PathBuf::from("/home")]
        };
        let path = if cfg!(windows) {
            PathBuf::from(r"\\?\c:\Users\leo\proyecto")
        } else {
            PathBuf::from("/home/leo/proyecto")
        };
        assert_eq!(
            mount_for(&path, &mounts),
            Some(&mounts[if cfg!(windows) { 0 } else { 1 }])
        );
        assert_eq!(mount_for(Path::new("Z:/x"), &mounts[1..]), None);
    }
}
