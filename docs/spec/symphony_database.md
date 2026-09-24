# Symphony CLI: Base de datos

> Modelo de datos de Symphony, derivado de `idea.md` y de la especificación de flujo y vistas (`Symphony_CLI_User_Flow_and_Views.html`). Cada tabla existe porque alguna vista o flujo la necesita.

**Motor:** SQLite embebido en modo WAL, en un solo archivo: `~/.symphony/state.db`
**Estado:** Borrador v0. Las tablas se van activando por fase del roadmap.

---

## 1. Decisiones de diseño

**¿Por qué SQLite?**
Es local, sin servidor, transaccional y sobrevive a crashes. Trae FTS5 para búsqueda de texto. Encaja con las prioridades: ligero, rápido y fiable.

**Un solo escritor.**
Solo `symphonyd` escribe en la base. Los hooks de los CLIs **no** abren la DB: mandan sus eventos al daemon por IPC y el daemon los escribe en lote. Así se evitan bloqueos y corrupción con varios agentes a la vez.

**Qué no va en la base:**

| Dato | Dónde vive | Por qué |
|---|---|---|
| Código fuente | Git | Git ya es la fuente de verdad del código |
| Configuración (performance, routing, failover, context mode) | `config.toml` / `project.toml` | Es editable a mano, se versiona y es legible |
| Logs y outputs grandes | Object store en `~/.symphony/objects/` (BLAKE3 + zstd) | La DB solo guarda el hash y la metadata |
| Credenciales, tokens, cookies | Nunca en Symphony | Cada CLI oficial guarda las suyas |

**Phase 0 no usa SQLite.** En el spike basta con `events.jsonl` + `checkpoint.json` por agente. La base de datos nace en Phase 1.

### Convenciones

| Aspecto | Regla |
|---|---|
| IDs | `TEXT` con ULID (ordenables por tiempo). Excepción: `events.id` es `INTEGER AUTOINCREMENT` por volumen |
| Fechas | `INTEGER`, Unix epoch en milisegundos UTC. Sufijo `_at` |
| Booleanos | `INTEGER` 0/1 |
| Enums | `TEXT` con `CHECK (col IN (...))` |
| JSON | `TEXT` validado con `json_valid()`. Solo para datos que no se consultan por campo |
| Decimales | `REAL` (porcentajes, scores y confianza de 0 a 1) |
| Borrado | Lógico con `archived_at` en entidades de usuario. Físico solo en retención de eventos y GC de blobs |
| Integridad | `PRAGMA foreign_keys = ON` siempre |

**Tipos SQLite usados:** `TEXT`, `INTEGER`, `REAL`. `BLOB` no se usa: los blobs van al object store.

---

## 2. Mapa de dominios

| # | Dominio | Tablas | Vistas que lo usan |
|---|---|---|---|
| A | Proyecto y sesión | `projects`, `sessions` | Launch, First-run Setup, Recovery |
| B | Trabajo | `tasks`, `task_dependencies`, `milestones` | Task List, Task Detail, Milestone Review |
| C | Agentes y ejecución | `agents`, `worktrees`, `agent_runs`, `executor_changes`, `messages` | Home, Agent Overview/Conversation/History, Failover Event |
| D | Proveedores y modelos | `providers`, `provider_accounts`, `models`, `provider_health`, `provider_failures`, `usage_records` | Providers, Provider Detail, Provider Setup, Usage |
| E | Routing | `profiles`, `profile_models`, `routing_decisions`, `routing_candidates`, `model_suggestions` | Model Picker, Explain Route, Model Suggestion |
| F | Eventos y scheduler | `events`, `tool_calls`, `processes`, `resource_samples` | Agent Activity, System/Resources, Event/Audit Log |
| G | Checkpoints y contexto | `checkpoints`, `checkpoint_refs`, `blobs`, `context_objects`, `context_chunks`, `context_fts`, `handoffs`, `handoff_items`, `context_retrievals`, `project_facts` | Agent Context, Context Overview, Context Object, Context Stats |
| H | Validación y merge | `validation_runs`, `validation_checks`, `merge_requests`, `merge_conflicts`, `reviews` | Validation, Changes, Merge Preflight, Conflict Review, Review Result |
| I | Recuperación | `recovery_items` | Recovery Center |
| J | Recursos compartidos | `skills`, `mcp_servers` | Shared Skills, Shared MCP/Tools |

Son 42 tablas en total, más la tabla virtual FTS5. **Phase 1 solo necesita 19**; la columna *Fase* de cada tabla indica cuándo entra.

---

## 3. Tablas

### A. Proyecto y sesión

#### `projects` · Fase 1
Un repositorio registrado en Symphony.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | ULID |
| name | TEXT NOT NULL | `arete-mobile` |
| root_path | TEXT NOT NULL UNIQUE | Ruta absoluta del repo |
| default_branch | TEXT NOT NULL | Rama base para integración (`main`) |
| created_at | INTEGER NOT NULL | |
| last_opened_at | INTEGER | |
| archived_at | INTEGER | |

#### `sessions` · Fase 1
Cada vez que abro Symphony en un proyecto. Sirve para detectar cierres inesperados (Journey E).

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| project_id | TEXT NOT NULL FK → projects | |
| status | TEXT NOT NULL | `ACTIVE` · `CLOSED` · `INTERRUPTED` |
| daemon_pid | INTEGER | Para reconciliar si el daemon murió |
| started_at | INTEGER NOT NULL | |
| ended_at | INTEGER | Si es NULL y el daemon no vive, la sesión quedó `INTERRUPTED` y se abre Recovery |

---

### B. Trabajo

