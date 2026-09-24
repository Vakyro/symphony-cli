# Crates de STACK §58 — verificación (P00.S7)

Fecha: 2026-09-24 · Fuente: `cargo search` + `cargo info` contra crates.io · Agente: claude-code/opus-5.5

| Crate | Existe | Última estable | STACK §58 pide | Licencia | MSRV | Notas |
|---|---|---|---|---|---|---|
| tokio | ✅ | 1.53.1 | 1 | MIT | 1.71 | |
| tokio-util | ✅ | 0.7.19 | 0.7 | MIT | 1.71 | |
| clap | ✅ | 4.6.7 | 4 | MIT OR Apache-2.0 | 1.85 | |
| ratatui | ✅ | 0.30.2 | 0.30 | MIT | 1.88 | |
| crossterm | ✅ | 0.29.0 | 0.29 | MIT | 1.63 | |
| serde | ✅ | 1.0.229 | 1 | MIT OR Apache-2.0 | 1.56 | |
| serde_json | ✅ | 1.0.151 | 1 | MIT OR Apache-2.0 | 1.71 | |
| toml_edit | ✅ | 0.25.15 | 0.25 | MIT OR Apache-2.0 | 1.85 | |
| thiserror | ✅ | 2.0.21 | 2 | MIT OR Apache-2.0 | 1.77 | |
| miette | ✅ | 7.6.0 | 7 | Apache-2.0 | 1.70 | |
| interprocess | ✅ | 2.4.4 | 2 | 0BSD OR Apache-2.0 | 1.75 | |
| rusqlite | ✅ | 0.40.2 | 0.40 | MIT | — | `bundled` compila SQLite (necesita toolchain C: MSVC en Windows) |
| rusqlite_migration | ✅ | 2.6.0 | 2 | Apache-2.0 | **1.95** | fija el MSRV del workspace |
| ulid | ✅ | 3.0.0 | 3 | MIT | — | |
| blake3 | ✅ | 1.8.7 | 1 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | — | |
| zstd | ✅ | 0.14.0 | 0.14 | BSD-3-Clause | 1.64 | |
| tempfile | ✅ | 3.27.0 | 3 | MIT OR Apache-2.0 | 1.63 | |
| tracing | ✅ | 0.1.44 | 0.1 | MIT | 1.65 | |
| tracing-subscriber | ✅ | 0.3.23 | 0.3 | MIT | 1.65 | |
| tracing-appender | ✅ | 0.2.5 | 0.2 | MIT | 1.63 | |
| sysinfo | ✅ | 0.39.6 | 0.39 | MIT | **1.95** | |
| notify | ✅ | 8.2.0 | 8 | **CC0-1.0** | 1.77 | 9.0.0-rc.5 existe como prerelease: **no usar** hasta que sea estable. CC0-1.0 hay que agregarlo a `deny.toml` al introducirla |
| ignore | ✅ | 0.4.33 | 0.4 | Unlicense OR MIT | 1.88 | |
| tree-sitter | ✅ | 0.27.0 | 0.27 | MIT | 1.90 | |
| rmcp | ✅ | 3.4.1 | 3 | Apache-2.0 | 1.88 | |
| regex | ✅ | 1.13.1 | 1 | MIT OR Apache-2.0 | 1.65 | |
| processkit | ✅ | 3.3.4 | 3 | MIT | 1.88 | Se describe como "whole-tree kill-on-drop (no orphans)". Falta verificar en P04 que cubra Job Objects/cgroups con límites; si no, plan B de STACK §60 |
| windows-sys | ✅ | 0.61.2 | 0.61 | MIT OR Apache-2.0 | 1.71 | |
| nix | ✅ | 0.31.3 | 0.31 | MIT | 1.69 | |

**Resultado:** las 29 existen con el nombre y la versión mayor que pide STACK §58. No hace falta ningún reemplazo. MSRV del workspace: **1.95**, lo que coincide con STACK §3.1.
