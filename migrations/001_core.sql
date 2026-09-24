-- 001 · Core (Fase 1 de DB §6): las 19 tablas del núcleo.
-- Copia fiel de docs/spec/symphony_database.md §3. Convenciones (DB §1):
-- IDs TEXT (ULID), fechas INTEGER epoch ms UTC, booleanos INTEGER 0/1,
-- enums TEXT + CHECK, JSON TEXT validado con json_valid().
--
-- FKs hacia tablas de fases futuras (milestones, profiles, provider_accounts,
-- routing_decisions) quedan como columnas nullable SIN FK; su migración las agrega.

-- A. Proyecto y sesión ------------------------------------------------------

CREATE TABLE projects (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    root_path       TEXT NOT NULL UNIQUE,
    default_branch  TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    last_opened_at  INTEGER,
    archived_at     INTEGER
) STRICT;

CREATE TABLE sessions (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id),
    status      TEXT NOT NULL CHECK (status IN ('ACTIVE','CLOSED','INTERRUPTED')),
    daemon_pid  INTEGER,
    started_at  INTEGER NOT NULL,
    ended_at    INTEGER
) STRICT;
CREATE INDEX sessions_project ON sessions(project_id, started_at);

-- B. Trabajo ----------------------------------------------------------------

CREATE TABLE tasks (
    id             TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL REFERENCES projects(id),
    milestone_id   TEXT,  -- FK → milestones en la migración de Fase 2
    code           TEXT NOT NULL,
    kind           TEXT NOT NULL CHECK (kind IN ('WORK','REVIEW')),
    title          TEXT NOT NULL,
    description    TEXT,
    status         TEXT NOT NULL CHECK (status IN ('BACKLOG','READY','RUNNING','WAITING','BLOCKED','DONE','FAILED','CANCELLED')),
    status_reason  TEXT,
    priority       INTEGER NOT NULL DEFAULT 0,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    completed_at   INTEGER,
    UNIQUE (project_id, code)
) STRICT;
CREATE INDEX tasks_project_status ON tasks(project_id, status);

-- C. Agentes y ejecución ----------------------------------------------------

CREATE TABLE worktrees (
    id              TEXT PRIMARY KEY,
    project_id      TEXT NOT NULL REFERENCES projects(id),
    path            TEXT NOT NULL UNIQUE,
    branch          TEXT NOT NULL,
    base_ref        TEXT NOT NULL,
    head_commit     TEXT,
    deps_strategy   TEXT NOT NULL CHECK (deps_strategy IN ('PNPM_STORE','LINK','INSTALL','NONE')),
    deps_lock_hash  TEXT,
    status          TEXT NOT NULL CHECK (status IN ('CREATING','READY','DIRTY','DAMAGED','REMOVED')),
    created_at      INTEGER NOT NULL,
    removed_at      INTEGER
) STRICT;

-- AGENT ≠ MODEL: esta tabla NO guarda el modelo actual; vive en el run abierto.
CREATE TABLE agents (
    id                    TEXT PRIMARY KEY,
    project_id            TEXT NOT NULL REFERENCES projects(id),
    session_id            TEXT NOT NULL REFERENCES sessions(id),
    task_id               TEXT NOT NULL REFERENCES tasks(id),
    worktree_id           TEXT UNIQUE REFERENCES worktrees(id),
    number                INTEGER NOT NULL,
    state                 TEXT NOT NULL CHECK (state IN ('CREATED','READY','RUNNING','WAITING_PROVIDER','WAITING_RESOURCE','WAITING_DEPENDENCY','TESTING','BLOCKED','PAUSED','COMPLETED','FAILED','CANCELLED')),
    state_reason          TEXT,
    execution_mode        TEXT NOT NULL CHECK (execution_mode IN ('EXACT','PROFILE','DECIDE_LATER')),
    requested_model_id    TEXT REFERENCES models(id),
    requested_profile_id  TEXT,  -- FK → profiles en la migración de Fase 4
    failover_policy       TEXT NOT NULL CHECK (failover_policy IN ('NONE','SAME_PROVIDER','ANY')),
    context_mode          TEXT NOT NULL CHECK (context_mode IN ('RAW','SAFE','BALANCED','AGGRESSIVE')),
    priority              INTEGER NOT NULL DEFAULT 0,
    created_at            INTEGER NOT NULL,
    updated_at            INTEGER NOT NULL,
    archived_at           INTEGER,
    UNIQUE (project_id, number),
    CHECK (execution_mode <> 'EXACT' OR requested_model_id IS NOT NULL),
    CHECK (execution_mode <> 'PROFILE' OR requested_profile_id IS NOT NULL)
) STRICT;
-- Solo un agente activo por task.
CREATE UNIQUE INDEX agents_one_active_per_task ON agents(task_id)
    WHERE state NOT IN ('COMPLETED','FAILED','CANCELLED');
