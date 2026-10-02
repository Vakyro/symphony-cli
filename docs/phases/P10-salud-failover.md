# P10 · Salud, failover y routing

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p10-salud-failover |
| Inicio / cierre | 2026-10-01 / — |
| Agentes que trabajaron | claude-code/sonnet-5.5 |
| Tag | p10-done, v0.5.0 (pendientes) |
| Docs usados | IDEA §5.7, §5.8; STACK §19, §20; DB §3.D, §3.E, §6; FLOW §8, §12, §13; ADR-0010 |

## Diseño (antes de escribir código, ADR-0010 §3)

**Qué se construye.** (1) Tablas de salud, uso y routing (migración `002_health.sql`; la numeración sigue el orden de ejecución, no el del PLAN: 002 = P10, 003 = P09, 004 = P08). (2) Una máquina de estados de salud **pura** en `core::health`: dado el estado anterior y un evento (fallo clasificado, éxito, instantánea de cuota, paso del tiempo) devuelve el estado siguiente; no hay temporizadores: los cooldowns expirados se resuelven al leer (`effective_state`, p. ej. `RATE_LIMITED` → `PROBING`). (3) Un crate `symphony-router` **puro** (sin E/S): filtro de disponibilidad + scoring por pesos del profile + explicación generada desde los factores. (4) El daemon arma los candidatos desde la base, guarda `routing_decisions`/`routing_candidates` y reemplaza `repo::next_executor` en el failover. (5) Model Picker y Explain Route en la TUI.

**Reglas que no se discuten (IDEA §5.7–§5.8).** Un 429 no es «agotado». La cuota se muestra `KNOWN`/`ESTIMATED`/`UNKNOWN` y el porcentaje solo existe si el CLI lo informa (CHECK en la tabla). Un modelo exacto elegido por el usuario nunca se sobrescribe salvo que no esté disponible. Los profiles automáticos no gastan la reserva (`reserve = 0.20`); el usuario sí puede usarla a mano. Sin LLM en el camino crítico.

**Cuentas.** Una cuenta `default` por proveedor (`acct-<proveedor>`), creada al detectarlo; sin gestión de login (PLAN §2.9).

**Profiles.** Los siete de DB/FLOW (`@code`, `@debug`, `@fast`, `@reasoning`, `@docs`, `@review`, `@conserve`) con pesos iniciales editables en `profiles.weights_json` (claves `fit`, `context`, `health`, `quota`, `scarcity`, `failures`, `load`, `speed`) y un `base_score` por modelo en `profile_models` que se siembra al detectar los modelos.

**Cómo se prueba.** Proptest sobre la máquina de salud y sobre el router («un modelo no elegible nunca gana», «un 429 nunca produce `EXHAUSTED`», decisión independiente del orden de entrada), fixtures de fallos de cada adapter, Journey C con cada política de failover sobre `fake-agent`, y un benchmark del router.

## Pasos

### P10.S1 · Migración 002 — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-01
- **Qué se hizo:** `migrations/002_health.sql` con `provider_accounts`, `provider_health`, `usage_records`, `profiles` (7 sembrados), `profile_models`, `routing_decisions` y `routing_candidates`; las FKs que 001 dejó pendientes se añaden reconstruyendo `agents`, `agent_runs` y `provider_failures` dentro de la transacción (`PRAGMA defer_foreign_keys`, copia a `*_bak`, DROP, CREATE, INSERT). Las filas antiguas quedan con la cuenta `default`.
- **Archivos clave:** `migrations/002_health.sql`, `crates/store/src/{lib.rs,tests.rs}`
- **Cómo se verificó:** `cargo nextest run -p symphony-store` (23 passed). Test `migration_002_keeps_v01_data_and_adds_the_pending_foreign_keys`: una base de v0.1 con agentes, runs, fallo, cambio de executor y mensaje migra sin pérdida, `foreign_key_check` queda en 0, las FKs nuevas existen y la integridad referencial y los índices únicos parciales siguen activos. Además se migró una **copia de la base real de Leo** (4 agentes, 7 runs, 26 mensajes, 212 eventos): versión 2, mismos recuentos, 0 violaciones de FK.
- **Pendiente / notas:** ninguna.

