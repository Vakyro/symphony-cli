# P06 · Agent runtime, checkpoints y handoff

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p06-runtime |
| Inicio / cierre | 2026-09-24 / — |
| Agentes que trabajaron | claude-code/opus-5.5 (S1–S4) |
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

### P06.S2 · Mensajes y tool calls — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `symphony_daemon::recorder::Recorder`, dentro del `EventBus`, convierte cada evento canónico, venga de un hook o del stream, en filas de `messages` y `tool_calls`. Escribe por el writer único, después del evento y en el mismo orden.
  - **Mensajes:** `AssistantText` se guarda como `ASSISTANT` y `UserMessage` como `USER`. El runtime publica el prompt inicial como `UserMessage` (source `USER`). Los mensajes de más de 4 KiB van al object store: blob + `context_objects` (`CONVERSATION`, `ctx://message/<id>`) + `add_ref`, con `content` NULL.
  - **Tool calls:** `ToolRequested` crea la fila en `RUNNING`, con enforcement `NONE` y el comando redactado. `ToolFinished` la cierra como `DONE` o `FAILED`, con su exit code. Si llega un final sin su pedido, se registra la tool call entera.
  - **Deduplicación:** las tool calls se deduplican por `tool_use_id` (hook + stream); sin id, se emparejan por orden de llegada. El prompt repetido por el hook `UserPromptSubmit` no se duplica. El estado en memoria se suelta al terminar el run.
- **Archivos clave:** `crates/daemon/src/recorder.rs`, `crates/daemon/src/bus.rs`, `crates/store/src/repo.rs` (`insert_message`, `insert_context_object`, `insert_tool_call`, `finish_tool_call`), `crates/core/src/ids.rs` (`MessageId`, `ToolCallId`, `ContextObjectId`), `crates/daemon/tests/{runtime,recorder}.rs`
- **Cómo se verificó:**
  - `session_mirrors_conversation_and_tool_calls`: sesión fake-agent con un texto corto, un comando OK, una edición, un comando que falla y un texto de más de 4 KiB.
  - `duplicates_from_hook_and_stream_are_recorded_once`.
  - `cargo xtask check` → 144 passed, 1 skipped.
- **Pendiente / notas:**
  - `op_class` es provisional: comando = 2, edición u otra herramienta = 1. El clasificador real llega con el scheduler (P08).
  - `cwd` y `output_object_id` quedan NULL: la salida de las herramientas va al object store con el context engine (P09).
  - fake-agent ahora genera un `tool_use_id` único con un contador; antes, dos herramientas en el mismo milisegundo compartían id.

### P06.S3 · Checkpoints incrementales — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `symphony_daemon::checkpoint::Checkpointer`, dentro del `EventBus`.
  - **En el camino del bus** solo acumula en memoria, por agente: último mensaje del asistente, paso actual, siguiente paso, último comando, cantidad de comandos y los últimos 5 fallos. No toca git, así que un hook nunca espera.
  - **Qué dispara un checkpoint:** un archivo modificado, una herramienta terminada o un fin de turno.
  - **El worker** toma la foto de git del worktree: HEAD, `git diff <base>` redactado y archivos tocados (numstat + untracked). Después escribe una transacción con:
    - el blob del diff (reutiliza el `GIT_DIFF` si el diff no cambió);
    - el checkpoint con `plan_tail`, `current_step`, `next_step`, `head_commit`, `diff_object_id` y `summary_json`;
    - `checkpoint_refs(DIFF)`;
    - la poda: quedan los últimos 20 más los usados por handoffs, runs o executor changes. Los diffs que se quedan sin uso se borran y sueltan su ref del blob.
  - **Agrupación:** varios eventos seguidos del mismo agente producen un solo checkpoint (a lo sumo un trabajo pendiente por agente).
  - **`next_step`** es determinista: el primer `- [ ]` o una línea `Next:`/`Siguiente:` del último mensaje. Si no hay, se conserva el anterior (LEARNINGS H1).
  - **`plan_tail`** es la cola (2000 caracteres) del último mensaje del asistente, redactada.
- **Archivos clave:** `crates/daemon/src/checkpoint.rs`, `crates/daemon/src/bus.rs`, `crates/store/src/repo.rs` (`checkpoint_base`, `diff_object`, `prune_checkpoints`, `insert_checkpoint` con refs), `crates/daemon/tests/runtime.rs`
- **Cómo se verificó:**
  - `checkpoints_stay_monotonic_and_consistent` (proptest, 4 casos × 50 eventos, un checkpoint por evento significativo). Comprueba:
    - `seq` monótonos y cantidad exacta tras la poda;
    - el checkpoint del handoff se conserva;
    - ninguna referencia a objetos o blobs inexistentes, ningún diff huérfano;
    - `ref_count` de los blobs coherente;
    - el último diff igual al diff real del worktree.
  - `session_leaves_an_up_to_date_checkpoint`: sesión fake-agent con plan, edición, archivo nuevo y comando que falla.
  - 3 tests unitarios.
  - `cargo xtask check` → 149 passed, 1 skipped.
