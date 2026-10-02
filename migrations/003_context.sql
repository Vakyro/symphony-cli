-- 003 · Context Engine (Fase 3 de DB §6, P09): chunks + FTS5, items de handoff, retrievals,
-- hechos consolidados, skills y MCP. Copia fiel de docs/spec/symphony_database.md §3.G y §3.J.

-- G. Contexto ----------------------------------------------------------------

CREATE TABLE context_chunks (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    object_id   TEXT NOT NULL REFERENCES context_objects(id),
    seq         INTEGER NOT NULL,
    start_line  INTEGER,
    end_line    INTEGER,
    text        TEXT NOT NULL,
    UNIQUE (object_id, seq)
) STRICT;
CREATE INDEX context_chunks_object ON context_chunks(object_id, seq);

-- Búsqueda BM25 sobre los chunks, sin vector DB. Contenido externo: el índice no duplica el
-- texto; los triggers lo mantienen al día con la tabla de chunks.
CREATE VIRTUAL TABLE context_fts USING fts5(text, content='context_chunks', content_rowid='id', tokenize='unicode61');

CREATE TRIGGER context_chunks_ai AFTER INSERT ON context_chunks BEGIN
    INSERT INTO context_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER context_chunks_ad AFTER DELETE ON context_chunks BEGIN
    INSERT INTO context_fts(context_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
CREATE TRIGGER context_chunks_au AFTER UPDATE OF text ON context_chunks BEGIN
    INSERT INTO context_fts(context_fts, rowid, text) VALUES ('delete', old.id, old.text);
    INSERT INTO context_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TABLE handoff_items (
    id          TEXT PRIMARY KEY,
    handoff_id  TEXT NOT NULL REFERENCES handoffs(id),
    section     TEXT NOT NULL CHECK (section IN ('OBJECTIVE','PLAN','DECISIONS','FAILURES','CODE','DIFF','REFERENCES')),
    object_id   TEXT REFERENCES context_objects(id),
    path        TEXT,
    fidelity    INTEGER CHECK (fidelity BETWEEN 0 AND 5),
    tokens      INTEGER NOT NULL
) STRICT;
CREATE INDEX handoff_items_handoff ON handoff_items(handoff_id);

CREATE TABLE context_retrievals (
    id               TEXT PRIMARY KEY,
    run_id           TEXT NOT NULL REFERENCES agent_runs(id),
    object_id        TEXT NOT NULL REFERENCES context_objects(id),
    operation        TEXT NOT NULL CHECK (operation IN ('RETRIEVE','SEARCH','LINES')),
    query            TEXT,
    tokens_returned  INTEGER,
    found            INTEGER NOT NULL CHECK (found IN (0,1)),  -- 0 = retrieval miss
    requested_at     INTEGER NOT NULL
) STRICT;
CREATE INDEX context_retrievals_run ON context_retrievals(run_id, requested_at);

CREATE TABLE project_facts (
    id                TEXT PRIMARY KEY,
    project_id        TEXT NOT NULL REFERENCES projects(id),
    key               TEXT NOT NULL,
    value             TEXT NOT NULL,
    kind              TEXT NOT NULL CHECK (kind IN ('ARCHITECTURE','DECISION','CONSTRAINT','CONVENTION')),
    status            TEXT NOT NULL CHECK (status IN ('CURRENT','SUPERSEDED','CONFLICT')),
    superseded_by_id  TEXT REFERENCES project_facts(id),
    source_agent_id   TEXT REFERENCES agents(id),
    source_event_id   INTEGER REFERENCES events(id),
    created_at        INTEGER NOT NULL
) STRICT;
-- Un solo valor vigente por clave; `CONFLICT` alimenta la rama «contexto inconsistente».
CREATE UNIQUE INDEX project_facts_one_current ON project_facts(project_id, key) WHERE status = 'CURRENT';
CREATE INDEX project_facts_project ON project_facts(project_id, status);

-- J. Skills y MCP -------------------------------------------------------------

CREATE TABLE skills (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    scope         TEXT NOT NULL CHECK (scope IN ('BUILTIN','USER','PROJECT')),
    project_id    TEXT REFERENCES projects(id),
    path          TEXT NOT NULL,
    content_hash  TEXT,
    enabled       INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    updated_at    INTEGER NOT NULL,
    CHECK (scope <> 'PROJECT' OR project_id IS NOT NULL)
) STRICT;
CREATE UNIQUE INDEX skills_scope_name ON skills(scope, COALESCE(project_id, ''), name);

CREATE TABLE mcp_servers (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    scope       TEXT NOT NULL CHECK (scope IN ('USER','PROJECT')),
    project_id  TEXT REFERENCES projects(id),
    transport   TEXT NOT NULL CHECK (transport IN ('STDIO','HTTP')),
    share_mode  TEXT NOT NULL CHECK (share_mode IN ('SHARED','PER_CLIENT')),
    command     TEXT,
    url         TEXT,
    enabled     INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    status      TEXT CHECK (status IN ('RUNNING','STOPPED','ERROR')),
    CHECK (scope <> 'PROJECT' OR project_id IS NOT NULL)
) STRICT;
CREATE UNIQUE INDEX mcp_servers_scope_name ON mcp_servers(scope, COALESCE(project_id, ''), name);