#### `tasks` · Fase 1
Unidad concreta de trabajo.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| project_id | TEXT NOT NULL FK → projects | |
| milestone_id | TEXT FK → milestones | Opcional |
| code | TEXT NOT NULL | Código visible `T-12`; UNIQUE por proyecto |
| kind | TEXT NOT NULL | `WORK` · `REVIEW` (las tareas de review agent también son tasks) |
| title | TEXT NOT NULL | |
| description | TEXT | Instrucción original completa |
| status | TEXT NOT NULL | `BACKLOG` · `READY` · `RUNNING` · `WAITING` · `BLOCKED` · `DONE` · `FAILED` · `CANCELLED` |
| status_reason | TEXT | Frase humana: "espera a T-12 y T-16" |
| priority | INTEGER NOT NULL DEFAULT 0 | Mayor = más prioritaria |
| created_at | INTEGER NOT NULL | |
| updated_at | INTEGER NOT NULL | |
| completed_at | INTEGER | |

UNIQUE(`project_id`, `code`)

#### `task_dependencies` · Fase 2
Aristas del DAG. Una task no pasa a `READY` mientras tenga dependencias sin `DONE`.

| Columna | Tipo | Notas |
|---|---|---|
| task_id | TEXT NOT NULL FK → tasks | La que espera |
| depends_on_task_id | TEXT NOT NULL FK → tasks | La que bloquea |
| created_at | INTEGER NOT NULL | |

PK(`task_id`, `depends_on_task_id`) · CHECK(`task_id <> depends_on_task_id`). La detección de ciclos se hace en el daemon al insertar.

#### `milestones` · Fase 2
Agrupación opcional con gate de aprobación.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| project_id | TEXT NOT NULL FK → projects | |
| name | TEXT NOT NULL | "Authentication complete" |
| requires_approval | INTEGER NOT NULL DEFAULT 0 | |
| status | TEXT NOT NULL | `OPEN` · `IN_REVIEW` · `APPROVED` · `CHANGES_REQUESTED` · `HELD` |
| decided_at | INTEGER | |
| decision_note | TEXT | |

---

### C. Agentes y ejecución

#### `agents` · Fase 1
La entidad persistente. **No guarda el modelo actual.** El modelo es del run activo (AGENT ≠ MODEL).

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| project_id | TEXT NOT NULL FK → projects | |
| session_id | TEXT NOT NULL FK → sessions | Sesión en la que se creó |
| task_id | TEXT NOT NULL FK → tasks | |
| worktree_id | TEXT UNIQUE FK → worktrees | NULL para reviewers sin workspace propio |
| number | INTEGER NOT NULL | `#3` visible; UNIQUE por proyecto |
| state | TEXT NOT NULL | `CREATED` · `READY` · `RUNNING` · `WAITING_PROVIDER` · `WAITING_RESOURCE` · `WAITING_DEPENDENCY` · `TESTING` · `BLOCKED` · `PAUSED` · `COMPLETED` · `FAILED` · `CANCELLED` |
| state_reason | TEXT | Frase humana obligatoria en estados de espera o bloqueo |
| execution_mode | TEXT NOT NULL | `EXACT` · `PROFILE` · `DECIDE_LATER` |
| requested_model_id | TEXT FK → models | Si `EXACT` |
| requested_profile_id | TEXT FK → profiles | Si `PROFILE` |
| failover_policy | TEXT NOT NULL | `NONE` · `SAME_PROVIDER` · `ANY` |
| context_mode | TEXT NOT NULL | `RAW` · `SAFE` · `BALANCED` · `AGGRESSIVE` |
| priority | INTEGER NOT NULL DEFAULT 0 | |
| created_at | INTEGER NOT NULL | |
| updated_at | INTEGER NOT NULL | |
| archived_at | INTEGER | |

UNIQUE(`project_id`, `number`). CHECK: si `execution_mode='EXACT'`, `requested_model_id` no es NULL. Si es `'PROFILE'`, `requested_profile_id` no es NULL.
**Índice parcial:** solo un agente activo por task:
`UNIQUE(task_id) WHERE state NOT IN ('COMPLETED','FAILED','CANCELLED')`.

#### `worktrees` · Fase 1
Workspace Git aislado de un agente.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| project_id | TEXT NOT NULL FK → projects | |
| path | TEXT NOT NULL UNIQUE | `~/.symphony/worktrees/<proyecto>/agent-003` |
| branch | TEXT NOT NULL | `symphony/<sesion>/agent-003` |
| base_ref | TEXT NOT NULL | Commit o rama de origen |
| head_commit | TEXT | Último commit conocido |
| deps_strategy | TEXT NOT NULL | `PNPM_STORE` · `LINK` · `INSTALL` · `NONE` |
| deps_lock_hash | TEXT | Hash del lockfile, para saber si hay que reinstalar |
| status | TEXT NOT NULL | `CREATING` · `READY` · `DIRTY` · `DAMAGED` · `REMOVED` |
| created_at | INTEGER NOT NULL | |
| removed_at | INTEGER | |

