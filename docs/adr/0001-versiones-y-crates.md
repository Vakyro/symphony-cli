# ADR-0001 · Versiones y crates

- **Estado:** ACEPTADO
- **Fecha:** 2026-09-24
- **Autor:** claude-code/opus-5.5 · **Aprobado por:** — (sin cambios respecto al spec)
- **Fase/paso:** P00.S7

## Contexto
STACK §58 se escribió sin compilar. PLAN P00.S7 pide confirmar que cada crate existe, con qué versión y licencia, sobre todo ProcessKit, `rmcp` y `rusqlite_migration`.

## Opciones consideradas
1. Fijar las versiones de STACK §58 tal cual.
2. Reemplazar las crates que no existan o no encajen.

## Decisión
Opción 1. Toolchain: Rust stable (1.98.1 en la máquina de Leo), Edition 2024, **MSRV 1.95**. Versiones que se usan al agregar cada crate en su fase (requisito semver, `Cargo.lock` commiteado, sin saltos de major automáticos):

```toml
tokio = { version = "1.53", default-features = false }
tokio-util = "0.7.19"
clap = { version = "4.6", features = ["derive"] }
ratatui = "0.30.2"
crossterm = "0.29"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
toml_edit = { version = "0.25.15", features = ["serde"] }
thiserror = "2.0.21"
miette = "7.6"
interprocess = { version = "2.4", features = ["tokio"] }
rusqlite = { version = "0.40.2", features = ["bundled"] }
rusqlite_migration = "2.6"
ulid = "3"
blake3 = "1.8"
zstd = "0.14"
tempfile = "3.27"
tracing = "0.1.44"
tracing-subscriber = "0.3.23"
tracing-appender = "0.2.5"
sysinfo = "0.39.6"
notify = "8.2"          # no 9.0.0-rc
ignore = "0.4.33"
tree-sitter = "0.27"
rmcp = "3.4"
regex = "1.13"
processkit = "3.3"
windows-sys = "0.61.2"  # cfg(windows)
nix = "0.31.3"          # cfg(unix)
```

## Evidencia
`docs/research/crates.md` (tabla con versión, licencia y MSRV de cada crate, tomada de `cargo search` y `cargo info` el 2026-09-24).

## Consecuencias
- MSRV 1.95 en `Cargo.toml` y en el job `msrv` de CI.
- `notify` (CC0-1.0) obliga a agregar `CC0-1.0` a `[licenses].allow` en `deny.toml` cuando llegue (P09).
- ProcessKit existe, pero su cobertura de límites de recursos (Job Objects/cgroups) se valida en P04. Si no alcanza, se aplica el plan B de STACK §60 con un ADR nuevo.
