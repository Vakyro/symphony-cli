# STATUS — Symphony CLI

**Actualizado:** 2026-09-24 · por claude-code/opus-5.5
**Fase actual:** P06 · Agent runtime, checkpoints y handoff (rama `phase/p06-runtime`)
**Paso actual:** P06.S1 · Ciclo de vida del agente
**Estado del paso:** EN CURSO
**En curso por:** claude-code/opus-5.5 desde 2026-09-24

## Salud del repo
- `cargo xtask check`: ✅ (133 tests)
- `cargo deny check`: ✅
- CI en main: ✅ (ubuntu, windows, macos, msrv, deny)
- Tests conocidos en rojo: ninguno

## Progreso de la fase actual
- [ ] P06.S1 Ciclo de vida del agente   ← aquí
- (resto de P06 en PLAN.md)

## Próxima acción concreta
Flujo de creación de FLOW §6: task + agente + worktree + checkpoint inicial → spawn del run con modelo exacto; rollback si falla el worktree; razón humana en esperas y bloqueos.

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
