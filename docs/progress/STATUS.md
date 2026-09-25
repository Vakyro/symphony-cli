# STATUS — Symphony CLI

**Actualizado:** 2026-09-24 · por claude-code/opus-5.5
**Fase actual:** P05 · Adapters y event bus (rama `phase/p05-adapters`)
**Paso actual:** P05.S7 · Cierre
**Estado del paso:** BLOQUEADO (necesita permiso de Leo para la sesión real)
**En curso por:** claude-code/opus-5.5 desde 2026-09-24

## Salud del repo
- `cargo xtask check`: ✅ (133 tests)
- `cargo deny check`: ✅
- CI en main: ✅ (ubuntu, windows, macos, msrv, deny)
- Tests conocidos en rojo: ninguno

## Progreso de la fase actual
- [x] P05.S1 Trait `ProviderAdapter`
- [x] P05.S2 Event bus
- [x] P05.S3 `symphony hook emit`
- [x] P05.S4 Adapter Claude Code
- [x] P05.S5 Adapter Codex
- [x] P05.S6 Registro de proveedores y modelos
- [ ] P05.S7 Cierre   ← aquí (revisión de seguridad hecha; falta la sesión real con permiso)
- (resto de P05 en PLAN.md)

## Próxima acción concreta
Con permiso de Leo: `SYMPHONY_LIVE=1 cargo nextest run -p symphony-cli --no-capture live_` (Claude haiku + Codex luna, un `git status`). Si pasa: merge a `main`, tag `p05-done`.

## Handoff para el siguiente agente
(llenar si la sesión se cortó a media tarea — ver PLAN §4.4)
- Estaba haciendo:
- Archivo/función a medias:
- Comandos corridos y resultado:
- Hipótesis descartadas:

## Bloqueos y preguntas para Leo
- ¿Permiso para una sesión real corta de Claude Code y de Codex (criterio de salida de P05)?

## Pendientes arrastrados
- P05–P07: repositorios de executor_changes, messages, provider_failures, tool_calls, checkpoints, checkpoint_refs, handoffs y recovery_items (bitácora P03.S3).

## Fases
| Fase | Estado | Tag |
|---|---|---|
| P00 | ✅ | p00-done |
| P01 | ✅ | p01-done |
| P02 | ✅ | p02-done |
| P03 | ✅ | p03-done |
| P04 | ✅ | p04-done |
| P05 | 🟡 en curso | |
| P06–P16 | ⏳ (P08–P16 provisionales hasta P07.S10) | |
