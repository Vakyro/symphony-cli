# STATUS — Symphony CLI

**Actualizado:** 2026-09-25 · por claude-code/opus-5.5
**Fase actual:** P07 · TUI y release v0.1 (rama `phase/p07-tui`)
**Paso actual:** P07.S9 · Cierre (health + revisión) y P07.S10 · Replanificación (uso real ≥ 2 semanas)
**Estado del paso:** v0.1.0 publicado en la rama; Leo empieza el uso real
**En curso por:** —

## Salud del repo
- `cargo xtask check`: ✅ (199 tests; los live se omiten sin `SYMPHONY_LIVE=1`)
- `cargo deny check`: ✅ (solo avisos de duplicados)
- CI en main: ✅ (ubuntu, windows, macos, msrv, deny). La rama `phase/p07-tui` y el tag `v0.1.0` se pushean en P07.S8.
- Tests conocidos en rojo: ninguno.

## Progreso de la fase
- [x] P07.S1 Arquitectura TUI (suscripción al bus + métodos IPC de vistas)
- [x] P07.S2 Launch, First-run y Provider Setup
- [x] P07.S3 Home
- [x] P07.S4 New Agent y Model Picker básico
- [x] P07.S5 Vista de agente (+ «Abrir en el CLI», attach de ADR-0005)
- [x] P07.S6 Providers y Recovery Center
- [x] P07.S7 Snapshots y E2E (terminal real ✅ Leo; Journey A live con Claude Code ✅ 17 s)
- [x] P07.S8 Release v0.1.0 (tag `v0.1.0`, CHANGELOG, build release: daemon idle 16.5 MB)
- [ ] P07.S9 Cierre
- [ ] P07.S10 Replanificación (gate: ≥ 2 semanas de uso real)

## Próxima acción concreta
1. Leo usa v0.1 en trabajo real (P07.S10) y anota en `docs/research/uso-v0.1.md` qué usó, qué le faltó, qué falló, cuántos handoffs hubo y si la máquina se trabó.
2. P07.S9: skill `health` + revisión de código del diff de la fase; corregir hallazgos.
3. Al terminar el uso: ADR de replanificación de P08–P16 con `the-council`, merge a `main` y tag `p07-done`.

## Handoff para el siguiente agente
- La TUI está en `crates/tui`: `app.rs` tiene el estado y la lógica pura, `ui.rs` el render, `io.rs` el IPC y `lib.rs` el loop de terminal. Lee «Decisiones tomadas» en la bitácora P07.
- Para regenerar los snapshots: `INSTA_UPDATE=always cargo nextest run -p symphony-tui`. Revísalos antes de commitear.
- Si un test deja un `symphonyd.exe` huérfano, el build falla con «Acceso denegado» (LEARNINGS P07).

## Bloqueos y preguntas para Leo
- (ninguno)

## Pendientes arrastrados
- P06–P07: repositorios de executor_changes, messages, provider_failures, tool_calls, checkpoints, checkpoint_refs, handoffs y recovery_items (bitácora P03.S3).
- Config de proveedores: `--permission-mode` de Claude (default `acceptEdits`) y sandbox de Codex (default `workspace-write`) deberían salir de `config.toml`.
- Mensajes a Claude después de su turno y el regreso del attach: `resume` (adenda de ADR-0005).
- Los cambios de estado de un agente no pasan por el bus: la TUI sondea cada 3 s (`ponytail:` en `crates/tui/src/app.rs`).

## Fases
| Fase | Estado | Tag |
|---|---|---|
| P00 | ✅ | p00-done |
| P01 | ✅ | p01-done |
| P02 | ✅ | p02-done |
| P03 | ✅ | p03-done |
| P04 | ✅ | p04-done |
| P05 | ✅ | p05-done |
| P06 | ✅ | p06-done |
| P07 | 🟡 en curso (S1–S7 ✅) | |
| P08–P16 | ⏳ (P08–P16 provisionales hasta P07.S10) | |
