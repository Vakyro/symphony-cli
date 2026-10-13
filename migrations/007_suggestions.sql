-- 007 · Sugerencias de modelo y desempeño aprendido (P14, DB §3.E, IDEA §5.7).
--
-- `model_suggestions`: cambios que Symphony *propone* (nunca aplica por su cuenta).
-- `profile_models.outcome_score_sum`: suma de los resultados observados por (profile, modelo);
-- junto con `sample_count` da la media de la que sale `learned_adjustment` (ADR-0016).

CREATE TABLE model_suggestions (
    id                  TEXT PRIMARY KEY,
    agent_id            TEXT NOT NULL REFERENCES agents(id),
    current_model_id    TEXT NOT NULL REFERENCES models(id),
    suggested_model_id  TEXT NOT NULL REFERENCES models(id),
    reason              TEXT NOT NULL,
    confidence          REAL CHECK (confidence IS NULL OR confidence BETWEEN 0 AND 1),
    status              TEXT NOT NULL CHECK (status IN ('PENDING','ACCEPTED','KEPT','MUTED')),
    created_at          INTEGER NOT NULL,
    resolved_at         INTEGER
) STRICT;
CREATE INDEX model_suggestions_agent ON model_suggestions(agent_id, status);
-- Una sola pendiente por agente.
CREATE UNIQUE INDEX model_suggestions_one_pending ON model_suggestions(agent_id) WHERE status = 'PENDING';

ALTER TABLE profile_models ADD COLUMN outcome_score_sum REAL NOT NULL DEFAULT 0;

-- Una muestra por run: si un camino de cierre llama dos veces, la segunda no cuenta.
CREATE TABLE learning_samples (
    run_id  TEXT PRIMARY KEY
) STRICT;
