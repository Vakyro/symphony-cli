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

### P03.S2 · Store writer — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `Writer` = hilo `symphony-store-writer` dueño de la única conexión de escritura; canal tokio acotado (4096, backpressure con `send().await`); comandos `Event` (en lotes de hasta 50 por transacción), `Write` (closure en transacción propia, con rollback si falla), `Flush` (devuelve `WriterStats`) y `Shutdown` (escribe lo encolado, `PRAGMA optimize` y termina; los handles que quedan reciben `WriterClosed`). Si un lote falla, se reintenta evento por evento. `open_reader()` abre conexiones de solo lectura con `query_only`.
- **Archivos clave:** `crates/store/src/writer.rs`, `crates/store/tests/writer.rs`
- **Cómo se verificó:** 4 productores × 2 500 = **10 000 eventos en ~0.5 s**, 0 fallidos, **orden por productor conservado**, en lotes (menos lotes que eventos). Un lector consulta sin parar durante toda la escritura **sin ningún error** (0 `SQLITE_BUSY`). Un evento con FK rota no tira su lote (9/1). Closure con error → rollback y error al que llamó. `cargo xtask check` → 59 passed.

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|

## Decisiones tomadas
- **Lotes sin espera artificial:** DB §7 dice "cada ~100 ms o cada 50 eventos". El writer escribe **todo lo que haya en cola, hasta 50**, sin esperar. Con carga los lotes se llenan solos (se verificó); sin carga, la latencia baja de ~100 ms a casi 0. El objetivo de DB §7 (latencia baja con varios agentes) se cumple mejor.

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
