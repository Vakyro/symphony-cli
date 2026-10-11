-- 005 · Validación (P08.S6): `VALIDATION_FAILED` como tipo de recovery item.
-- Una validación que falla N veces deja la tarea bloqueada con una decisión pendiente
-- (FLOW §15.2): no encaja en ninguno de los tipos anteriores. SQLite no cambia un CHECK:
-- se reconstruye la tabla con el patrón de 002/003/004.

PRAGMA defer_foreign_keys = ON;

CREATE TABLE recovery_items_bak AS SELECT * FROM recovery_items;
DROP TABLE recovery_items;

CREATE TABLE recovery_items (
    id                TEXT PRIMARY KEY,
    project_id        TEXT NOT NULL REFERENCES projects(id),
    agent_id          TEXT REFERENCES agents(id),
    run_id            TEXT REFERENCES agent_runs(id),
    kind              TEXT NOT NULL CHECK (kind IN ('EXECUTOR_EXITED','NO_HEARTBEAT','RATE_LIMITED','AUTH_ERROR','CHECKPOINT_INVALID','WORKSPACE_DAMAGED','MERGE_CONFLICT','DEPENDENCY_FAILED','SESSION_INTERRUPTED','MACHINE_PRESSURE','VALIDATION_FAILED')),
    detail            TEXT NOT NULL,
    checkpoint_valid  INTEGER CHECK (checkpoint_valid IN (0,1)),
    workspace_intact  INTEGER CHECK (workspace_intact IN (0,1)),
    status            TEXT NOT NULL CHECK (status IN ('OPEN','RESOLVED','DISMISSED')),
    resolution        TEXT CHECK (resolution IN ('RESTART','FAILOVER','REBUILD_FROM_WORKSPACE','OLDER_CHECKPOINT','RECLAIM','ARCHIVE','MANUAL')),
    created_at        INTEGER NOT NULL,
    resolved_at       INTEGER
) STRICT;
CREATE INDEX recovery_items_open ON recovery_items(project_id) WHERE status = 'OPEN';

INSERT INTO recovery_items SELECT * FROM recovery_items_bak;
DROP TABLE recovery_items_bak;
