# P08 · Runtime multiagente

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p08-runtime-multiagente |
| Inicio / cierre | 2026-10-11 / — |
| Agentes que trabajaron | claude-code/sonnet-5.5 (S1) |
| Tag | p08-done (pendiente) |
| Docs usados | ADR-0011, ADR-0013, DB §3.B/§3.C/§3.H, PLAN §P08, CONSTRAINTS A12 |

## Diseño (ADR-0013, aceptado por Leo el 2026-10-11)
- **Plan por agente (D1):** archivo temporal `.symphony/plan/<task_id>.md` en el worktree. Mientras la tarea corre documenta lo hecho y lo que falló; al terminarla se borran las instrucciones, aunque algo haya fallado. Antes de borrarlo, lo hecho y los fallos pasan al checkpoint y a la bitácora del agente (el archivo no sobrevive).
- **Sesión nueva por fase (D2):** el run saliente cierra con `PHASE_DONE` y estado `HANDED_OFF`; el daemon abre otro con el mismo proveedor y modelo y un handoff acotado. Migración 004.
- **Estados (D3):** se mantienen los 12; la fase es dato del plan, no estado del agente.
- **Alcance (D4):** F1–F4, F7–F11, F13. F12 (contabilidad de tokens) se mide antes de S3.
- **Benchmark live (S9):** todos los proveedores, 2 mensajes sencillos por agente, mínimo de tokens.
- **Instalación (CONSTRAINTS A12):** hooks, skills y MCP por invocación; nada se escribe en el proyecto del usuario salvo el worktree del agente.

## Pasos

### P08.S1 · Migración 004 — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-11
- **Qué se hizo:** migración `004_multiagent.sql` con `milestones`, `task_dependencies`, `processes`, `resource_samples`, `validation_runs`, `validation_checks`, `merge_requests`, `merge_conflicts` y `reviews`; FK `tasks.milestone_id → milestones` (reconstrucción de `tasks`); `PHASE_DONE` en el CHECK de `agent_runs.end_reason` (reconstrucción de `agent_runs`) y en `RunEndReason`; spec de DB actualizada.
- **Archivos clave:** `migrations/004_multiagent.sql`, `crates/store/src/lib.rs`, `crates/store/src/tests.rs`, `crates/core/src/enums.rs`, `docs/spec/symphony_database.md`.
- **Cómo se verificó:** `cargo nextest run -p symphony-store -p symphony-core` → 102 passed. El test `migration_004_keeps_data_and_adds_milestones_and_phase_done` parte de una DB en la 003 con tasks, agente, dos runs, tool call y mensaje; comprueba que no se pierde nada, `foreign_key_check` en 0, `PHASE_DONE` aceptado y un motivo inventado rechazado, un solo run abierto por agente, FK de milestone, y DAG sin auto-dependencias ni duplicados.
- **Pendiente / notas:** los enums de las tablas nuevas (estado de milestone, trigger de validación, etc.) se agregan al core en el paso que los usa (S5, S6, S7), con su fila en el spec.

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| PLAN P08.S1 | «Migración 002» | Es la **004** | 002 (salud) y 003 (contexto) se ejecutaron antes por ADR-0010 |
| DB §3.C `agent_runs.end_reason` | 8 valores | 9: se agrega `PHASE_DONE` | ADR-0013 D2 |

## Notas para el siguiente agente
- Reconstruir una tabla padre: patrón `defer_foreign_keys` + `_bak` + `DROP` + reinsertar con su nombre (002, 003, 004). No uses `RENAME`.
- Siguiente: P08.S2 (clasificador de operaciones y Resource Manager). Antes de S3, medir F12 (tokens por proveedor) para que la concurrencia por cuota no sea ciega.