CREATE INDEX agents_project_state ON agents(project_id, state);

CREATE TABLE agent_runs (
    id                   TEXT PRIMARY KEY,
    agent_id             TEXT NOT NULL REFERENCES agents(id),
    seq                  INTEGER NOT NULL,
    provider_id          TEXT NOT NULL REFERENCES providers(id),
    account_id           TEXT,  -- FK → provider_accounts en la migración de Fase 4
    model_id             TEXT NOT NULL REFERENCES models(id),
    routing_decision_id  TEXT,  -- FK → routing_decisions en la migración de Fase 4
    start_checkpoint_id  TEXT REFERENCES checkpoints(id),
    cli_session_id       TEXT,
    transcript_path      TEXT,
    pid                  INTEGER,
    status               TEXT NOT NULL CHECK (status IN ('STARTING','RUNNING','EXITED','KILLED','FAILED','HANDED_OFF')),
    end_reason           TEXT CHECK (end_reason IN ('COMPLETED','QUOTA_EXHAUSTED','RATE_LIMITED','AUTH_ERROR','CRASH','NO_HEARTBEAT','USER_SWITCH','USER_STOP')),
    exit_code            INTEGER,
    last_heartbeat_at    INTEGER,
    started_at           INTEGER NOT NULL,
    ended_at             INTEGER,
    UNIQUE (agent_id, seq)
) STRICT;
-- Un solo executor vivo por agente.
CREATE UNIQUE INDEX agent_runs_one_open_per_agent ON agent_runs(agent_id) WHERE ended_at IS NULL;

CREATE TABLE executor_changes (
    id                 TEXT PRIMARY KEY,
    agent_id           TEXT NOT NULL REFERENCES agents(id),
    from_run_id        TEXT NOT NULL REFERENCES agent_runs(id),
    to_run_id          TEXT REFERENCES agent_runs(id),
    reason             TEXT NOT NULL CHECK (reason IN ('FAILOVER','USER_SWITCH','SUGGESTION_ACCEPTED','RESTART','RECLAIM')),
    failure_id         TEXT REFERENCES provider_failures(id),
    checkpoint_id      TEXT REFERENCES checkpoints(id),
    checkpoint_age_ms  INTEGER,
    occurred_at        INTEGER NOT NULL
) STRICT;
CREATE INDEX executor_changes_agent ON executor_changes(agent_id, occurred_at);

CREATE TABLE messages (
    id                 TEXT PRIMARY KEY,
    agent_id           TEXT NOT NULL REFERENCES agents(id),
    run_id             TEXT REFERENCES agent_runs(id),
    role               TEXT NOT NULL CHECK (role IN ('USER','ASSISTANT','SYSTEM','EXECUTOR_CHANGE')),
    content            TEXT,
    content_object_id  TEXT REFERENCES context_objects(id),
    created_at         INTEGER NOT NULL
) STRICT;
CREATE INDEX messages_agent ON messages(agent_id, created_at);

-- D. Proveedores y modelos --------------------------------------------------

CREATE TABLE providers (
    id               TEXT PRIMARY KEY,
    display_name     TEXT NOT NULL,
    cli_name         TEXT NOT NULL,
    cli_path         TEXT,
    cli_version      TEXT,
    adapter_mode     TEXT NOT NULL CHECK (adapter_mode IN ('CLI','DIRECT')),
    setup_state      TEXT NOT NULL CHECK (setup_state IN ('READY','LOGIN_REQUIRED','NOT_FOUND','ERROR')),
    enabled          INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    hooks_supported  INTEGER NOT NULL DEFAULT 0 CHECK (hooks_supported IN (0,1)),
    hooks_can_hold   INTEGER CHECK (hooks_can_hold IN (0,1)),  -- NULL = no probado (Test C)
    last_checked_at  INTEGER
) STRICT;

