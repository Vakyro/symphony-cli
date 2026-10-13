-- 006 · Plugins (P12, ADR-0015): `plugin` como nombre de un check de validación.
-- Un validator externo produce un check que no es lint, typecheck ni tests. SQLite no cambia
-- un CHECK: se reconstruye la tabla con el patrón de 002–005.

PRAGMA defer_foreign_keys = ON;

CREATE TABLE validation_checks_bak AS SELECT * FROM validation_checks;
DROP TABLE validation_checks;

CREATE TABLE validation_checks (
    id                 TEXT PRIMARY KEY,
    validation_run_id  TEXT NOT NULL REFERENCES validation_runs(id),
    name               TEXT NOT NULL CHECK (name IN ('syntax','format','lint','typecheck','targeted_tests','full_tests','build','security','plugin')),
    status             TEXT NOT NULL CHECK (status IN ('PASS','FAIL','WARN','SKIPPED')),
    passed_count       INTEGER,
    failed_count       INTEGER,
    summary            TEXT,
    output_object_id   TEXT REFERENCES context_objects(id)
) STRICT;
CREATE INDEX validation_checks_run ON validation_checks(validation_run_id);

INSERT INTO validation_checks SELECT * FROM validation_checks_bak;
DROP TABLE validation_checks_bak;
