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

### P00.S1 · Verificar el entorno — 🟡
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** Rust no estaba instalado. Con permiso de Leo: `winget install Rustlang.Rustup` → toolchain `stable-x86_64-pc-windows-msvc`. VS 2022 Community estaba sin workload C++ (`link.exe not found`); con permiso de Leo se agregó `Microsoft.VisualStudio.Workload.NativeDesktop`.
- **Versiones:** Git 2.47.1.windows.1 · rustup 1.29.1 · rustc 1.98.1 (48a229cea 2026-09-01) · cargo 1.98.1 · componentes: clippy, rustfmt, rust-docs
- **Cómo se verificó:** pendiente `cargo new --bin hello && cargo run` tras instalar el workload C++.
- **Pendiente / notas:** `cargo-nextest` y `cargo-deny` se instalan después del workload.

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

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|

## Métricas

## Pruebas

## Estado final

## Notas para el siguiente agente
- La máquina de Leo usa PowerShell y Git Bash. Lanzar CLIs desde Git Bash bloquea al padre (LEARNINGS H4).