CREATE TABLE models (
    id               TEXT PRIMARY KEY,
    provider_id      TEXT NOT NULL REFERENCES providers(id),
    cli_model_id     TEXT NOT NULL,
    display_name     TEXT NOT NULL,
    context_window   INTEGER,
    supports_tools   INTEGER NOT NULL DEFAULT 1 CHECK (supports_tools IN (0,1)),
    supports_vision  INTEGER NOT NULL DEFAULT 0 CHECK (supports_vision IN (0,1)),
    speed_class      TEXT CHECK (speed_class IN ('FAST','MEDIUM','SLOW')),
    enabled          INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    discovered_at    INTEGER NOT NULL,
    last_seen_at     INTEGER
) STRICT;
CREATE INDEX models_provider ON models(provider_id);

CREATE TABLE provider_failures (
    id              TEXT PRIMARY KEY,
    provider_id     TEXT NOT NULL REFERENCES providers(id),
    account_id      TEXT,  -- FK → provider_accounts en la migración de Fase 4
    model_id        TEXT REFERENCES models(id),
    run_id          TEXT REFERENCES agent_runs(id),
    failure_type    TEXT NOT NULL CHECK (failure_type IN ('RPM','TPM','TEMP_RATE_LIMIT','DAILY_QUOTA','WEEKLY_QUOTA','MODEL_LIMIT','ACCOUNT_LIMIT','AUTH','NETWORK','PROVIDER_ERROR','MODEL_UNAVAILABLE','UNKNOWN')),
    raw_code        TEXT,
    message         TEXT,  -- redactado (sin secretos)
    retry_after_at  INTEGER,
    reset_at        INTEGER,
    confidence      REAL CHECK (confidence BETWEEN 0 AND 1),
    occurred_at     INTEGER NOT NULL
) STRICT;
CREATE INDEX provider_failures_provider ON provider_failures(provider_id, occurred_at);

-- F. Eventos y scheduler ----------------------------------------------------

CREATE TABLE events (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id    TEXT NOT NULL REFERENCES projects(id),
    agent_id      TEXT REFERENCES agents(id),
    run_id        TEXT REFERENCES agent_runs(id),
    type          TEXT NOT NULL,
    source        TEXT NOT NULL CHECK (source IN ('HOOK','JSON_STREAM','STDOUT','PTY','PROCESS','SYSTEM','USER')),
    payload_json  TEXT CHECK (payload_json IS NULL OR json_valid(payload_json)),
    occurred_at   INTEGER NOT NULL
) STRICT;
CREATE INDEX events_agent_time ON events(agent_id, occurred_at);
CREATE INDEX events_project_type_time ON events(project_id, type, occurred_at);

CREATE TABLE tool_calls (
    id                       TEXT PRIMARY KEY,
    agent_id                 TEXT NOT NULL REFERENCES agents(id),
    run_id                   TEXT NOT NULL REFERENCES agent_runs(id),
    tool_name                TEXT NOT NULL,
    command                  TEXT,  -- redactado
    cwd                      TEXT,
    op_class                 INTEGER NOT NULL CHECK (op_class BETWEEN 0 AND 4),
    status                   TEXT NOT NULL CHECK (status IN ('REQUESTED','QUEUED','RUNNING','DONE','FAILED','DENIED','CANCELLED')),
    queue_reason             TEXT,
    blocked_by_tool_call_id  TEXT REFERENCES tool_calls(id),
    enforcement              TEXT CHECK (enforcement IN ('HOOK_HOLD','OS_LIMIT','NONE')),
    exit_code                INTEGER,
    output_object_id         TEXT REFERENCES context_objects(id),
    requested_at             INTEGER NOT NULL,
    started_at               INTEGER,
    finished_at              INTEGER
) STRICT;
-- Cola del scheduler.
CREATE INDEX tool_calls_queue ON tool_calls(status, op_class, requested_at)
    WHERE status IN ('QUEUED','RUNNING');
CREATE INDEX tool_calls_run ON tool_calls(run_id);

-- G. Checkpoints y contexto -------------------------------------------------

CREATE TABLE blobs (
    hash          TEXT PRIMARY KEY,  -- BLAKE3 hex
    size_bytes    INTEGER NOT NULL,
    stored_bytes  INTEGER NOT NULL,
    codec         TEXT NOT NULL DEFAULT 'zstd',
    mime          TEXT,
    ref_count     INTEGER NOT NULL DEFAULT 0 CHECK (ref_count >= 0),
    created_at    INTEGER NOT NULL
) STRICT;

