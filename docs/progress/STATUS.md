# STATUS — Symphony CLI

**Actualizado:** 2026-09-24 · por antigravity/gemini-3.7-flash
**Fase actual:** P07 · TUI y release v0.1 (rama `phase/p07-tui`)
**Paso actual:** P07.S1 · Arquitectura TUI
**Estado del paso:** LISTO PARA EMPEZAR
**En curso por:** —

## Salud del repo
- `cargo xtask check`: ✅ (162 tests; 1 live omitido)
- `cargo deny check`: ✅
- CI en main: ✅ (ubuntu, windows, macos, msrv, deny)
- Tests conocidos en rojo: ninguno

## Progreso de la fase anterior (P06)
- [x] P06.S1 Ciclo de vida del agente
- [x] P06.S2 Mensajes y tool calls
- [x] P06.S3 Checkpoints incrementales
- [x] P06.S4 Handoff v1
- [x] P06.S5 Cambio de executor
- [x] P06.S6 Heartbeat y reclaim
- [x] P06.S7 Comandos de agente
- [x] P06.S8 Prueba de aceptación: forced kill
- [x] P06.S9 Cierre de fase P06

## Próxima acción concreta
Arrancar la fase P07 en la rama `phase/p07-tui`: crear el crate `tui` con arquitectura desacoplada comunicándose exclusivamente con el daemon vía IPC (suscripción de eventos + requests) sin abrir SQLite directamente (P07.S1).

## Handoff para el siguiente agente
- Fase P06 completada y testeada con 162 pruebas unitarias y de integración pasando en verde.
- P06 verificó Journey C, forced kill acceptance test (Test D) y la suite completa de comandos CLI sobre IPC.
- Siguiente paso: comenzar P07.S1 creando el crate `tui` y la estructura de vistas de terminal con `ratatui` y `crossterm`.

## Bloqueos y preguntas para Leo
- (ninguno)

## Pendientes arrastrados
- P06–P07: repositorios de executor_changes, messages, provider_failures, tool_calls, checkpoints, checkpoint_refs, handoffs y recovery_items (bitácora P03.S3).
- Config de proveedores: `--permission-mode` de Claude (default `acceptEdits`) y sandbox de Codex (default `workspace-write`) deberían salir de `config.toml`.

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
| P07 | 🟡 en curso | |
| P08–P16 | ⏳ (P08–P16 provisionales hasta P07.S10) | |
