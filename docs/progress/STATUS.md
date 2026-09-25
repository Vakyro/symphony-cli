# STATUS — Symphony CLI

**Actualizado:** 2026-09-24 · por claude-code/opus-5.5
**Fase actual:** P05 · Adapters y event bus (rama `phase/p05-adapters`)
**Paso actual:** P05.S1 · Trait `ProviderAdapter`
**Estado del paso:** EN CURSO
**En curso por:** claude-code/opus-5.5 desde 2026-09-24

## Salud del repo
- `cargo xtask check`: ✅ (111 tests)
- `cargo deny check`: ✅
- CI en main: ✅ (ubuntu, windows, macos, msrv, deny)
- Tests conocidos en rojo: ninguno

## Progreso de la fase actual
- [ ] P05.S1 Trait `ProviderAdapter`   ← aquí
- (resto de P05 en PLAN.md)

## Próxima acción concreta
`crates/adapters/common`: trait de STACK §18.1 + eventos canónicos (IDEA §5.3); adapter `fake` sobre `fake-agent` con una suite de contrato compartida.

## Handoff para el siguiente agente
(llenar si la sesión se cortó a media tarea — ver PLAN §4.4)
- Estaba haciendo:
- Archivo/función a medias:
- Comandos corridos y resultado:
- Hipótesis descartadas:

## Bloqueos y preguntas para Leo
- (ninguno)

## Pendientes arrastrados
- P05: verificar que el servidor del named pipe pertenece al usuario actual antes de mandar datos sensibles (bitácora P02.S8).
- P05–P07: repositorios de executor_changes, messages, provider_failures, tool_calls, checkpoints, checkpoint_refs, handoffs y recovery_items (bitácora P03.S3).
- P05: hooks por invocación (Claude `--settings`, Codex `-c hooks.*` en sintaxis PowerShell) según ADR-0003; resolver el ejecutable nativo en vez del shim `.cmd` (LEARNINGS Test A/B).

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
