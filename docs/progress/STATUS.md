# STATUS — Symphony CLI

**Actualizado:** 2026-09-24 · por claude-code/opus-5.5
**Fase actual:** P06 · Agent runtime, checkpoints y handoff (rama `phase/p06-runtime`)
**Paso actual:** P06.S4 · Handoff v1
**Estado del paso:** PENDIENTE
**En curso por:** —

## Salud del repo
- `cargo xtask check`: ✅ (149 tests)
- `cargo deny check`: ✅
- CI en main: ✅ (ubuntu, windows, macos, msrv, deny)
- Tests conocidos en rojo: ninguno

## Progreso de la fase actual
- [x] P06.S1 Ciclo de vida del agente
- [x] P06.S2 Mensajes y tool calls
- [x] P06.S3 Checkpoints incrementales
- [ ] P06.S4 Handoff v1   ← aquí
- (resto de P06 en PLAN.md)

## Próxima acción concreta
Prompt de arranque armado desde el checkpoint (objetivo, plan vigente, diff, fallos recientes, archivos tocados) con plantilla fija; guardar `handoffs` con `tokens_sent` estimados y `outcome`. Snapshot `insta` del prompt para un checkpoint fijo.

## Handoff para el siguiente agente
(llenar si la sesión se cortó a media tarea — ver PLAN §4.4)
- Estaba haciendo:
- Archivo/función a medias:
- Comandos corridos y resultado:
- Hipótesis descartadas:

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
| P06 | 🟡 en curso | |
| P07–P16 | ⏳ (P08–P16 provisionales hasta P07.S10) | |