- **Pendiente / notas:**
  - `trigger_event_id` queda NULL: los eventos se escriben en lote y el bus no conoce su id.
  - El conteo de tests en `summary_json` llega con el parser de salidas (P09).
  - Al reiniciar el daemon se pierde el acumulado en memoria. El último checkpoint guardado sigue siendo válido para el handoff.

### P06.S4 · Handoff v1 — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:**
  - **Crate nuevo `symphony-context`** (STACK §4.1) con `handoff::assemble`. Es una función pura, sin E/S ni LLM, que arma el prompt de arranque con una plantilla fija.
    - **Secciones:** motivo del cambio, objetivo, último plan, en qué estaba, qué seguía, último comando y resultado, fallos recientes, archivos tocados, `git status`, diff contra la base y archivos nuevos.
    - **Límites por modo** (`agents.context_mode`): `RAW` no recorta nada (salida de emergencia). `SAFE`, `BALANCED` y `AGGRESSIVE` recortan el diff y los archivos nuevos, con un presupuesto total, y avisan cuántos caracteres se omitieron y dónde está el original.
    - **`estimate_tokens`:** ~4 caracteres por token, determinista.
  - **`symphony_daemon::handoff::prepare`** (`Runtime::prepare_handoff`) junta el último checkpoint válido con el **git vivo** del worktree (ADR-0004, H2: git manda): status, diff redactado, numstat y archivos nuevos redactados de hasta 1 MiB y UTF-8.
    - `tokens_raw_estimate` = el prompt sin recortes + toda la conversación del agente.
    - También mide `build_ms`.
  - **Tabla `handoffs`:** el primer spawn ya deja su fila (`checkpoint_id` NULL, prompt = objetivo). Hay `insert_handoff` y `set_handoff_outcome` para P06.S5.
- **Archivos clave:** `crates/context/src/handoff.rs` (+ `snapshots/`), `crates/daemon/src/handoff.rs`, `crates/store/src/repo.rs` (`latest_checkpoint`, `conversation_chars`, `insert_handoff`, `set_handoff_outcome`), `crates/core/src/ids.rs` (`HandoffId`)
- **Cómo se verificó:**
  - Snapshot `insta` del prompt para un checkpoint fijo, y otro para un checkpoint sin progreso.
  - Tests de recorte por modo, presupuesto de archivos nuevos y tokens.
  - `handoff_combines_the_checkpoint_with_live_git`: sesión fake-agent más un archivo escrito después del último checkpoint, que igual aparece en el prompt.
  - `cargo xtask check` → 155 passed, 1 skipped. `cargo deny check` → ok.
- **Pendiente / notas:**
  - `outcome` se llena al cambiar de executor (P06.S5, P06.S8).
  - Si el diff vivo se recorta, el puntero apunta al diff del último checkpoint, no al vivo.
  - Niveles L0–L5 de archivos y decisiones vigentes llegan con el context engine (P09).

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|
| Crear agente con modelo exacto y correr el executor en su worktree | `exact_model_creates_everything_and_runs_the_executor` | ✅ |
| Nunca queda un agente "medio roto" | `workspace_failure_…`, `store_failure_after_the_worktree_rolls_it_back` | ✅ |
| Razón humana en `FAILED` + Recovery | `executor_that_cannot_start_…`, `executor_crash_…` | ✅ |
| Conversación y tool calls espejadas, sin duplicados, con secretos redactados | `session_mirrors_…`, `duplicates_from_hook_and_stream_…` | ✅ |
| Checkpoints incrementales monótonos, podados, sin objetos colgantes | proptest `checkpoints_stay_monotonic_and_consistent` | ✅ |
| Prompt de handoff con plantilla fija, desde checkpoint + git vivo | snapshots `insta` + `handoff_combines_the_checkpoint_with_live_git` | ✅ |

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
| insta (dev) | 1.48.0 | Snapshots del prompt de handoff (P06.S4) | Sí: STACK §24.3 y §4 (tests) |

## Métricas
- Crear un agente con worktree y lanzar fake-agent: < 2 s en Windows debug (tests).

## Pruebas
- Comando(s): `cargo xtask check`
- Totales: 155 passed, 1 skipped (live, sin `SYMPHONY_LIVE`)

## Estado final
(al cerrar)

## Notas para el siguiente agente
- `Runtime` recibe los adapters inyectados. Los tests usan `FakeAdapter` sobre fake-agent y nunca un CLI real.
- `Runtime::wait_executors` espera a que terminen los pumps. Úsalo en los tests antes de mirar la base, y haz `flush` del writer.