### P10.S2 · Máquina de estados de salud — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-02
- **Qué se hizo:** `core::health` (función pura `Health::apply` + `effective_state`, cooldowns como instantes, `PROBING` al vencer); enums nuevos (`AccountAuthStatus`, `UsageSource`, `SpeedClass`, `RoutingTrigger`, `RejectReason`) comparados con la DB spec; `store::health` (salud por proveedor y por modelo, fallos recientes, ventanas de cuota desde los eventos, uso); `daemon::health` que aplica la máquina desde el recorder (errores no fatales, `Quota`, `TurnUsage`) y desde el executor (fallo fatal en la misma escritura que cierra el run, éxito al terminar bien). `parse_retry_after_ms` en `adapters/common` y los cinco adapters lo usan; los cinco ya registran `retry_after_ms`. `[providers.limits.<id>]` en `config.toml` (reserva y presupuesto por proveedor). `fake-agent` gana el paso `quota`.
- **Archivos clave:** `crates/core/src/health.rs`, `crates/store/src/health.rs`, `crates/daemon/src/health.rs`, `crates/daemon/src/recorder.rs`, `crates/adapters/common/src/lib.rs`
- **Cómo se verificó:** 14 tests de la máquina (4 proptest: un 429 nunca produce `EXHAUSTED`, el porcentaje solo con certeza `KNOWN`, cooldowns siempre en el futuro y se vuelven `PROBING`, `AUTH_ERROR` no caduca); 6 de store; 4 L2 en el daemon (`a_429_rate_limits…`, `exhausted_quota_marks…` con su hora de reinicio en ms, cuota conocida → `QUOTA_LOW` con 10 % real, uso `REPORTED` frente a `ESTIMATED`); 4 de propiedades de los parsers de los cinco adapters (nunca entran en pánico, mensajes redactados, un 429 nunca es cuota agotada, el `retry_after` se lee del texto). Sustituyen a los fuzz targets del PLAN (corren en CI sin nightly).
- **Pendiente / notas:** los textos de cuota/auth de Kimi, Antigravity y Copilot siguen siendo sintéticos. Dos cambios de infraestructura de tests: `START_TIMEOUT` del cliente y de los tests de daemon de 10 s a 30 s (con cinco CLIs que detectar, `copilot --version` tarda ~2 s y bajo carga el daemon pasaba de 10 s), y `pinned_bin` copia una sola vez por versión con un archivo de bloqueo.

### P10.S3 · Cuota y uso — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-02
- **Qué se hizo:** `HealthEvent::Estimate` (cuota `ESTIMATED` a partir de un presupuesto opcional por proveedor: `[providers.limits.<id>] window_hours` + `window_tokens`): nunca es un porcentaje, nunca agota y nunca pisa una cuota `KNOWN`. `daemon::health::on_run_finished` registra el uso (estimado si el CLI no informó tokens) y refresca la estimación al terminar un run. `providers.list` devuelve por proveedor su salud vigente (estado, certeza, restante solo si `KNOWN`, evidencia, reintento/reinicio, reserva), agentes activos, modelos, último fallo y uso de 7 días; método nuevo `usage.get`. TUI: la pantalla de Proveedores muestra SALUD y CUOTA y un detalle con lo que pide FLOW §12.1; CLI: `symphony providers` con las dos columnas y `symphony usage [--days N]` (informado frente a estimado).
- **Archivos clave:** `crates/core/src/health.rs`, `crates/daemon/src/{health,providers,server}.rs`, `crates/tui/src/ui.rs`, `crates/cli/src/main.rs`
- **Cómo se verificó:** `cargo xtask check` → 338 passed. Tests de la máquina (`Estimate` nunca agota ni pisa `KNOWN`, ahora dentro de las propiedades), L2 `a_token_budget_in_the_config_gives_an_estimated_quota_never_a_percentage`, snapshots de la TUI (cuota conocida con %, estimada sin %, desconocida; límite temporal con «reintenta en 45 s»), CLI `usage` y `providers`.
- **Pendiente / notas:** la estimación suma tokens de entrada + salida de los runs en la ventana (las ventanas reales de cada plan son más finas); por eso solo se activa si el usuario da un presupuesto.