CREATE TABLE context_objects (
    id                 TEXT PRIMARY KEY,
    uri                TEXT NOT NULL UNIQUE,
    project_id         TEXT NOT NULL REFERENCES projects(id),
    agent_id           TEXT REFERENCES agents(id),
    run_id             TEXT REFERENCES agent_runs(id),
    kind               TEXT NOT NULL CHECK (kind IN ('FILE','GIT_DIFF','TEST_OUTPUT','TOOL_OUTPUT','LOG','JSON','CONVERSATION','DECISION')),
    blob_hash          TEXT NOT NULL REFERENCES blobs(hash),
    compressed_text    TEXT,
    compressor         TEXT CHECK (compressor IN ('LOG_COLLAPSE','JSON_STRUCT','AST','DEDUP','NONE')),
    tokens_original    INTEGER,
    tokens_compressed  INTEGER,
    created_at         INTEGER NOT NULL
) STRICT;
CREATE INDEX context_objects_blob ON context_objects(blob_hash);

-- Nunca se genera al fallar: se actualiza de forma incremental.
CREATE TABLE checkpoints (
    id                TEXT PRIMARY KEY,
    agent_id          TEXT NOT NULL REFERENCES agents(id),
    run_id            TEXT REFERENCES agent_runs(id),
    seq               INTEGER NOT NULL,
    trigger_event_id  INTEGER REFERENCES events(id),
    objective         TEXT NOT NULL,
    plan_tail         TEXT,
    current_step      TEXT,
    next_step         TEXT,
    head_commit       TEXT,
    diff_object_id    TEXT REFERENCES context_objects(id),
    summary_json      TEXT CHECK (summary_json IS NULL OR json_valid(summary_json)),
    is_valid          INTEGER NOT NULL DEFAULT 1 CHECK (is_valid IN (0,1)),
    created_at        INTEGER NOT NULL,
    UNIQUE (agent_id, seq)
) STRICT;

CREATE TABLE checkpoint_refs (
    checkpoint_id  TEXT NOT NULL REFERENCES checkpoints(id),
    object_id      TEXT NOT NULL REFERENCES context_objects(id),
    role           TEXT NOT NULL CHECK (role IN ('DIFF','TEST_OUTPUT','TOOL_OUTPUT','FILE','FAILURE','DECISION')),
    PRIMARY KEY (checkpoint_id, object_id)
) STRICT;

CREATE TABLE handoffs (
    id                   TEXT PRIMARY KEY,
    agent_id             TEXT NOT NULL REFERENCES agents(id),
    checkpoint_id        TEXT REFERENCES checkpoints(id),  -- NULL en el primer spawn
    to_run_id            TEXT NOT NULL UNIQUE REFERENCES agent_runs(id),
    mode                 TEXT NOT NULL CHECK (mode IN ('RAW','SAFE','BALANCED','AGGRESSIVE')),
    tokens_raw_estimate  INTEGER,
    tokens_sent          INTEGER,
    build_ms             INTEGER,
    outcome              TEXT CHECK (outcome IN ('CONTINUED','NEEDED_RETRIEVAL','FAILED_TO_CONTINUE','RETRIED_SAFER')),
    created_at           INTEGER NOT NULL
) STRICT;

-- I. Recuperación -----------------------------------------------------------

CREATE TABLE recovery_items (
    id                TEXT PRIMARY KEY,
    project_id        TEXT NOT NULL REFERENCES projects(id),
    agent_id          TEXT REFERENCES agents(id),
    run_id            TEXT REFERENCES agent_runs(id),
    kind              TEXT NOT NULL CHECK (kind IN ('EXECUTOR_EXITED','NO_HEARTBEAT','RATE_LIMITED','AUTH_ERROR','CHECKPOINT_INVALID','WORKSPACE_DAMAGED','MERGE_CONFLICT','DEPENDENCY_FAILED','SESSION_INTERRUPTED','MACHINE_PRESSURE')),
    detail            TEXT NOT NULL,
    checkpoint_valid  INTEGER CHECK (checkpoint_valid IN (0,1)),
    workspace_intact  INTEGER CHECK (workspace_intact IN (0,1)),
    status            TEXT NOT NULL CHECK (status IN ('OPEN','RESOLVED','DISMISSED')),
    resolution        TEXT CHECK (resolution IN ('RESTART','FAILOVER','REBUILD_FROM_WORKSPACE','OLDER_CHECKPOINT','RECLAIM','ARCHIVE','MANUAL')),
    created_at        INTEGER NOT NULL,
    resolved_at       INTEGER
) STRICT;
CREATE INDEX recovery_items_open ON recovery_items(project_id) WHERE status = 'OPEN';
