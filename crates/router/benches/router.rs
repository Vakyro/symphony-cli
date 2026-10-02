//! P10.S4/S6: latencia del router (STACK §25.1). `cargo bench -p symphony-router`.
//! Debe quedar en microsegundos: se decide en cada spawn y en cada failover.
// Código de bench: mismas reglas que los tests (CONSTRAINTS C3).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use criterion::{Criterion, criterion_group, criterion_main};
use symphony_core::{Health, HealthEvent, ProviderState, SpeedClass};
use symphony_router::{Candidate, Request, Weights, route};

fn candidates(n: usize) -> Vec<Candidate> {
    (0..n)
        .map(|i| Candidate {
            model_id: format!("p{}/m{i}", i % 5),
            provider_id: format!("p{}", i % 5),
            display_name: format!("Modelo {i}"),
            unavailable: None,
            supports_tools: true,
            context_window: Some(200_000),
            speed: Some(if i % 3 == 0 {
                SpeedClass::Fast
            } else {
                SpeedClass::Medium
            }),
            base_score: 0.4 + (i % 7) as f64 / 20.0,
            health: Health {
                state: ProviderState::Healthy,
                ..Health::unknown(0)
            }
            .apply(
                &HealthEvent::Quota {
                    used_fraction: (i % 9) as f64 / 10.0,
                    reset_at: None,
                    reserve: 0.2,
                },
                0,
            ),
            model_health: None,
            reserve: 0.2,
            recent_failures: (i % 4) as u32,
            load: (i % 3) as u32,
        })
        .collect()
}

fn bench(c: &mut Criterion) {
    let weights = Weights::default();
    let req = Request {
        now: 1,
        profile: "@code",
        weights,
        needed_context: Some(30_000),
        need_tools: true,
        allow_reserve: false,
        excluded: &[],
    };
    for n in [5usize, 20, 100] {
        let cands = candidates(n);
        c.bench_function(&format!("route_{n}_candidates"), |b| {
            b.iter(|| route(std::hint::black_box(&cands), &req))
        });
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
