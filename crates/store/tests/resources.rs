//! Muestras de recursos: inserción y agregación por minuto (P08.S2).

use symphony_core::PerformanceProfile;
use symphony_store::open_in_memory;
use symphony_store::resources::{NewSample, compact_samples, insert_sample, samples_since};

fn sample(at: i64, cpu: f64, used: u32) -> NewSample {
    NewSample {
        cpu_pct: cpu,
        ram_pct: 50.0,
        heavy_slots_used: used,
        heavy_slots_total: 3,
        process_count: Some(10),
        profile: PerformanceProfile::Balanced,
        sampled_at: at,
    }
}

#[test]
fn old_samples_collapse_to_one_per_minute_and_recent_ones_stay() {
    let conn = open_in_memory().unwrap();
    // Minuto 0: tres muestras viejas; minuto 1: una sola vieja; y dos recientes.
    for (at, cpu, used) in [
        (1_000, 10.0, 0),
        (2_000, 30.0, 2),
        (3_000, 20.0, 1),
        (61_000, 5.0, 0),
    ] {
        insert_sample(&conn, &sample(at, cpu, used)).unwrap();
    }
    insert_sample(&conn, &sample(500_000, 90.0, 3)).unwrap();
    insert_sample(&conn, &sample(500_001, 80.0, 3)).unwrap();

    let removed = compact_samples(&conn, 100_000, 60_000).unwrap();
    assert_eq!(removed, 3);

    let rows = samples_since(&conn, 0).unwrap();
    assert_eq!(rows.len(), 4, "1 agregada + 1 suelta + 2 recientes");
    let agg = &rows[0];
    assert_eq!(agg.sampled_at, 0, "empieza en el minuto");
    assert!((agg.cpu_pct - 20.0).abs() < 1e-9, "promedio de CPU");
    assert_eq!(agg.heavy_slots_used, 2, "máximo de slots");
    assert_eq!(agg.profile, "BALANCED");

    // Idempotente: compactar otra vez no cambia nada.
    assert_eq!(compact_samples(&conn, 100_000, 60_000).unwrap(), 0);
    assert_eq!(samples_since(&conn, 0).unwrap().len(), 4);
}

#[test]
fn the_profile_check_rejects_unknown_profiles() {
    let conn = open_in_memory().unwrap();
    assert!(
        conn.execute(
            "INSERT INTO resource_samples (cpu_pct, ram_pct, heavy_slots_used, heavy_slots_total, performance_profile, sampled_at)
             VALUES (1, 1, 0, 1, 'TURBO', 0)",
            []
        )
        .is_err()
    );
}
