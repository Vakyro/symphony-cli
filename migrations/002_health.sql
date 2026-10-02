-- 002 · Salud, uso y routing (Fase 4 de DB §6, ejecutada tras P11 por ADR-0010).
-- Copia fiel de docs/spec/symphony_database.md §3.D y §3.E, más las FKs que 001 dejó
-- pendientes (`provider_accounts`, `routing_decisions`, `profiles`).
--
-- Desviación de DB §3.D: UNIQUE(provider_id, account_id, model_id) no impide dos filas de
-- nivel proveedor (model_id NULL: en SQLite los NULL son distintos); se usa un índice único
-- con COALESCE, que es más estricto.

-- D. Salud y uso ------------------------------------------------------------

CREATE TABLE provider_accounts (
    id               TEXT PRIMARY KEY,
    provider_id      TEXT NOT NULL REFERENCES providers(id),
    label            TEXT NOT NULL,
    auth_status      TEXT NOT NULL CHECK (auth_status IN ('OK','EXPIRED','MISSING','UNKNOWN')),
    last_checked_at  INTEGER
) STRICT;
CREATE UNIQUE INDEX provider_accounts_label ON provider_accounts(provider_id, label);

-- Sin tokens ni credenciales: una cuenta «default» por proveedor ya detectado.
INSERT INTO provider_accounts (id, provider_id, label, auth_status)
    SELECT 'acct-' || id, id, 'default', 'UNKNOWN' FROM providers;

CREATE TABLE provider_health (
    id               TEXT PRIMARY KEY,
    provider_id      TEXT NOT NULL REFERENCES providers(id),
    account_id       TEXT REFERENCES provider_accounts(id),
    model_id         TEXT REFERENCES models(id),
    state            TEXT NOT NULL CHECK (state IN ('HEALTHY','DEGRADED','THROTTLED','RATE_LIMITED','QUOTA_LOW','EXHAUSTED','AUTH_ERROR','OFFLINE','UNKNOWN','PROBING')),
    quota_certainty  TEXT NOT NULL CHECK (quota_certainty IN ('KNOWN','ESTIMATED','UNKNOWN')),
    quota_remaining  REAL CHECK (quota_remaining BETWEEN 0 AND 1),
    evidence         TEXT,
    retry_after_at   INTEGER,
    reset_at         INTEGER,
    confidence       REAL CHECK (confidence BETWEEN 0 AND 1),
    updated_at       INTEGER NOT NULL,
    -- Nunca con falsa precisión: el porcentaje solo existe si el CLI lo informa.
    CHECK (quota_remaining IS NULL OR quota_certainty = 'KNOWN')
) STRICT;
CREATE UNIQUE INDEX provider_health_scope
    ON provider_health(provider_id, COALESCE(account_id, ''), COALESCE(model_id, ''));

CREATE TABLE usage_records (
    id           TEXT PRIMARY KEY,
    run_id       TEXT NOT NULL REFERENCES agent_runs(id),
    provider_id  TEXT NOT NULL REFERENCES providers(id),
    model_id     TEXT NOT NULL REFERENCES models(id),
    tokens_in    INTEGER,
    tokens_out   INTEGER,
    source       TEXT NOT NULL CHECK (source IN ('REPORTED','ESTIMATED')),
    recorded_at  INTEGER NOT NULL
) STRICT;
CREATE INDEX usage_records_provider_time ON usage_records(provider_id, recorded_at);
CREATE INDEX usage_records_run ON usage_records(run_id);

-- E. Routing -----------------------------------------------------------------

CREATE TABLE profiles (
    id            TEXT PRIMARY KEY,
    description   TEXT NOT NULL,
    weights_json  TEXT NOT NULL CHECK (json_valid(weights_json)),
    builtin       INTEGER NOT NULL DEFAULT 1 CHECK (builtin IN (0,1))
) STRICT;

-- Pesos iniciales (bootstrap, editables): fit, context, health, quota, scarcity, failures, load, speed.
INSERT INTO profiles (id, description, weights_json, builtin) VALUES
    ('@code',      'Best available for implementation',  '{"fit":1.0,"context":0.3,"health":0.5,"quota":0.4,"scarcity":0.4,"failures":0.5,"load":0.2,"speed":0.1}', 1),
    ('@debug',     'Best available for debugging',       '{"fit":1.0,"context":0.5,"health":0.5,"quota":0.3,"scarcity":0.3,"failures":0.6,"load":0.2,"speed":0.1}', 1),
    ('@fast',      'Prioritize response speed',          '{"fit":0.4,"context":0.1,"health":0.5,"quota":0.3,"scarcity":0.2,"failures":0.4,"load":0.3,"speed":1.0}', 1),
    ('@reasoning', 'Prioritize difficult reasoning',     '{"fit":1.2,"context":0.4,"health":0.4,"quota":0.2,"scarcity":0.2,"failures":0.4,"load":0.1,"speed":0.0}', 1),
    ('@docs',      'Documentation-oriented',             '{"fit":0.8,"context":0.3,"health":0.4,"quota":0.5,"scarcity":0.5,"failures":0.4,"load":0.2,"speed":0.2}', 1),
    ('@review',    'Careful review of changes',          '{"fit":1.0,"context":0.5,"health":0.5,"quota":0.3,"scarcity":0.3,"failures":0.5,"load":0.2,"speed":0.0}', 1),
    ('@conserve',  'Preserve scarce quota',              '{"fit":0.5,"context":0.2,"health":0.5,"quota":1.0,"scarcity":1.0,"failures":0.5,"load":0.3,"speed":0.2}', 1);

