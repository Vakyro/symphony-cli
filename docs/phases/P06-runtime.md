# P06 · Agent runtime, checkpoints y handoff

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p06-runtime |
| Inicio / cierre | 2026-09-24 / — |
| Agentes que trabajaron | claude-code/opus-5.5 (S1) |
| Tag | — |
| Docs usados | FLOW §6, §7, §16; DB §3.C, §3.G, §3.I; IDEA §5.5 |

## Pasos

### P06.S1 · Ciclo de vida del agente — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `symphony_daemon::runtime::Runtime::create_agent`, que implementa el flujo de FLOW §6:
  1. **Executor elegible.** Con modelo exacto se verifica que el modelo y el proveedor existan, estén habilitados y el proveedor esté `READY`, y que haya un adapter. Con «decidir después» basta con que haya al menos un proveedor listo.
  2. **Worktree.** Se crea con `git worktree add -b symphony/<sesión>/agent-NNN` en `~/.symphony/worktrees/<proyecto>/agent-NNN`. De dependencias solo se aplica `LINK`.
  3. **Una sola transacción** con project, session, task, worktree, agente, checkpoint inicial (seq 1, objetivo, commit base) y run.
  4. **Lanzamiento del CLI** vía adapter con el modelo exacto. La salida del CLI se lee y cada línea va al bus como evento `JSON_STREAM`.
  5. **Cierre del run.** Con exit 0 → run `EXITED/COMPLETED`, agente `COMPLETED`, task `DONE`. Si el CLI termina con error → run `FAILED/CRASH`, agente `FAILED` con razón humana y recovery item `EXECUTOR_EXITED`.
- **Ramas de error:**
  - Tarea vacía.
  - Profile (llega en P10).
  - Directorio que no es repo git.
  - Modelo exacto no disponible: el error ofrece esperar, escoger otro modelo, usar un profile o cancelar, y devuelve la tarea escrita.
  - Ningún proveedor listo: se devuelve la tarea escrita.
  - Falla el workspace: nada en la base y el worktree se deshace.
  - Falla la base después del worktree: se deshacen el worktree y la rama.
  - El CLI no arranca: el agente queda `FAILED` con razón y el workspace intacto.
- **Archivos clave:** `crates/daemon/src/runtime.rs`, `crates/store/src/repo.rs` (`active_session`, `eligible_model`, `ready_providers`, `insert_checkpoint`, `open_recovery_item`), `crates/daemon/tests/runtime.rs`
- **Cómo se verificó:** `cargo nextest run -p symphony-daemon --test runtime` → 9 passed. `cargo xtask check` → 142 passed, 1 skipped.
- **Pendiente / notas:**
  - Falta exponerlo por IPC (`agent.create`) y el comando `symphony spawn` (P06.S7).
  - El hook inyectado es `None` en los tests. El daemon real va a pasar `symphony hook emit`.

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|
| Crear agente con modelo exacto y correr el executor en su worktree | `exact_model_creates_everything_and_runs_the_executor` | ✅ |
| Nunca queda un agente "medio roto" | `workspace_failure_…`, `store_failure_after_the_worktree_rolls_it_back` | ✅ |
| Razón humana en `FAILED` + Recovery | `executor_that_cannot_start_…`, `executor_crash_…` | ✅ |

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|
| `INSTALL` / `PNPM_STORE` no se ejecutan al crear el worktree | Un proyecto node sin `node_modules` en la base arranca sin dependencias | crear agente en repo npm sin instalar | Son operaciones pesadas: las corre el scheduler (P08) |

## Decisiones tomadas
- **Orden git → transacción.** El worktree se crea antes de escribir en la base. Así el rollback se reduce a borrar el worktree y la rama; no hay filas `CREATING` que limpiar.
- **Exit 0 del CLI = tarea terminada** (agente `COMPLETED`). P06.S5 y P06.S6 afinan esto con failover y heartbeat.
- **Rama con los últimos 8 caracteres de la sesión.** Se usa `symphony/<sesión>/agent-NNN` recortando el ULID de la sesión a sus últimos 8 caracteres (en minúsculas), para que el nombre sea corto.

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| FLOW §6 «Modelo exacto no disponible» | Mostrar la decisión al usuario | El runtime devuelve un error con las 4 opciones y la tarea escrita. La decisión la muestra la CLI/TUI | El runtime no tiene UI |

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|
| (ninguna nueva; daemon ahora usa `symphony-git`, `symphony-process` y, en tests, `symphony-testkit`) | | | |

## Métricas
- Crear un agente con worktree y lanzar fake-agent: < 2 s en Windows debug (tests).

## Pruebas
- Comando(s): `cargo xtask check`
- Totales: 142 passed, 1 skipped (live, sin `SYMPHONY_LIVE`)

## Estado final
(al cerrar)

## Notas para el siguiente agente
- `Runtime` recibe los adapters inyectados. Los tests usan `FakeAdapter` sobre fake-agent y nunca un CLI real.
- `Runtime::wait_executors` espera a que terminen los pumps. Úsalo en los tests antes de mirar la base, y haz `flush` del writer.