#### `agent_runs` · Fase 1
Un **executor**: un periodo en el que un CLI y un modelo concretos ejecutaron al agente. Si hay failover, se cierra un run y se abre otro. El run activo es el que tiene `ended_at IS NULL`.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| agent_id | TEXT NOT NULL FK → agents | |
| seq | INTEGER NOT NULL | 1, 2, 3… por agente |
| provider_id | TEXT NOT NULL FK → providers | |
| account_id | TEXT FK → provider_accounts | NULL hasta Fase 4 |
| model_id | TEXT NOT NULL FK → models | |
| routing_decision_id | TEXT FK → routing_decisions | NULL si el modelo fue exacto |
| start_checkpoint_id | TEXT FK → checkpoints | Desde qué checkpoint arrancó (handoff) |
| cli_session_id | TEXT | ID de sesión del CLI (para `resume`) |
| transcript_path | TEXT | Archivo de sesión del CLI; de ahí se lee la cola del plan |
| pid | INTEGER | Proceso raíz del CLI |
| status | TEXT NOT NULL | `STARTING` · `RUNNING` · `EXITED` · `KILLED` · `FAILED` · `HANDED_OFF` |
| end_reason | TEXT | `COMPLETED` · `QUOTA_EXHAUSTED` · `RATE_LIMITED` · `AUTH_ERROR` · `CRASH` · `NO_HEARTBEAT` · `USER_SWITCH` · `USER_STOP` |
| exit_code | INTEGER | |
| last_heartbeat_at | INTEGER | Lo actualiza el daemon en memoria y lo escribe cada N segundos |
| started_at | INTEGER NOT NULL | |
| ended_at | INTEGER | |

UNIQUE(`agent_id`, `seq`). **Índice parcial:** `UNIQUE(agent_id) WHERE ended_at IS NULL` (un solo executor vivo por agente).

#### `executor_changes` · Fase 1
Historial de cambios de executor. Alimenta la vista *History* y el separador "Executor changed" en *Conversation*.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| agent_id | TEXT NOT NULL FK → agents | |
| from_run_id | TEXT NOT NULL FK → agent_runs | |
| to_run_id | TEXT FK → agent_runs | NULL si no hubo reemplazo (quedó `WAITING_PROVIDER`) |
| reason | TEXT NOT NULL | `FAILOVER` · `USER_SWITCH` · `SUGGESTION_ACCEPTED` · `RESTART` · `RECLAIM` |
| failure_id | TEXT FK → provider_failures | Si fue por falla |
| checkpoint_id | TEXT FK → checkpoints | Checkpoint usado para el handoff |
| checkpoint_age_ms | INTEGER | Para mostrar "Checkpoint: 8 sec old" y advertir si está viejo |
| occurred_at | INTEGER NOT NULL | |

#### `messages` · Fase 1
Espejo de la conversación visible con el executor. Vista *Agent Conversation*.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| agent_id | TEXT NOT NULL FK → agents | |
| run_id | TEXT FK → agent_runs | |
| role | TEXT NOT NULL | `USER` · `ASSISTANT` · `SYSTEM` · `EXECUTOR_CHANGE` |
| content | TEXT | Mensajes cortos |
| content_object_id | TEXT FK → context_objects | Mensajes largos van al object store |
| created_at | INTEGER NOT NULL | |

---

### D. Proveedores y modelos

#### `providers` · Fase 1
Las cinco familias. El ID es un slug estable.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | `anthropic` · `openai` · `google` · `kimi` · `github` |
| display_name | TEXT NOT NULL | "Claude" |
| cli_name | TEXT NOT NULL | `claude` · `codex` · … |
| cli_path | TEXT | Detectado |
| cli_version | TEXT | |
| adapter_mode | TEXT NOT NULL | `CLI` · `DIRECT` |
| setup_state | TEXT NOT NULL | `READY` · `LOGIN_REQUIRED` · `NOT_FOUND` · `ERROR` |
| enabled | INTEGER NOT NULL DEFAULT 1 | |
| hooks_supported | INTEGER NOT NULL DEFAULT 0 | |
| hooks_can_hold | INTEGER | Resultado del Test C: si el hook puede retener comandos. NULL = no probado |
| last_checked_at | INTEGER | |

*La reserva de cuota (`reserve = 0.20`) vive en `config.toml`, no aquí.*

#### `provider_accounts` · Fase 4
La salud se gestiona por proveedor + cuenta + modelo. En la mayoría de los casos habrá una cuenta por proveedor.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| provider_id | TEXT NOT NULL FK → providers | |
| label | TEXT NOT NULL | "personal", "Briga" |
| auth_status | TEXT NOT NULL | `OK` · `EXPIRED` · `MISSING` · `UNKNOWN` |
| last_checked_at | INTEGER | |

*Sin tokens ni credenciales: solo el estado que reporta el CLI.*

#### `models` · Fase 1
Model registry: metadata fija, no scores de calidad.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | Canónico: `claude/sonnet` |
| provider_id | TEXT NOT NULL FK → providers | |
| cli_model_id | TEXT NOT NULL | Lo que espera el CLI |
| display_name | TEXT NOT NULL | |
| context_window | INTEGER | Tokens |
| supports_tools | INTEGER NOT NULL DEFAULT 1 | |
| supports_vision | INTEGER NOT NULL DEFAULT 0 | |
| speed_class | TEXT | `FAST` · `MEDIUM` · `SLOW` |
| enabled | INTEGER NOT NULL DEFAULT 1 | |
| discovered_at | INTEGER NOT NULL | |
| last_seen_at | INTEGER | Si el CLI deja de listarlo, se marca como no disponible |

#### `provider_health` · Fase 4
Estado **actual** por scope. Es una fila que se actualiza, no un historial: el historial está en `provider_failures` y `events`.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| provider_id | TEXT NOT NULL FK → providers | |
| account_id | TEXT FK → provider_accounts | |
| model_id | TEXT FK → models | NULL = estado a nivel proveedor |
| state | TEXT NOT NULL | `HEALTHY` · `DEGRADED` · `THROTTLED` · `RATE_LIMITED` · `QUOTA_LOW` · `EXHAUSTED` · `AUTH_ERROR` · `OFFLINE` · `UNKNOWN` · `PROBING` |
| quota_certainty | TEXT NOT NULL | `KNOWN` · `ESTIMATED` · `UNKNOWN` |
| quota_remaining | REAL | 0–1. **Solo si `quota_certainty='KNOWN'`** (CHECK). Nunca con falsa precisión |
| evidence | TEXT | "provider warning", "429 x3" |
| retry_after_at | INTEGER | |
| reset_at | INTEGER | |
| confidence | REAL | 0–1 |
| updated_at | INTEGER NOT NULL | |

