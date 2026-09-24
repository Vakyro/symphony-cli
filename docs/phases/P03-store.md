# P03 · Persistencia

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p03-store |
| Inicio / cierre | 2026-09-24 / — |
| Agentes que trabajaron | claude-code/opus-5.5 |
| Tag | — |
| Docs usados | DB §1, §3 (A, B, C, D, F, G, I), §5, §6, §7; STACK §9, §10 |

## Pasos

### P03.S1 · Crate `store` y migración 001 — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `migrations/001_core.sql` con las 19 tablas de Fase 1, copiadas de DB §3: tipos, `NOT NULL`, `CHECK` de enums, FKs, `UNIQUE` e índices parciales (un agente activo por task, un run abierto por agente, cola de `tool_calls`). Crate `symphony-store`: `open()` (PRAGMAs de DB §7 + WAL verificado + migraciones con `rusqlite_migration` y `foreign_key_check`) y `open_in_memory()` para tests.
- **Archivos clave:** `migrations/001_core.sql`, `crates/store/src/{lib,tests}.rs`
- **Cómo se verificó:** `cargo xtask check` → 56 passed (9 del store): esquema exacto de 19 tablas, `user_version = 1`, PRAGMAs, `foreign_key_check` vacío, reapertura idempotente, índices parciales que rechazan duplicados, FKs activas, CHECKs de enums iguales a los enums de `core`, `EXACT`/`PROFILE` exigen su destino, `json_valid`, y **`agents` sin columna de modelo** (CONSTRAINTS A1).

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|

## Decisiones tomadas
- ADR-NNNN: …

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| DB §3 | FKs a `milestones`, `profiles`, `provider_accounts`, `routing_decisions` | Columnas nullable **sin FK** en 001 | Esas tablas son de Fase 2 y 4; su migración agrega la FK (lo pide PLAN P03.S1) |
| DB §1 (convenciones) | Booleanos `INTEGER 0/1`, confianza de 0 a 1 | `CHECK (x IN (0,1))` y `CHECK (confidence BETWEEN 0 AND 1)` | Hace cumplir la convención en la base |
| DB §1 | Tipos `TEXT`, `INTEGER`, `REAL` | Tablas `STRICT` | SQLite rechaza tipos equivocados en lugar de convertirlos en silencio |
| DB §3 | Índices listados | Se agregaron índices de FK usados por las consultas de DB §5 (sessions, tasks, agents, executor_changes, messages, models, provider_failures, tool_calls.run_id, context_objects.blob_hash, recovery_items abiertos) | DB §5 pide que esas consultas sean rápidas |

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|
| rusqlite | 0.40.2 (`bundled`) | SQLite | Sí |
| rusqlite_migration | 2.6 | Migraciones con `user_version` | Sí |

## Métricas
(benchmarks, tiempos, RAM, cobertura — con comando)

## Pruebas
- Comando(s): …
- Totales: N passed, M failed (cuáles y por qué)

## Estado final
Resumen de 3–5 líneas para Leo.

## Notas para el siguiente agente
Trampas, comandos útiles y lo que no vale la pena reintentar.
