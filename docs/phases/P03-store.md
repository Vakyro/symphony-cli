# P03 · Persistencia

| Campo | Valor |
|---|---|
| Estado | CERRADA |
| Rama | phase/p03-store |
| Inicio / cierre | 2026-09-24 / 2026-09-24 |
| Agentes que trabajaron | claude-code/opus-5.5 |
| Tag | p03-done |
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

### P03.S3 · Repositorios — ✅ (núcleo del ciclo de vida)
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `symphony_store::repo` con SQL a mano (sin ORM) para projects, sessions, tasks, agents, worktrees, providers, models y agent_runs. Lectura de enums e IDs de `core` con `FromStr` (`col`/`opt_col`). `set_task_status` y `set_agent_state` validan la transición con `core` y dejan el estado igual si es inválida; los estados de espera exigen `state_reason` (FLOW §7). `open_run`/`close_run`/`current_run`: el modelo actual sale del run abierto (AGENT ≠ MODEL). `RepoError` se convierte a `rusqlite::Error` para usarlo dentro de `WriterHandle::write`.
- **Archivos clave:** `crates/store/src/repo.rs`, `crates/store/tests/repo.rs`
- **Cómo se verificó:** 8 tests de repositorio (uno por entidad + failover de run + uso vía writer); `cargo xtask check` → 67 passed.
- **Pendiente:** repos de `executor_changes`, `messages`, `provider_failures`, `tool_calls`, `checkpoints`, `checkpoint_refs`, `handoffs` y `recovery_items` se escriben en la fase que los consume (P05–P06; P07 para recovery). `blobs`/`context_objects` en P03.S4.

### P03.S4 · Crate `object-store` — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `symphony-object-store`: `put` (BLAKE3 del original → si ya existe, dedupe; si no, zstd nivel 3 a un temporal `.tmp-*` del mismo directorio, `fsync` y `persist_noclobber`) + fila en `blobs`; `get` descomprime y **verifica el hash**; valida que el hash sea hex de 64 caracteres (sin path traversal); `add_ref`/`release` (nunca baja de 0); `gc(grace)` borra blobs con `ref_count = 0`, archivos sin fila y temporales de crashes, todos más viejos que el margen.
- **Archivos clave:** `crates/object-store/src/lib.rs`, `crates/object-store/tests/object_store.rs`
- **Cómo se verificó:** 5 tests: roundtrip y ubicación `<h[0:2]>/<h>`; dedupe (dos puts iguales → 1 archivo y 1 fila); **crash a media escritura** (temporal con la mitad de los bytes → el blob no existe, un put posterior lo escribe completo y el GC borra el temporal); corrupción detectada (contenido cambiado, zstd inválido, hash con `../`); refcount y GC (margen de gracia, huérfanos). `cargo xtask check` → 72 passed.

### P03.S5 · Recuperación al arrancar — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** el daemon abre `~/.symphony/symphony.db` con el `Writer` después de tomar el lock de instancia y corre `repo::interrupt_orphan_sessions`: toda sesión `ACTIVE` pasa a `INTERRUPTED` con `ended_at`, y se abre un `recovery_items(SESSION_INTERRUPTED)` con una frase que nombra el pid muerto. `status` agrega `recovery_open`. Nuevos: `RecoveryItemId` en `core`, `SymphonyHome::db_path()` y `objects_dir()`.
- **Archivos clave:** `crates/daemon/src/server.rs`, `crates/store/src/repo.rs`, `crates/daemon/tests/daemon.rs`
- **Cómo se verificó:** test con el binario real: se deja una sesión `ACTIVE` de un daemon muerto (pid 999999) → `symphonyd` arranca → `status.recovery_open == 1`, la sesión queda `INTERRUPTED`, el recovery item nombra el pid → un segundo arranque no duplica. `cargo xtask check` → 73 passed.
- **Decisión:** no se revisa si el pid sigue vivo. Con el lock de instancia tomado, ningún daemon anterior del mismo home puede estar vivo; además, los pids se reusan.

### P03.S6 · Benchmarks — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `crates/store/benches/store.rs` (Criterion). Inserción de eventos de punta a punta por el `Writer` (spawn del hilo, envío por canal, lotes, `flush`) y la consulta de Home (`repo::home_rows`, nueva) con 50 agentes vivos, cada uno con su run abierto, y 100k eventos en la base.
- **Cómo se verificó:** `cargo bench -p symphony-store --bench store` en la laptop de Leo (i7-8650U, Windows 11, SSD).

### P03.S7 · Cierre — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** revisión del diff (sin `unwrap`/`expect`/`todo!` en runtime; SQL con parámetros; el object store valida el hash antes de construir rutas). Merge a `main` y tag `p03-done`.


## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|
| Migración 001 idéntica a DB (19 tablas, CHECKs, FKs, índices parciales) | `crates/store/src/tests.rs` | ✅ |
| Un solo escritor con concurrencia | 4 × 2 500 eventos, lector en paralelo | ✅ 0 errores, orden conservado |
| Object store atómico, con dedupe e integridad | `crates/object-store/tests` | ✅ |
| Recuperación de sesiones al arrancar | test con el binario real | ✅ |
| AGENT ≠ MODEL en el esquema y en los repos | tests de esquema y de failover de runs | ✅ |

### Arreglo: carrera stop → autoarranque (P03.S5)
- La CI de macOS mostró que `daemon stop` daba por detenido al daemon cuando desaparecía su socket, pero el proceso seguía cerrando la base con el lock tomado; el siguiente autoarranque fallaba con "ya hay un daemon". Ahora `stop` espera a que se libere el **lock de instancia** y el autoarranque (`ensure_running`) espera a que el socket conteste o el lock quede libre antes de lanzar otro daemon.

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|

## Decisiones tomadas
- **Lotes sin espera artificial:** DB §7 dice "cada ~100 ms o cada 50 eventos". El writer escribe **todo lo que haya en cola, hasta 50**, sin esperar. Con carga los lotes se llenan solos (se verificó); sin carga, la latencia baja de ~100 ms a casi 0. El objetivo de DB §7 (latencia baja con varios agentes) se cumple mejor.

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| PLAN P03.S3 | Repos para "las entidades de Fase 1" | Solo el núcleo del ciclo de vida (8 de 19 tablas) | YAGNI: el resto se escribe con sus consumidores, así su API sale de un uso real |
| DB §3 | FKs a `milestones`, `profiles`, `provider_accounts`, `routing_decisions` | Columnas nullable **sin FK** en 001 | Esas tablas son de Fase 2 y 4; su migración agrega la FK (lo pide PLAN P03.S1) |
| DB §1 (convenciones) | Booleanos `INTEGER 0/1`, confianza de 0 a 1 | `CHECK (x IN (0,1))` y `CHECK (confidence BETWEEN 0 AND 1)` | Hace cumplir la convención en la base |
| DB §1 | Tipos `TEXT`, `INTEGER`, `REAL` | Tablas `STRICT` | SQLite rechaza tipos equivocados en lugar de convertirlos en silencio |
| DB §3 | Índices listados | Se agregaron índices de FK usados por las consultas de DB §5 (sessions, tasks, agents, executor_changes, messages, models, provider_failures, tool_calls.run_id, context_objects.blob_hash, recovery_items abiertos) | DB §5 pide que esas consultas sean rápidas |

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|
| rusqlite | 0.40.2 (`bundled`) | SQLite | Sí |
| rusqlite_migration | 2.6 | Migraciones con `user_version` | Sí |
| blake3 | 1.8 | Hash de contenido | Sí |
| zstd | 0.14 | Compresión de blobs | Sí |

## Métricas
Línea base P03 (laptop de Leo, `cargo bench -p symphony-store --bench store`):

| Benchmark | Tiempo (intervalo) | Throughput |
|---|---|---|
| Insertar 10 000 eventos por el Writer | 166–181 ms | ~58 000 eventos/s |
| Insertar 100 000 eventos por el Writer | 1.50–1.54 s | ~66 000 eventos/s |
| Home query, 50 agentes + 100k eventos | 94–97 µs | — |

DB §7 asumía unos 50 eventos cada 100 ms (≈500/s): hay más de 100× de margen.
(benchmarks, tiempos, RAM, cobertura — con comando)

## Pruebas
- Comando(s): `cargo xtask check`, `cargo deny check`, `cargo bench -p symphony-store --bench store`, CI en 3 OS
- Totales: 74 passed, 0 failed
- Totales: N passed, M failed (cuáles y por qué)

## Estado final
Persistencia lista: SQLite con las 19 tablas de Fase 1 copiadas de DB, un solo escritor en su propio hilo (~60k eventos/s), lectores de solo lectura, repositorios del ciclo de vida con transiciones validadas por `core`, object store BLAKE3 + zstd atómico con GC, y recuperación de sesiones interrumpidas al arrancar el daemon. De paso se arregló una carrera real entre `stop` y el autoarranque (el lock de instancia es la señal de "daemon terminado").
Resumen de 3–5 líneas para Leo.

## Notas para el siguiente agente
Trampas, comandos útiles y lo que no vale la pena reintentar.