UNIQUE(`provider_id`, `account_id`, `model_id`).

#### `provider_failures` · Fase 1
Cada error ya clasificado por `parse_error()` del adapter. Así queda registrado que un 429 ≠ agotado.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| provider_id | TEXT NOT NULL FK → providers | |
| account_id | TEXT FK → provider_accounts | |
| model_id | TEXT FK → models | |
| run_id | TEXT FK → agent_runs | |
| failure_type | TEXT NOT NULL | `RPM` · `TPM` · `TEMP_RATE_LIMIT` · `DAILY_QUOTA` · `WEEKLY_QUOTA` · `MODEL_LIMIT` · `ACCOUNT_LIMIT` · `AUTH` · `NETWORK` · `PROVIDER_ERROR` · `MODEL_UNAVAILABLE` · `UNKNOWN` |
| raw_code | TEXT | `429`, exit code, texto clave |
| message | TEXT | Redactado (sin secretos) |
| retry_after_at | INTEGER | |
| reset_at | INTEGER | |
| confidence | REAL | |
| occurred_at | INTEGER NOT NULL | |

#### `usage_records` · Fase 4
Uso de tokens por turno o run. Vista *Usage / Context Stats*.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| run_id | TEXT NOT NULL FK → agent_runs | |
| provider_id | TEXT NOT NULL FK → providers | Desnormalizado para agregaciones rápidas |
| model_id | TEXT NOT NULL FK → models | |
| tokens_in | INTEGER | |
| tokens_out | INTEGER | |
| source | TEXT NOT NULL | `REPORTED` · `ESTIMATED`. La UI los muestra distinto |
| recorded_at | INTEGER NOT NULL | |

---

### E. Routing

#### `profiles` · Fase 4
Modelos virtuales (intención).

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | `@code` · `@debug` · `@fast` · `@reasoning` · `@docs` · `@review` · `@conserve` · … |
| description | TEXT NOT NULL | "Best available for implementation" |
| weights_json | TEXT NOT NULL | Pesos del scoring (calidad, cuota, latencia…) |
| builtin | INTEGER NOT NULL DEFAULT 1 | |

#### `profile_models` · Fase 4
Qué modelos son elegibles para cada profile, con un score **bootstrap** editable (no es verdad absoluta).

| Columna | Tipo | Notas |
|---|---|---|
| profile_id | TEXT NOT NULL FK → profiles | |
| model_id | TEXT NOT NULL FK → models | |
| base_score | REAL NOT NULL | 0–1, punto de partida |
| learned_adjustment | REAL | Fase 6; se aplica solo si `sample_count` ≥ umbral |
| sample_count | INTEGER NOT NULL DEFAULT 0 | |

PK(`profile_id`, `model_id`).

#### `routing_decisions` · Fase 4
Cada vez que Symphony elige un modelo. Alimenta `/explain-route`.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| agent_id | TEXT NOT NULL FK → agents | |
| trigger | TEXT NOT NULL | `SPAWN` · `FAILOVER` · `SWITCH` · `SUGGESTION` |
| profile_id | TEXT FK → profiles | |
| selected_model_id | TEXT FK → models | NULL si no hubo candidatos |
| engine | TEXT NOT NULL | `RULES` · `DECISION_MODEL` |
| explanation | TEXT | Generada desde los scores, sin LLM |
| decided_at | INTEGER NOT NULL | |

#### `routing_candidates` · Fase 4
Todos los modelos evaluados en una decisión, incluidos los descartados y el motivo.

| Columna | Tipo | Notas |
|---|---|---|
| decision_id | TEXT NOT NULL FK → routing_decisions | |
| model_id | TEXT NOT NULL FK → models | |
| eligible | INTEGER NOT NULL | |
| reject_reason | TEXT | `OFFLINE` · `AUTH` · `EXHAUSTED` · `CONTEXT` · `CAPABILITY` · `COOLDOWN` · `RESERVE` · `DISABLED` |
| score | REAL | NULL si no es elegible |
| factors_json | TEXT | `{"fit":+0.3,"quota":+0.2,"scarcity":-0.4}` |

PK(`decision_id`, `model_id`).

#### `model_suggestions` · Fase 6
Sugerencias de cambio por optimización (nunca automáticas).

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| agent_id | TEXT NOT NULL FK → agents | |
| current_model_id | TEXT NOT NULL FK → models | |
| suggested_model_id | TEXT NOT NULL FK → models | |
| reason | TEXT NOT NULL | "documentation → multi-file refactor" |
| confidence | REAL | |
| status | TEXT NOT NULL | `PENDING` · `ACCEPTED` · `KEPT` · `MUTED` |
| created_at | INTEGER NOT NULL | |
| resolved_at | INTEGER | |

---

### F. Eventos y scheduler

#### `events` · Fase 1
Event bus persistido (audit log). Es append-only y la tabla con más volumen.

| Columna | Tipo | Notas |
|---|---|---|
| id | INTEGER PK AUTOINCREMENT | |
| project_id | TEXT NOT NULL FK → projects | |
| agent_id | TEXT FK → agents | |
| run_id | TEXT FK → agent_runs | |
| type | TEXT NOT NULL | `AgentStarted` · `TurnStarted` · `ToolRequested` · `CommandFinished` · `FileModified` · `ProviderError` · `CheckpointCreated` · … |
| source | TEXT NOT NULL | `HOOK` · `JSON_STREAM` · `STDOUT` · `PTY` · `PROCESS` · `SYSTEM` · `USER` |
| payload_json | TEXT | Pequeño; outputs grandes van por `context_objects` |
| occurred_at | INTEGER NOT NULL | |

