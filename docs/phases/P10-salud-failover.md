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

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|
| Migración 001 → 002 sobre datos reales sin pérdida | test + copia de la base real | ✅ |

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
