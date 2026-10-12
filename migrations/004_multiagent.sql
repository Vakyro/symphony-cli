-- 004 · Runtime multiagente (Fase 2 de DB §6, P08.S1; ADR-0013).
-- Copia fiel de docs/spec/symphony_database.md §3.B, §3.C y §3.H, más:
--   · la FK `tasks.milestone_id → milestones` que 001 dejó pendiente;
--   · `PHASE_DONE` en `agent_runs.end_reason` (ADR-0013 D2: un run termina al cerrarse
--     una fase del plan y el daemon abre otro con handoff acotado).
-- Es la 004 y no la 002 que decía el PLAN: 002 y 003 se ejecutaron antes (ADR-0010).

-- B. Trabajo -----------------------------------------------------------------

CREATE TABLE milestones (
    id                 TEXT PRIMARY KEY,
    project_id         TEXT NOT NULL REFERENCES projects(id),
    name               TEXT NOT NULL,
    requires_approval  INTEGER NOT NULL DEFAULT 0 CHECK (requires_approval IN (0,1)),
    status             TEXT NOT NULL CHECK (status IN ('OPEN','IN_REVIEW','APPROVED','CHANGES_REQUESTED','HELD')),
    decided_at         INTEGER
) STRICT;
CREATE INDEX milestones_project ON milestones(project_id);

CREATE TABLE task_dependencies (
    task_id             TEXT NOT NULL REFERENCES tasks(id),
    depends_on_task_id  TEXT NOT NULL REFERENCES tasks(id),
    created_at          INTEGER NOT NULL,
    PRIMARY KEY (task_id, depends_on_task_id),
    CHECK (task_id <> depends_on_task_id)
) STRICT;
-- Quién espera a una task que acaba de cerrarse.
CREATE INDEX task_dependencies_blocker ON task_dependencies(depends_on_task_id);

-- FKs y CHECKs pendientes ------------------------------------------------------
-- SQLite no puede añadir una FK ni cambiar un CHECK: se reconstruyen `tasks` y
-- `agent_runs` con el patrón de 002/003 (`defer_foreign_keys`, DROP del padre y
-- reinserción con su nombre antes del COMMIT).
PRAGMA defer_foreign_keys = ON;

CREATE TABLE tasks_bak AS SELECT * FROM tasks;
CREATE TABLE agent_runs_bak AS SELECT * FROM agent_runs;
DROP TABLE agent_runs;
DROP TABLE tasks;