Índices: (`agent_id`, `occurred_at`), (`project_id`, `type`, `occurred_at`).
**Retención:** se purgan eventos de agentes archivados con más de N días. Checkpoints y runs no se purgan.

#### `tool_calls` · Fase 1
Cada herramienta o comando que pide el executor. Es la unidad que controla el scheduler.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| agent_id | TEXT NOT NULL FK → agents | |
| run_id | TEXT NOT NULL FK → agent_runs | |
| tool_name | TEXT NOT NULL | `bash` · `edit` · `grep` · `mcp:github/...` |
| command | TEXT | `npm test auth` (redactado) |
| cwd | TEXT | |
| op_class | INTEGER NOT NULL | 0–4 (red · barato · medio · pesado · muy pesado) |
| status | TEXT NOT NULL | `REQUESTED` · `QUEUED` · `RUNNING` · `DONE` · `FAILED` · `DENIED` · `CANCELLED` |
| queue_reason | TEXT | "heavy slot occupied by Agent #2" |
| blocked_by_tool_call_id | TEXT FK → tool_calls | Qué operación ocupa el slot |
| enforcement | TEXT | `HOOK_HOLD` · `OS_LIMIT` · `NONE`. Cómo se controló |
| exit_code | INTEGER | |
| output_object_id | TEXT FK → context_objects | |
| requested_at | INTEGER NOT NULL | |
| started_at | INTEGER | |
| finished_at | INTEGER | |

Índice parcial para la cola: (`status`, `op_class`, `requested_at`) `WHERE status IN ('QUEUED','RUNNING')`.

#### `processes` · Fase 2
Árbol de procesos observado en la capa OS. Vista *inspect processes*.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| run_id | TEXT NOT NULL FK → agent_runs | |
| tool_call_id | TEXT FK → tool_calls | Si se pudo atribuir |
| pid | INTEGER NOT NULL | |
| parent_process_id | TEXT FK → processes | |
| command | TEXT | |
| peak_rss_bytes | INTEGER | |
| cpu_time_ms | INTEGER | |
| started_at | INTEGER NOT NULL | |
| ended_at | INTEGER | |

#### `resource_samples` · Fase 2
Muestras periódicas del sistema. Vista *System* y benchmarks.

| Columna | Tipo | Notas |
|---|---|---|
| id | INTEGER PK AUTOINCREMENT | |
| session_id | TEXT FK → sessions | |
| cpu_pct | REAL NOT NULL | |
| ram_pct | REAL NOT NULL | |
| heavy_slots_used | INTEGER NOT NULL | |
| heavy_slots_total | INTEGER NOT NULL | |
| process_count | INTEGER | |
| performance_profile | TEXT NOT NULL | `ECO` · `BALANCED` · `PERFORMANCE` · `CUSTOM` |
| sampled_at | INTEGER NOT NULL | |

*Se muestrea cada 5–10 s y se agrega por minuto después de 24 h.*

---

### G. Checkpoints y contexto

#### `checkpoints` · Fase 1
Estado observable de un agente, actualizado de forma incremental. **Nunca se genera al fallar.**

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| agent_id | TEXT NOT NULL FK → agents | |
| run_id | TEXT FK → agent_runs | Run activo cuando se creó |
| seq | INTEGER NOT NULL | Incremental por agente |
| trigger_event_id | INTEGER FK → events | Qué evento lo disparó |
| objective | TEXT NOT NULL | |
| plan_tail | TEXT | Último plan/TODOs del transcript del CLI |
| current_step | TEXT | "debugging refresh rotation" |
| next_step | TEXT | Lo que muestra *Agent Overview → NEXT* |
| head_commit | TEXT | |
| diff_object_id | TEXT FK → context_objects | Diff actual |
| summary_json | TEXT | Archivos tocados, último comando, conteo de tests |
| is_valid | INTEGER NOT NULL DEFAULT 1 | 0 si está corrupto o incompleto (Recovery) |
| created_at | INTEGER NOT NULL | |

UNIQUE(`agent_id`, `seq`). *Se conservan los últimos N por agente más los usados en handoffs.*

#### `checkpoint_refs` · Fase 1
Relación N:M entre un checkpoint y los objetos de contexto relevantes en ese momento.

| Columna | Tipo | Notas |
|---|---|---|
| checkpoint_id | TEXT NOT NULL FK → checkpoints | |
| object_id | TEXT NOT NULL FK → context_objects | |
| role | TEXT NOT NULL | `DIFF` · `TEST_OUTPUT` · `TOOL_OUTPUT` · `FILE` · `FAILURE` · `DECISION` |

PK(`checkpoint_id`, `object_id`).

#### `blobs` · Fase 1
Índice del object store en disco (direccionado por contenido).

| Columna | Tipo | Notas |
|---|---|---|
| hash | TEXT PK | BLAKE3 hex |
| size_bytes | INTEGER NOT NULL | Original |
| stored_bytes | INTEGER NOT NULL | Después de zstd |
| codec | TEXT NOT NULL DEFAULT 'zstd' | |
| mime | TEXT | |
| ref_count | INTEGER NOT NULL DEFAULT 0 | Para GC |
| created_at | INTEGER NOT NULL | |

*El archivo vive en `~/.symphony/objects/<hash[0:2]>/<hash>`. Si dos agentes guardan el mismo log, se deduplica solo.*

