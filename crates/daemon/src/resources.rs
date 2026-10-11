//! Resource Manager (P08.S2, IDEA §5.4): muestrea CPU y RAM de la máquina cada pocos
//! segundos hacia `resource_samples` y agrega por minuto lo que pasa de 24 h.
//!
//! Los slots pesados llegan con el scheduler (P08.S3): por ahora se registra el total que
//! implica el perfil y cero en uso.

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

/// Procesos pesados simultáneos que permite cada perfil (IDEA §5.4; el scheduler real
/// los toma de la config en P08.S3).
pub fn heavy_slots(profile: PerformanceProfile) -> u32 {
    match profile {
        PerformanceProfile::Performance => 3,
        PerformanceProfile::Eco | PerformanceProfile::Balanced | PerformanceProfile::Custom => 1,
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

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
    profile: PerformanceProfile,
    every: Duration,
    shutdown: CancellationToken,
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

fn into_sqlite(e: symphony_store::repo::RepoError) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(e))
}