CREATE TABLE tasks (
    id             TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL REFERENCES projects(id),
    milestone_id   TEXT REFERENCES milestones(id),
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

CREATE TABLE agent_runs (
    id                   TEXT PRIMARY KEY,
    agent_id             TEXT NOT NULL REFERENCES agents(id),
    seq                  INTEGER NOT NULL,
    provider_id          TEXT NOT NULL REFERENCES providers(id),
    account_id           TEXT REFERENCES provider_accounts(id),
    model_id             TEXT NOT NULL REFERENCES models(id),
    routing_decision_id  TEXT REFERENCES routing_decisions(id),
    start_checkpoint_id  TEXT REFERENCES checkpoints(id),
    cli_session_id       TEXT,
    transcript_path      TEXT,
    pid                  INTEGER,
    status               TEXT NOT NULL CHECK (status IN ('STARTING','RUNNING','EXITED','KILLED','FAILED','HANDED_OFF')),
    end_reason           TEXT CHECK (end_reason IN ('COMPLETED','QUOTA_EXHAUSTED','RATE_LIMITED','AUTH_ERROR','CRASH','NO_HEARTBEAT','USER_SWITCH','USER_STOP','PHASE_DONE')),
    exit_code            INTEGER,
    last_heartbeat_at    INTEGER,
    started_at           INTEGER NOT NULL,
    ended_at             INTEGER,
    UNIQUE (agent_id, seq)
) STRICT;
CREATE UNIQUE INDEX agent_runs_one_open_per_agent ON agent_runs(agent_id) WHERE ended_at IS NULL;

-- Padres primero.
INSERT INTO tasks SELECT * FROM tasks_bak;
INSERT INTO agent_runs SELECT * FROM agent_runs_bak;
DROP TABLE tasks_bak;
DROP TABLE agent_runs_bak;

-- C. Recursos y procesos -------------------------------------------------------

CREATE TABLE processes (
    id                 TEXT PRIMARY KEY,
    run_id             TEXT NOT NULL REFERENCES agent_runs(id),
    tool_call_id       TEXT REFERENCES tool_calls(id),
    pid                INTEGER NOT NULL,
    parent_process_id  TEXT REFERENCES processes(id),
    command            TEXT,
    peak_rss_bytes     INTEGER,
    cpu_time_ms        INTEGER,
    started_at         INTEGER NOT NULL,
    ended_at           INTEGER
) STRICT;
CREATE INDEX processes_run ON processes(run_id);

CREATE TABLE resource_samples (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id          TEXT REFERENCES sessions(id),
    cpu_pct             REAL NOT NULL,
    ram_pct             REAL NOT NULL,
    heavy_slots_used    INTEGER NOT NULL,
    heavy_slots_total   INTEGER NOT NULL,
    process_count       INTEGER,
    performance_profile TEXT NOT NULL CHECK (performance_profile IN ('ECO','BALANCED','PERFORMANCE','CUSTOM')),
    sampled_at          INTEGER NOT NULL
) STRICT;
CREATE INDEX resource_samples_time ON resource_samples(sampled_at);

-- H. Validación y merge ---------------------------------------------------------

CREATE TABLE validation_runs (
    id           TEXT PRIMARY KEY,
    agent_id     TEXT NOT NULL REFERENCES agents(id),
    run_id       TEXT REFERENCES agent_runs(id),
    tier         INTEGER NOT NULL CHECK (tier IN (1,2,3)),
    trigger      TEXT NOT NULL CHECK (trigger IN ('AUTO','USER','MERGE_GATE','MILESTONE_GATE')),
    status       TEXT NOT NULL CHECK (status IN ('QUEUED','RUNNING','PASS','FAIL','SKIPPED')),
    tool_call_id TEXT REFERENCES tool_calls(id),
    started_at   INTEGER,
    finished_at  INTEGER
) STRICT;
CREATE INDEX validation_runs_agent ON validation_runs(agent_id);

CREATE TABLE validation_checks (
    id                 TEXT PRIMARY KEY,
    validation_run_id  TEXT NOT NULL REFERENCES validation_runs(id),
    name               TEXT NOT NULL CHECK (name IN ('syntax','format','lint','typecheck','targeted_tests','full_tests','build','security')),
    status             TEXT NOT NULL CHECK (status IN ('PASS','FAIL','WARN','SKIPPED')),
    passed_count       INTEGER,
    failed_count       INTEGER,
    summary            TEXT,
    output_object_id   TEXT REFERENCES context_objects(id)
) STRICT;
CREATE INDEX validation_checks_run ON validation_checks(validation_run_id);

CREATE TABLE merge_requests (
    id                 TEXT PRIMARY KEY,
    agent_id           TEXT NOT NULL REFERENCES agents(id),
    task_id            TEXT NOT NULL REFERENCES tasks(id),
    target_branch      TEXT NOT NULL,
    status             TEXT NOT NULL CHECK (status IN ('PREFLIGHT','READY','QUEUED','MERGED','BLOCKED','ABORTED')),
    preflight_json     TEXT CHECK (preflight_json IS NULL OR json_valid(preflight_json)),
    requires_approval  INTEGER NOT NULL DEFAULT 1 CHECK (requires_approval IN (0,1)),
    approved_at        INTEGER,
    queue_position     INTEGER,
    merge_commit       TEXT,
    created_at         INTEGER NOT NULL,
    merged_at          INTEGER
) STRICT;
CREATE INDEX merge_requests_agent ON merge_requests(agent_id);

CREATE TABLE merge_conflicts (
    id                TEXT PRIMARY KEY,
    merge_request_id  TEXT NOT NULL REFERENCES merge_requests(id),
    path              TEXT NOT NULL,
    other_agent_id    TEXT REFERENCES agents(id),
    resolution        TEXT NOT NULL CHECK (resolution IN ('PENDING','MANUAL','REVIEWER','ABORTED')),
    resolved_at       INTEGER
) STRICT;
CREATE INDEX merge_conflicts_request ON merge_conflicts(merge_request_id);

-- Un review nunca mergea solo: solo recomienda.
CREATE TABLE reviews (
    id                 TEXT PRIMARY KEY,
    target_agent_id    TEXT NOT NULL REFERENCES agents(id),
    reviewer_agent_id  TEXT NOT NULL REFERENCES agents(id),
    merge_request_id   TEXT REFERENCES merge_requests(id),
    status             TEXT NOT NULL CHECK (status IN ('RUNNING','DONE','FAILED')),
    recommendation     TEXT CHECK (recommendation IN ('APPROVE','REQUEST_CHANGES','REJECT')),
    findings_json      TEXT CHECK (findings_json IS NULL OR json_valid(findings_json)),
    created_at         INTEGER NOT NULL,
    finished_at        INTEGER
) STRICT;
CREATE INDEX reviews_target ON reviews(target_agent_id);
