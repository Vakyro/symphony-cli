//! P09.S3: costo de los compresores sobre ~1 MB de log. `cargo bench -p symphony-context`.
//! Corren en el hilo del writer al registrar un objeto: deben ser rápidos.
// Código de bench: mismas reglas que los tests (CONSTRAINTS C3).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use criterion::{Criterion, criterion_group, criterion_main};
use symphony_context::compress::{Hint, compress};

fn log_1mb() -> String {
    let mut s = String::new();
    let mut i = 0;
    while s.len() < 1_000_000 {
        s.push_str(&format!("   Compiling dep{i} v0.1.{i} (/work/dep{i})\n"));
        if i % 5000 == 0 {
            s.push_str("error[E0308]: mismatched types\n");
        }
        i += 1;
    }
    s
}

fn bench(c: &mut Criterion) {
    let log = log_1mb();
    c.bench_function("compress_log_1mb", |b| {
        b.iter(|| compress(std::hint::black_box(&log), Hint::Log));
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