#### `context_objects` · Fase 1
Cada `ctx://...` direccionable. Guarda el original (vía blob) y su versión comprimida.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| uri | TEXT NOT NULL UNIQUE | `ctx://run/921`, `ctx://file/auth.ts@<commit>` |
| project_id | TEXT NOT NULL FK → projects | |
| agent_id | TEXT FK → agents | Quién lo generó |
| run_id | TEXT FK → agent_runs | |
| kind | TEXT NOT NULL | `FILE` · `GIT_DIFF` · `TEST_OUTPUT` · `TOOL_OUTPUT` · `LOG` · `JSON` · `CONVERSATION` · `DECISION` |
| blob_hash | TEXT NOT NULL FK → blobs | Original, siempre recuperable |
| compressed_text | TEXT | Versión compacta ("896 passed, 1 failed…") |
| compressor | TEXT | `LOG_COLLAPSE` · `JSON_STRUCT` · `AST` · `DEDUP` · `NONE` |
| tokens_original | INTEGER | Estimado |
| tokens_compressed | INTEGER | |
| created_at | INTEGER NOT NULL | |

#### `context_chunks` · Fase 3
Fragmentos de un objeto para buscar y recuperar solo lo necesario (`context.search`, `context.lines`).

| Columna | Tipo | Notas |
|---|---|---|
| id | INTEGER PK AUTOINCREMENT | rowid para FTS5 |
| object_id | TEXT NOT NULL FK → context_objects | |
| seq | INTEGER NOT NULL | |
| start_line | INTEGER | |
| end_line | INTEGER | |
| text | TEXT NOT NULL | |

#### `context_fts` · Fase 3 (tabla virtual)
`CREATE VIRTUAL TABLE context_fts USING fts5(text, content='context_chunks', content_rowid='id', tokenize='unicode61');`
Búsqueda BM25 sobre los chunks, sin vector DB.

#### `handoffs` · Fase 1
Cada prompt de arranque armado para un executor (spawn o failover).

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| agent_id | TEXT NOT NULL FK → agents | |
| checkpoint_id | TEXT FK → checkpoints | NULL en el primer spawn |
| to_run_id | TEXT NOT NULL UNIQUE FK → agent_runs | Un handoff inicia exactamente un run |
| mode | TEXT NOT NULL | `RAW` · `SAFE` · `BALANCED` · `AGGRESSIVE` |
| tokens_raw_estimate | INTEGER | Lo que costaría sin optimizar |
| tokens_sent | INTEGER | |
| build_ms | INTEGER | Latencia del armado |
| outcome | TEXT | `CONTINUED` · `NEEDED_RETRIEVAL` · `FAILED_TO_CONTINUE` · `RETRIED_SAFER`. Mide si AGENT ≠ MODEL funciona |
| created_at | INTEGER NOT NULL | |

#### `handoff_items` · Fase 3
Qué entró al handoff y con qué fidelidad. Vista *Context Overview*.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| handoff_id | TEXT NOT NULL FK → handoffs | |
| section | TEXT NOT NULL | `OBJECTIVE` · `PLAN` · `DECISIONS` · `FAILURES` · `CODE` · `DIFF` · `REFERENCES` |
| object_id | TEXT FK → context_objects | |
| path | TEXT | Si es un archivo |
| fidelity | INTEGER | 0–5 (L0–L5) |
| tokens | INTEGER NOT NULL | |

#### `context_retrievals` · Fase 3
Cada vez que un executor pidió más detalle vía el MCP de contexto. Sirve para detectar compresión demasiado agresiva.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| run_id | TEXT NOT NULL FK → agent_runs | |
| object_id | TEXT NOT NULL FK → context_objects | |
| operation | TEXT NOT NULL | `RETRIEVE` · `SEARCH` · `LINES` |
| query | TEXT | |
| tokens_returned | INTEGER | |
| found | INTEGER NOT NULL | 0 = retrieval miss |
| requested_at | INTEGER NOT NULL | |

#### `project_facts` · Fase 3
Memoria consolidada: qué sigue siendo verdad en el proyecto (decisiones, stack, restricciones).

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| project_id | TEXT NOT NULL FK → projects | |
| key | TEXT NOT NULL | `auth_storage` |
| value | TEXT NOT NULL | `httpOnly cookie` |
| kind | TEXT NOT NULL | `ARCHITECTURE` · `DECISION` · `CONSTRAINT` · `CONVENTION` |
| status | TEXT NOT NULL | `CURRENT` · `SUPERSEDED` · `CONFLICT` |
| superseded_by_id | TEXT FK → project_facts | |
| source_agent_id | TEXT FK → agents | |
| source_event_id | INTEGER FK → events | |
| created_at | INTEGER NOT NULL | |

**Índice parcial:** `UNIQUE(project_id, key) WHERE status = 'CURRENT'`. Un solo valor vigente por clave; `CONFLICT` alimenta la rama "Contexto inconsistente".

---

### H. Validación y merge

#### `validation_runs` · Fase 2

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| agent_id | TEXT NOT NULL FK → agents | |
| run_id | TEXT FK → agent_runs | |
| tier | INTEGER NOT NULL | 1 · 2 · 3 |
| trigger | TEXT NOT NULL | `AUTO` · `USER` · `MERGE_GATE` · `MILESTONE_GATE` |
| status | TEXT NOT NULL | `QUEUED` · `RUNNING` · `PASS` · `FAIL` · `SKIPPED` |
| tool_call_id | TEXT FK → tool_calls | Pasa por el scheduler como cualquier comando pesado |
| started_at | INTEGER | |
| finished_at | INTEGER | |

