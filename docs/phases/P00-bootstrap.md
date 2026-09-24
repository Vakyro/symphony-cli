# P00 · Arranque del repositorio

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p00-bootstrap |
| Inicio / cierre | 2026-09-24 / — |
| Agentes que trabajaron | claude-code/opus-5.5 (S0–S4) |
| Tag | — |
| Docs usados | PLAN §0–§7 y Apéndices A–D; IDEA §3, §5, §10 |

## Pasos

### P00.S0 · Prevalidación — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** comparación de 7 herramientas existentes contra IDEA §3/§5 y Test D manual en ambos sentidos (Claude → Codex, Codex → Claude).
- **Archivos clave:** `docs/research/prevalidacion.md`, `docs/research/prevalidacion/`
- **Cómo se verificó:** ambas corridas terminan con tests en verde (32/32 y 28/28) sin reexplicar la tarea. Ninguna herramienta hace handoff a otro proveedor ni scheduler de recursos.
- **Gate:** pasa. Leo aprobó seguir ("sigue con la construcción", 2026-09-24).
- **Pendiente / notas:** no se probó Claude Squad a mano (requiere WSL + tmux). Hallazgos H1–H7 en `LEARNINGS.md`.

### P00.S1 · Verificar el entorno — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** Rust no estaba instalado. Con permiso de Leo: `winget install Rustlang.Rustup` → toolchain `stable-x86_64-pc-windows-msvc`. VS 2022 Community estaba sin workload C++ (`link.exe not found`); con permiso de Leo se agregó `Microsoft.VisualStudio.Workload.NativeDesktop`.
- **Versiones:** Git 2.47.1.windows.1 · rustup 1.29.1 · rustc 1.98.1 (48a229cea 2026-09-01) · cargo 1.98.1 · MSVC 14.43.34808 · cargo-nextest 0.9.146 · componentes: clippy, rustfmt, rust-docs
- **Cómo se verificó:** `cargo new --bin hello && cargo run` → `Hello, world!`
- **Pendiente / notas:** `cargo install --locked cargo-nextest cargo-deny` tarda más de 10 min en esta máquina. En CI se usan binarios precompilados.

### P00.S2 · Crear el repositorio — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `git init -b main`, `.gitignore`, `.gitattributes` (`eol=lf`), `LICENSE` MIT, `README.md`. Se commiteó también el spec tal como estaba en la raíz, para que `git status` quede limpio; S3 lo mueve con `git mv`.
- **Cómo se verificó:** `git status` limpio tras `chore: init repo [P00.S2]`.
- **Commits:** 9f9f848

### P00.S3 · Copiar la documentación — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** 6 docs movidos a `docs/spec/` + `Catalogo-Skills-ClaudeCode.pdf` copiado desde `~/Downloads`. `docs/spec/README.md` con mapa y precedencia. Prevalidación movida a `docs/research/`. H1–H7 en `LEARNINGS.md`.
- **Cómo se verificó:** `ls docs/spec` → 7 docs + README.md
- **Commits:** 47fe3c2

### P00.S4 · Archivos de coordinación — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `AGENTS.md`, `docs/phases/_TEMPLATE.md` y `docs/adr/0000-template.md` extraídos tal cual de los Apéndices D, B y C. `CLAUDE.md`, `GEMINI.md` y `.github/copilot-instructions.md` son una línea que redirige. `STATUS.md`, `SESSIONS.md`, esta bitácora.
- **Cómo se verificó:** STATUS apunta a P00.S5 (con S1 pendiente de verificar).
- **Pendiente / notas:** no se creó `docs/research/.gitkeep`: la carpeta ya tiene archivos.

### P00.S5 · CONSTRAINTS.md — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `CONSTRAINTS.md` con reglas de PLAN §2, prohibiciones de STACK §36, presupuestos de IDEA §3/§6 y STACK §39, política de `unsafe` (STACK §27), errores y dependencias. Cada regla dice cómo se comprueba (lints de workspace y `deny.toml` se crean en S6).
- **Cómo se verificó:** el archivo existe; `AGENTS.md` lo referencia (paso 4 de "Antes de hacer nada").

### P00.S6 · Esqueleto del workspace — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** workspace con `crates/{protocol,core,daemon,cli,testkit}` y `xtask/`. Edition 2024, MSRV 1.95, `resolver = "3"`. `rust-toolchain.toml` (stable + rustfmt + clippy), `rustfmt.toml`, `clippy.toml` (unwrap/expect solo en tests), `deny.toml` con las prohibiciones de STACK §36. Lints de workspace de CONSTRAINTS C3/C6. `[profile.release]` con `lto = "thin"` y sin `panic = "abort"`. Los binarios solo usan std: `--version` no justifica traer clap todavía (llega con el primer comando real).
- **Archivos clave:** `Cargo.toml`, `xtask/src/main.rs`, `crates/cli/src/main.rs`, `crates/cli/tests/version.rs`, `deny.toml`, `.cargo/config.toml`
- **Cómo se verificó:** `cargo xtask check` → fmt ok, clippy `-D warnings` ok, nextest 2 passed. `cargo run -p symphony-cli -- --version` → `symphony 0.0.1`.

### P00.S7 · Verificar que las crates del stack existen — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** las 29 crates de STACK §58 se buscaron con `cargo search` y `cargo info`. Todas existen con el nombre y la versión mayor esperados, así que no hay reemplazos. El MSRV más alto es 1.95 (`rusqlite_migration`, `sysinfo`), igual al de STACK §3.1. `notify` 9 está en rc: se fija la 8.2. `notify` es CC0-1.0 → agregar a `deny.toml` cuando se introduzca.
- **Archivos clave:** `docs/research/crates.md`, `docs/adr/0001-versiones-y-crates.md`
- **Cómo se verificó:** ADR-0001 existe con la lista fijada.
- **Pendiente / notas:** falta confirmar en P04 que ProcessKit cubre límites de recursos por Job Object/cgroup.

### P00.S8 · CI mínima — 🟡 (sin remoto)
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `.github/workflows/ci.yml` con tres jobs: `check` en matriz ubuntu/windows/macos (`cargo xtask check`, nextest precompilado, `Swatinem/rust-cache`), `msrv` (`cargo +1.95 check`) y `deny` (`cargo-deny-action`). En `deny.toml`: `unused-allowed-license = "allow"` y `allow-wildcard-paths = true` (para los path deps del workspace).
- **Cómo se verificó:** local: `cargo xtask check` ✅ y `cargo deny check` → `advisories ok, bans ok, licenses ok, sources ok`. **Workflow no ejecutado:** no hay remoto. Falta que Leo decida si lo crea.

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|

## Decisiones tomadas
- (ninguna todavía)

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| PLAN P00.S2 | `.gitignore` con la lista de reglas | Se agregó `graphify-smart-out/` y un `.gitattributes` con `eol=lf` | Salida de herramienta local; la máquina tiene `autocrlf=true` y sin esto rustfmt/CI verían diffs de CRLF |
| PLAN P00.S2 | Primer commit solo de init | Incluye también el spec, luego movido en S3 | Único modo de dejar `git status` limpio sin ignorar el spec |
| IDEA §6 | Routing y latencia de eventos "en milisegundos" | CONSTRAINTS R3/R4 fijan objetivos p99 < 10 ms y < 50 ms | Hace falta un número para que el presupuesto sea verificable. Se ajusta con ADR si la medición real no aplica |

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|

## Métricas

## Pruebas

## Estado final

## Notas para el siguiente agente
- La máquina de Leo usa PowerShell y Git Bash. Lanzar CLIs desde Git Bash bloquea al padre (LEARNINGS H4).
