//! El Resource Manager escribe muestras reales y se detiene con el apagado (P08.S2).

use std::time::Duration;

use symphony_core::PerformanceProfile;
use symphony_daemon::resources;
use symphony_store::Writer;
use symphony_store::resources::samples_since;
use tokio_util::sync::CancellationToken;

#[tokio::test(flavor = "multi_thread")]
async fn samples_cpu_and_ram_into_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("symphony.db");
    let writer = Writer::start(&db).unwrap();
    let shutdown = CancellationToken::new();
    let task = resources::spawn(
        writer.handle(),
        PerformanceProfile::Performance,
        Duration::from_millis(50),
        shutdown.clone(),
        |_, _| {},
    );

    // El primer refresco de CPU de sysinfo puede tardar (en Windows, cientos de ms):
    // se espera a tener varias muestras en vez de dormir un tiempo fijo.
    let reader = writer.reader().unwrap();
    let mut rows = Vec::new();
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        rows = samples_since(&reader, 0).unwrap();
        if rows.len() >= 3 {
            break;
        }
    }
    shutdown.cancel();
    task.await.unwrap();
    writer.handle().flush().await.unwrap();

    assert!(
        rows.len() >= 3,
        "se esperaban varias muestras, hubo {}",
        rows.len()
    );
    for r in &rows {
        assert!(
            (0.0..=100.0).contains(&r.ram_pct) && r.ram_pct > 0.0,
            "RAM {}",
            r.ram_pct
        );
        assert!(r.cpu_pct >= 0.0, "CPU {}", r.cpu_pct);
        assert_eq!(r.profile, "PERFORMANCE");
        assert_eq!(r.heavy_slots_total, 3);
        assert!(r.sampled_at > 0);
    }
    writer.shutdown();
}