#### `validation_checks` · Fase 2

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| validation_run_id | TEXT NOT NULL FK → validation_runs | |
| name | TEXT NOT NULL | `syntax` · `format` · `lint` · `typecheck` · `targeted_tests` · `full_tests` · `build` · `security` |
| status | TEXT NOT NULL | `PASS` · `FAIL` · `WARN` · `SKIPPED` |
| passed_count | INTEGER | |
| failed_count | INTEGER | |
| summary | TEXT | "auth.refresh.test.ts → expected 200, received 401" |
| output_object_id | TEXT FK → context_objects | Log completo |

#### `merge_requests` · Fase 2
Preflight y cola de integración.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| agent_id | TEXT NOT NULL FK → agents | |
| task_id | TEXT NOT NULL FK → tasks | |
| target_branch | TEXT NOT NULL | |
| status | TEXT NOT NULL | `PREFLIGHT` · `READY` · `QUEUED` · `MERGED` · `BLOCKED` · `ABORTED` |
| preflight_json | TEXT | Resultado de cada check (task, validación, conflictos, deps, política) |
| requires_approval | INTEGER NOT NULL DEFAULT 1 | |
| approved_at | INTEGER | |
| queue_position | INTEGER | |
| merge_commit | TEXT | |
| created_at | INTEGER NOT NULL | |
| merged_at | INTEGER | |

#### `merge_conflicts` · Fase 2

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| merge_request_id | TEXT NOT NULL FK → merge_requests | |
| path | TEXT NOT NULL | |
| other_agent_id | TEXT FK → agents | Con quién choca (si se sabe) |
| resolution | TEXT NOT NULL | `PENDING` · `MANUAL` · `REVIEWER` · `ABORTED` |
| resolved_at | INTEGER | |

#### `reviews` · Fase 2

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| target_agent_id | TEXT NOT NULL FK → agents | Agente revisado |
| reviewer_agent_id | TEXT NOT NULL FK → agents | Agente con task `kind='REVIEW'` |
| merge_request_id | TEXT FK → merge_requests | |
| status | TEXT NOT NULL | `RUNNING` · `DONE` · `FAILED` |
| recommendation | TEXT | `APPROVE` · `REQUEST_CHANGES` · `REJECT` |
| findings_json | TEXT | |
| created_at | INTEGER NOT NULL | |
| finished_at | INTEGER | |

*Un review nunca mergea solo: solo recomienda.*

---

### I. Recuperación

#### `recovery_items` · Fase 1
Todo lo que requiere intervención humana, en un solo lugar. Vista *Recovery Center*.

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| project_id | TEXT NOT NULL FK → projects | |
| agent_id | TEXT FK → agents | |
| run_id | TEXT FK → agent_runs | |
| kind | TEXT NOT NULL | `EXECUTOR_EXITED` · `NO_HEARTBEAT` · `RATE_LIMITED` · `AUTH_ERROR` · `CHECKPOINT_INVALID` · `WORKSPACE_DAMAGED` · `MERGE_CONFLICT` · `DEPENDENCY_FAILED` · `SESSION_INTERRUPTED` · `MACHINE_PRESSURE` |
| detail | TEXT NOT NULL | Frase humana |
| checkpoint_valid | INTEGER | Snapshot al detectarse |
| workspace_intact | INTEGER | |
| status | TEXT NOT NULL | `OPEN` · `RESOLVED` · `DISMISSED` |
| resolution | TEXT | `RESTART` · `FAILOVER` · `REBUILD_FROM_WORKSPACE` · `OLDER_CHECKPOINT` · `RECLAIM` · `ARCHIVE` · `MANUAL` |
| created_at | INTEGER NOT NULL | |
| resolved_at | INTEGER | |

---

### J. Recursos compartidos

#### `skills` · Fase 3

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| name | TEXT NOT NULL | |
| scope | TEXT NOT NULL | `BUILTIN` · `USER` · `PROJECT` |
| project_id | TEXT FK → projects | Solo si `scope='PROJECT'` |
| path | TEXT NOT NULL | Fuente canónica en disco |
| content_hash | TEXT | Para detectar cambios y regenerar el formato de cada CLI |
| enabled | INTEGER NOT NULL DEFAULT 1 | |
| updated_at | INTEGER NOT NULL | |

Prioridad al resolver nombres duplicados: `PROJECT` > `USER` > `BUILTIN`.

#### `mcp_servers` · Fase 3

| Columna | Tipo | Notas |
|---|---|---|
| id | TEXT PK | |
| name | TEXT NOT NULL | `github` · `playwright` · `symphony-context` |
| scope | TEXT NOT NULL | `USER` · `PROJECT` |
| project_id | TEXT FK → projects | |
| transport | TEXT NOT NULL | `STDIO` · `HTTP` |
| share_mode | TEXT NOT NULL | `SHARED` (una instancia) · `PER_CLIENT` (compatibilidad) |
| command | TEXT | Si es stdio |
| url | TEXT | Si es http |
| enabled | INTEGER NOT NULL DEFAULT 1 | |
| status | TEXT | `RUNNING` · `STOPPED` · `ERROR` |

*Las variables secretas de un MCP no se guardan aquí: se referencian desde el entorno o el keychain del sistema.*

---

## 4. Relaciones principales

