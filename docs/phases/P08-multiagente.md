# P08 · Runtime multiagente

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p08-runtime-multiagente |
| Inicio / cierre | 2026-10-11 / — |
| Agentes que trabajaron | claude-code/sonnet-5.5 (S1–S2) |
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

### P08.S2 · Clasificador de operaciones y Resource Manager — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-11
- **Qué se hizo:** crate `symphony-scheduler` con `classify(tool_name, command) -> OpClass` (clases 0–4 de IDEA §5.4): reglas por programa y subcomando (`npm`/`pnpm`/`yarn`/`bun`, `cargo`, `git`, `go`, `docker`, `pip`…), costo del segmento más caro en comandos compuestos (`&&`, `;`, `|`), prefijos (`sudo`, `FOO=1`, `cmd /c`, `pwsh -Command`, `bash -c`) y rutas/extensiones ignorados; un comando desconocido es «medio», nunca «gratis». El recorder ya clasifica con él (antes todo comando era 2). Resource Manager en el daemon: muestreo de CPU y RAM con `sysinfo` cada 5 s hacia `resource_samples` y agregado por minuto de lo que pasa de 24 h (cada 120 muestras).
- **Archivos clave:** `crates/scheduler/src/classify.rs`, `crates/daemon/src/resources.rs`, `crates/store/src/resources.rs`, `crates/daemon/src/recorder.rs`.
- **Cómo se verificó:** `cargo xtask check` → 474 passed. Tabla de casos del PLAN (`npm test` → 3, `pnpm build` → 4, `rg` → 1) y más de 30 comandos; proptest: cualquier texto cae en 0–4 y añadir un segmento nunca baja la clase; test de compactación (idempotente, promedio de CPU, máximo de slots); test del daemon con muestras reales de CPU/RAM.
- **Cambio de comportamiento:** `git status` y similares pasan de clase 2 a 1 (lectura); se ajustó el test `session_mirrors_conversation_and_tool_calls`.
- **Pendiente / notas:** `heavy_slots_used` queda en 0 y el total sale del perfil (1, o 3 en `performance`) hasta que S3 traiga el semáforo; `process_count` espera a S4 (árbol de procesos). El primer refresco de CPU de `sysinfo` tarda cientos de ms en Windows (el test espera por sondeo, no por tiempo fijo).

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| PLAN P08.S1 | «Migración 002» | Es la **004** | 002 (salud) y 003 (contexto) se ejecutaron antes por ADR-0010 |
| DB §3.C `agent_runs.end_reason` | 8 valores | 9: se agrega `PHASE_DONE` | ADR-0013 D2 |

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|
| `symphony-scheduler` (crate propio) | — | Clasificador y, desde S3, cola y slots | Sí (STACK §8, crate `scheduler`) |
| `sysinfo` en el daemon | 0.39.6 (ya en el workspace) | Muestreo de CPU y RAM | Sí |

## Notas para el siguiente agente
- Reconstruir una tabla padre: patrón `defer_foreign_keys` + `_bak` + `DROP` + reinsertar con su nombre (002, 003, 004). No uses `RENAME`.
- Siguiente: P08.S3 (scheduler). Antes de S3, medir F12 (tokens por proveedor) para que la concurrencia por cuota no sea ciega.