### P10.S4 · Router determinista — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-02
- **Qué se hizo:** crate `symphony-router` (puro: sin E/S ni LLM). Filtro de disponibilidad con los ocho motivos de la DB (`DISABLED`, `OFFLINE`, `AUTH`, `EXHAUSTED`, `COOLDOWN`, `CAPABILITY`, `CONTEXT`, `RESERVE`) y puntaje = suma de ocho factores ponderados por el profile (`fit`, `context`, `health`, `quota`, `scarcity`, `failures`, `load`, `speed`). Reserva: un profile automático nunca gasta una cuota `KNOWN` dentro de la reserva (ni una estimada ya marcada `QUOTA_LOW`); un `QUOTA_LOW` sin cifra (aviso del proveedor) no descarta, solo penaliza; una elección manual sí puede gastarla. Explicación generada desde los factores (formato de FLOW §8.4, en español). Desempate por id: la decisión no depende del orden de entrada.
- **Archivos clave:** `crates/router/src/{lib,tests}.rs`, `crates/router/benches/router.rs`
- **Cómo se verificó:** 13 tests, 4 proptest (un modelo no elegible nunca gana y el elegido tiene el máximo puntaje; independiente del orden; un profile automático nunca gasta una reserva informada; la explicación siempre nombra al elegido), el ejemplo de FLOW §8.4 como test dorado. **Benchmark** (`cargo bench -p symphony-router`): 5 candidatos ≈ 8 µs, 20 candidatos ≈ 23 µs.
- **Pendiente / notas:** los puntajes iniciales por modelo (`routing::traits`) son una heurística por familia (Opus/Sol/Pro altos en razonamiento, Haiku/Luna/Flash en velocidad y conservación): editables en `profile_models`, no verdades.

### P10.S5 · Failover completo y Model Picker — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-02
- **Qué se hizo:** el router reemplaza a `repo::next_executor` (eliminada). **Spawn por profile** (`Execution::Profile`, antes «llega en P10»): el agente guarda solo el profile y el modelo vive en el run (AGENT ≠ MODEL). **Failover:** primero se escribe el fallo y la salud, luego el router elige con la política (`NONE` no cambia; `SAME_PROVIDER` solo ese proveedor; `ANY` todos) y se excluye lo que el agente ya agotó o donde le rechazaron el login; sin reemplazo, `WAITING_PROVIDER` y recovery. **Cada decisión se guarda** (`routing_decisions` + `routing_candidates` con motivo y factores), atada al run; el spawn exacto y el cambio manual también dejan su decisión («Modelo exacto elegido por el usuario… no se sustituye en silencio»). Cambio por profile (`agent.switch {profile}`). IPC: `profiles.list`, `route.explain`, `models.list` con salud. TUI: Model Picker con PROFILES y MODELOS EXACTOS con su estado de salud, Nuevo agente con tres modos (exacto / profile / decidir después), vista **Explain Route** (tecla `?` o `:explain-route`). CLI: `symphony explain-route <agente>` y `symphony switch --profile`.
- **Archivos clave:** `crates/daemon/src/{routing,executor,runtime,server}.rs`, `crates/tui/src/{app,ui}.rs`, `crates/cli/src/main.rs`
- **Cómo se verificó:** `cargo xtask check` → 365 passed. **Journey C con cada política** sobre `fake-agent`: `failover_any_goes_to_the_best_other_provider_and_records_why`, `failover_none_leaves_the_agent_waiting…`, `failover_same_provider_does_not_leave_a_provider_that_is_exhausted`; reserva (`an_automatic_failover_never_spends_the_reserve_of_another_provider`); spawn por profile que salta un proveedor agotado; un profile sin candidatos explica por qué y conserva la tarea; un modelo exacto se obedece aun con cuota baja; el cambio manual y el cambio por profile quedan registrados; 5 tests nuevos de la TUI con snapshots (picker, Explain Route) y el flujo de creación por profile.
- **Pendiente / notas:** el failover usa el profile del agente o `routing.default_profile`; `needed_context` (tamaño del handoff) aún no se pasa al router; «checkpoint obsoleto» (FLOW §13.3) no se avisa todavía.

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|
| Migración 001 → 002 sobre datos reales sin pérdida | test + copia de la base real | ✅ |
| La salud sigue a los runs: 429 → `RATE_LIMITED`, cuota → `EXHAUSTED` con reinicio, cuota conocida → `QUOTA_LOW` | L2 en el daemon + proptest | ✅ |
| Uso informado frente a estimado | L2 | ✅ |
| El router decide en ~8–23 µs y un modelo no elegible nunca gana | proptest + bench | ✅ |
| Journey C con `NONE`, `SAME_PROVIDER` y `ANY`; reserva respetada; spawn por profile | L2 en el daemon | ✅ |

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|

## Decisiones tomadas
- ADR-0010: P10 → P09 → P08 completas.

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| PLAN P10.S1 | «Migración 004» | `002_health.sql` | la numeración sigue el orden de ejecución (ADR-0010): P10 va antes que P08 y P09 |
| DB §3.D `provider_health` | `UNIQUE(provider_id, account_id, model_id)` | índice único con `COALESCE` | los NULL son distintos en SQLite: no impedía dos filas de nivel proveedor |

## Dependencias agregadas
Ninguna.