| Desde | Cardinalidad | Hacia | Significado |
|---|---|---|---|
| projects | 1 : N | sessions, tasks, agents, worktrees, milestones | Todo pertenece a un proyecto |
| sessions | 1 : N | agents | Sesión donde se creó el agente (el agente la sobrevive) |
| milestones | 1 : N | tasks | Agrupación opcional |
| tasks | N : M | tasks | DAG vía `task_dependencies` |
| tasks | 1 : N | agents | Historial de agentes; **máximo 1 activo** |
| agents | 1 : 0..1 | worktrees | Un workspace aislado por agente |
| agents | 1 : N | agent_runs | **AGENT ≠ MODEL**: cada run es un executor distinto |
| agent_runs | N : 1 | providers, models, provider_accounts | Quién y con qué ejecutó |
| agent_runs | N : 0..1 | routing_decisions | Si vino de un profile |
| agent_runs | 1 : 1 | handoffs | Cada run arranca de un handoff |
| agents | 1 : N | executor_changes | Cada cambio conecta `from_run` → `to_run` |
| executor_changes | N : 0..1 | provider_failures, checkpoints | Por qué cambió y desde dónde siguió |
| agent_runs | 1 : N | events, tool_calls, messages, usage_records, processes | Lo que ocurrió durante ese executor |
| tool_calls | N : 0..1 | tool_calls | `blocked_by`: quién ocupa el slot |
| agents | 1 : N | checkpoints | Estado incremental |
| checkpoints | N : M | context_objects | Vía `checkpoint_refs` |
| handoffs | 1 : N | handoff_items | Qué entró y con qué fidelidad |
| context_objects | N : 1 | blobs | Original siempre recuperable |
| context_objects | 1 : N | context_chunks → context_fts | Búsqueda BM25 |
| project_facts | N : 0..1 | project_facts | `superseded_by` |
| profiles | N : M | models | Vía `profile_models` |
| routing_decisions | 1 : N | routing_candidates | Todos los evaluados, incluidos los rechazados |
| providers | 1 : N | models, provider_accounts, provider_health, provider_failures | |
| agents | 1 : N | validation_runs → validation_checks | Tier 1–3 |
| agents | 1 : N | merge_requests → merge_conflicts | Preflight y cola |
| agents | 1 : N | reviews (como target y como reviewer) | |
| projects | 1 : N | recovery_items, project_facts, skills, mcp_servers | |

---

## 5. Consultas que la base debe resolver rápido

Cada vista del flujo se traduce en una consulta sencilla.

| Vista | Consulta |
|---|---|
| **Home** | agents activos + su run abierto (`ended_at IS NULL`) + modelo + task, `provider_health` por proveedor y el último `resource_samples` |
| **Agent Overview** | agente + run actual + último checkpoint (`MAX(seq)`) + stats del diff (`diff_object_id`) + última `validation_runs` |
| **Agent History** | `executor_changes` + `agent_runs` del agente ordenados por `seq` |
| **WAITING_RESOURCE** | `tool_calls` con `status='QUEUED'` + JOIN a `blocked_by_tool_call_id` → agente que ocupa el slot |
| **Task List** | tasks por status + `task_dependencies` pendientes |
| **Explain Route** | `routing_decisions` + `routing_candidates` de la última decisión |
| **Failover** | último checkpoint válido + candidatos filtrados por `provider_health` + `failover_policy` |
| **Recovery Center** | `recovery_items WHERE status='OPEN'` |
| **Context Stats** | `SUM(tokens_raw_estimate)`, `SUM(tokens_sent)` de handoffs + `context_retrievals WHERE found=0` |
| **Usage** | `usage_records` agrupado por proveedor, separando `REPORTED` y `ESTIMATED` |

---

## 6. Qué tablas entran en cada fase

| Fase | Tablas |
|---|---|
| **0 · Spike** | Ninguna: `events.jsonl` + `checkpoint.json` |
| **1 · Core** (19) | projects, sessions, tasks, agents, worktrees, agent_runs, executor_changes, messages, providers, models, provider_failures, events, tool_calls, checkpoints, checkpoint_refs, blobs, context_objects, handoffs, recovery_items |
| **2 · Multi-agent** | task_dependencies, milestones, processes, resource_samples, validation_runs, validation_checks, merge_requests, merge_conflicts, reviews |
| **3 · Context Engine** | context_chunks, context_fts, handoff_items, context_retrievals, project_facts, skills, mcp_servers |
| **4 · Failover y profiles** | provider_accounts, provider_health, usage_records, profiles, profile_models, routing_decisions, routing_candidates |
| **6 · Opcionales** | model_suggestions, columna `learned_adjustment` activa |

Las migraciones se versionan con `PRAGMA user_version` y archivos `migrations/NNN_*.sql` aplicados por el daemon al arrancar.

---

## 7. Configuración de SQLite

```sql
PRAGMA journal_mode = WAL;          -- lectores no bloquean al escritor
PRAGMA synchronous = NORMAL;        -- seguro con WAL y mucho más rápido que FULL
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA temp_store = MEMORY;
PRAGMA cache_size = -16000;         -- ~16 MB de caché, suficiente
PRAGMA wal_autocheckpoint = 1000;
```

- **Escritura:** los eventos de hooks se acumulan en memoria y se escriben en una transacción cada ~100 ms o cada 50 eventos. Esto mantiene la latencia baja con varios agentes.
- **Lectura:** la TUI y la futura GUI leen por la API del daemon, no abren la DB directamente.
- **Mantenimiento:** `PRAGMA optimize` al cerrar sesión. GC de blobs con `ref_count = 0` y retención de `events` y `resource_samples` en un job manual o al arrancar.

---

## 8. Pendientes

- Definir el umbral de `sample_count` para que el desempeño aprendido influya en el router.
- Decidir si `messages` guarda toda la conversación o solo una ventana más punteros al transcript del CLI (para no duplicar datos).
- Validar con el spike si hace falta `processes` fila por fila, o si basta con agregados por run.
- Revisar el tamaño real de `events` con 3 agentes en una jornada, para fijar la retención.