CREATE TABLE profile_models (
    profile_id          TEXT NOT NULL REFERENCES profiles(id),
    model_id            TEXT NOT NULL REFERENCES models(id),
    base_score          REAL NOT NULL CHECK (base_score BETWEEN 0 AND 1),
    learned_adjustment  REAL,
    sample_count        INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (profile_id, model_id)
) STRICT;

CREATE TABLE routing_decisions (
    id                 TEXT PRIMARY KEY,
    agent_id           TEXT NOT NULL REFERENCES agents(id),
    trigger            TEXT NOT NULL CHECK (trigger IN ('SPAWN','FAILOVER','SWITCH','SUGGESTION')),
    profile_id         TEXT REFERENCES profiles(id),
    selected_model_id  TEXT REFERENCES models(id),
    engine             TEXT NOT NULL CHECK (engine IN ('RULES','DECISION_MODEL')),
    explanation        TEXT,
    decided_at         INTEGER NOT NULL
) STRICT;
CREATE INDEX routing_decisions_agent ON routing_decisions(agent_id, decided_at);

CREATE TABLE routing_candidates (
    decision_id    TEXT NOT NULL REFERENCES routing_decisions(id),
    model_id       TEXT NOT NULL REFERENCES models(id),
    eligible       INTEGER NOT NULL CHECK (eligible IN (0,1)),
    reject_reason  TEXT CHECK (reject_reason IN ('OFFLINE','AUTH','EXHAUSTED','CONTEXT','CAPABILITY','COOLDOWN','RESERVE','DISABLED')),
    score          REAL,
    factors_json   TEXT CHECK (factors_json IS NULL OR json_valid(factors_json)),
    PRIMARY KEY (decision_id, model_id),
    CHECK (eligible = 1 OR reject_reason IS NOT NULL)
) STRICT;

-- FKs pendientes de 001 ------------------------------------------------------
-- SQLite no puede añadir una FK a una columna existente: se reconstruyen `agents`,
-- `agent_runs` y `provider_failures` dentro de la transacción de la migración. Con
-- `defer_foreign_keys` el DROP de un padre no falla: las filas hijas cuentan como
-- violaciones pendientes y se resuelven al volver a insertar al padre, antes del COMMIT.

PRAGMA defer_foreign_keys = ON;

CREATE TABLE agents_bak AS SELECT * FROM agents;
CREATE TABLE agent_runs_bak AS SELECT * FROM agent_runs;
CREATE TABLE provider_failures_bak AS SELECT * FROM provider_failures;
DROP TABLE agent_runs;
DROP TABLE provider_failures;
DROP TABLE agents;

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
    requested_profile_id  TEXT REFERENCES profiles(id),
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
CREATE UNIQUE INDEX agents_one_active_per_task ON agents(task_id)
    WHERE state NOT IN ('COMPLETED','FAILED','CANCELLED');
CREATE INDEX agents_project_state ON agents(project_id, state);

CREATE TABLE provider_failures (
    id              TEXT PRIMARY KEY,
    provider_id     TEXT NOT NULL REFERENCES providers(id),
    account_id      TEXT REFERENCES provider_accounts(id),
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
    end_reason           TEXT CHECK (end_reason IN ('COMPLETED','QUOTA_EXHAUSTED','RATE_LIMITED','AUTH_ERROR','CRASH','NO_HEARTBEAT','USER_SWITCH','USER_STOP')),
    exit_code            INTEGER,
    last_heartbeat_at    INTEGER,
    started_at           INTEGER NOT NULL,
    ended_at             INTEGER,
    UNIQUE (agent_id, seq)
) STRICT;
CREATE UNIQUE INDEX agent_runs_one_open_per_agent ON agent_runs(agent_id) WHERE ended_at IS NULL;

-- Orden de reinserción: padres primero. Las filas antiguas quedan con la cuenta «default».
INSERT INTO agents SELECT * FROM agents_bak;
INSERT INTO provider_failures
    (id, provider_id, account_id, model_id, run_id, failure_type, raw_code, message, retry_after_at, reset_at, confidence, occurred_at)
    SELECT id, provider_id, 'acct-' || provider_id, model_id, run_id, failure_type, raw_code, message, retry_after_at, reset_at, confidence, occurred_at
    FROM provider_failures_bak;
INSERT INTO agent_runs
    (id, agent_id, seq, provider_id, account_id, model_id, routing_decision_id, start_checkpoint_id, cli_session_id, transcript_path, pid, status, end_reason, exit_code, last_heartbeat_at, started_at, ended_at)
    SELECT id, agent_id, seq, provider_id, 'acct-' || provider_id, model_id, routing_decision_id, start_checkpoint_id, cli_session_id, transcript_path, pid, status, end_reason, exit_code, last_heartbeat_at, started_at, ended_at
    FROM agent_runs_bak;

DROP TABLE agents_bak;
DROP TABLE agent_runs_bak;
DROP TABLE provider_failures_bak;
