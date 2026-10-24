//! Desempeño aprendido (P14.S3, ADR-0016): cada run que termina aporta, como mucho, una muestra
//! al (profile, modelo) con que se eligió. Barato: una consulta y una actualización dentro de
//! la misma transacción que cierra el run (CONSTRAINTS A11).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use symphony_core::{RunId, learning, learning::Outcome};
use symphony_store::repo::RepoError;
use symphony_store::suggestions;

/// Lo aprendido se comparte entre proyectos (ADR-0018, decisión de Leo): cada proyecto tiene su
/// base, pero las muestras van además a `~/.symphony/learning.db`. `profile_models` de cada
/// proyecto guarda el total compartido como copia de trabajo, que es lo que lee el router.
static SHARED: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Cuánto espera una escritura si otro daemon está escribiendo en la base compartida.
// Corto: se escribe dentro de la transacción que cierra un run, y esa transacción es la del único
// escritor del proyecto. Si la base compartida está ocupada se salta la muestra.
const SHARED_BUSY: Duration = Duration::from_millis(500);

/// Dónde está la base compartida (`None` = no se comparte, como en los tests de un solo proceso).
pub fn set_shared_db(path: Option<PathBuf>) {
    if let Ok(mut s) = SHARED.lock() {
        *s = path;
    }
}

fn shared_path() -> Option<PathBuf> {
    SHARED.lock().ok().and_then(|s| s.clone())
}

/// ponytail: una conexión por operación; basta para una muestra por run. Si hiciera falta más,
/// guardar la conexión.
pub(crate) fn open_shared(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(SHARED_BUSY)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE IF NOT EXISTS samples (
             run_id  TEXT PRIMARY KEY,
             profile TEXT NOT NULL,
             model   TEXT NOT NULL,
             score   REAL NOT NULL
         );
         CREATE INDEX IF NOT EXISTS samples_pair ON samples (profile, model);",
    )?;
    Ok(conn)
}

fn shared_total(conn: &Connection, profile: &str, model: &str) -> rusqlite::Result<(i64, f64)> {
    conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(score), 0.0) FROM samples WHERE profile = ?1 AND model = ?2",
        params![profile, model],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
}

/// Pone el total compartido de (profile, modelo) en `profile_models` y recalcula el ajuste.
fn apply_total(
    conn: &Connection,
    profile: &str,
    model: &str,
    n: i64,
    sum: f64,
) -> Result<(), RepoError> {
    conn.execute(
        "UPDATE profile_models
         SET sample_count = ?3, outcome_score_sum = ?4, learned_adjustment = ?5
         WHERE profile_id = ?1 AND model_id = ?2 AND sample_count <= ?3",
        params![profile, model, n, sum, learning::learned_adjustment(n, sum)],
    )?;
    Ok(())
}

/// Al arrancar el daemon: lo que otros proyectos ya aprendieron llega a este. Ninguna falla de la
/// base compartida impide arrancar.
pub fn sync_from_shared(conn: &Connection) {
    let Some(path) = shared_path() else { return };
    let run = || -> Result<(), Box<dyn std::error::Error>> {
        let shared = open_shared(&path)?;
        let mut stmt = shared.prepare(
            "SELECT profile, model, COUNT(*), SUM(score) FROM samples GROUP BY profile, model",
        )?;
        let rows: Vec<(String, String, i64, f64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<_, _>>()?;
        for (profile, model, n, sum) in rows {
            apply_total(conn, &profile, &model, n, sum)?;
        }
        Ok(())
    };
    if let Err(e) = run() {
        tracing::warn!(error = %e, "no se pudo leer lo aprendido por otros proyectos");
    }
}

/// Anota el resultado del run en `profile_models`. Sin profile (un modelo exacto que pidió el
/// usuario) o con un final que no habla del modelo (cuota, auth, cambio o parada del usuario,
/// fase terminada) no hay muestra.
pub fn on_run_finished(conn: &Connection, run: RunId) -> Result<(), RepoError> {
    let row: Option<(String, Option<String>, String, Option<String>)> = conn
        .query_row(
            "SELECT r.model_id, r.end_reason, r.agent_id, d.profile_id
             FROM agent_runs r LEFT JOIN routing_decisions d ON d.id = r.routing_decision_id
             WHERE r.id = ?1",
            params![run.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((model, Some(end), agent, Some(profile))) = row else {
        return Ok(());
    };
    let outcome = match end.as_str() {
        "COMPLETED" => {
            let retries: i64 = conn.query_row(
                "SELECT COUNT(*) FROM validation_runs WHERE agent_id = ?1 AND run_id = ?2
                 AND status = 'FAIL'",
                params![agent, run.to_string()],
                |r| r.get(0),
            )?;
            if retries > 0 {
                Outcome::NeededRetries
            } else {
                Outcome::Clean
            }
        }
        // `NO_HEARTBEAT` (una laptop dormida, la red) habla más del entorno que del modelo.
        "CRASH" => Outcome::Failed,
        _ => return Ok(()),
    };
    suggestions::record_outcome(conn, &run.to_string(), &profile, &model, outcome)?;
    share(conn, run, &profile, &model, outcome);
    Ok(())
}

/// Suma la muestra a la base compartida y trae de vuelta el total (con lo de otros proyectos).
/// Si la base compartida falla, queda lo local: aprender no puede romper el cierre de un run.
fn share(conn: &Connection, run: RunId, profile: &str, model: &str, outcome: Outcome) {
    let Some(path) = shared_path() else { return };
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let shared = open_shared(&path)?;
        // Idempotente por run, como en la base del proyecto.
        shared.execute(
            "INSERT OR IGNORE INTO samples (run_id, profile, model, score) VALUES (?1, ?2, ?3, ?4)",
            params![run.to_string(), profile, model, outcome.score()],
        )?;
        let (n, sum) = shared_total(&shared, profile, model)?;
        apply_total(conn, profile, model, n, sum)?;
        Ok(())
    })();
    if let Err(e) = result {
        tracing::warn!(error = %e, "no se pudo compartir lo aprendido; queda solo en este proyecto");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE profile_models (
                 profile_id TEXT, model_id TEXT, sample_count INTEGER NOT NULL DEFAULT 0,
                 outcome_score_sum REAL NOT NULL DEFAULT 0, learned_adjustment REAL);
             INSERT INTO profile_models (profile_id, model_id) VALUES ('@code', 'm');",
        )
        .unwrap();
        c
    }

    fn row(c: &Connection) -> (i64, Option<f64>) {
        c.query_row(
            "SELECT sample_count, learned_adjustment FROM profile_models",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    /// Lo que un proyecto aprende llega a otro: por cada muestra en vivo y al arrancar.
    #[test]
    fn what_one_project_learns_reaches_the_others() {
        let dir = tempfile::tempdir().unwrap();
        set_shared_db(Some(dir.path().join("learning.db")));
        let (a, b) = (project_db(), project_db());
        for _ in 0..learning::MIN_SAMPLES {
            share(&a, RunId::new(), "@code", "m", Outcome::Clean);
        }
        // El proyecto que hizo el trabajo ve el total y ya tiene ajuste (media 1,0 > 0,75).
        let (n, adj) = row(&a);
        assert_eq!(n, learning::MIN_SAMPLES);
        assert!(adj.is_some_and(|x| x > 0.0), "{adj:?}");
        // El otro, recién abierto, lo recibe al sincronizar.
        assert_eq!(row(&b), (0, None));
        sync_from_shared(&b);
        assert_eq!(row(&b), row(&a));
        // Repetir la misma muestra no cuenta dos veces.
        let run = RunId::new();
        share(&b, run, "@code", "m", Outcome::Failed);
        share(&b, run, "@code", "m", Outcome::Failed);
        assert_eq!(row(&b).0, learning::MIN_SAMPLES + 1);
    }

    /// Lo que un proyecto aprendió antes de compartir no se pierde si el total compartido es menor.
    #[test]
    fn the_shared_total_never_makes_a_project_forget() {
        let dir = tempfile::tempdir().unwrap();
        set_shared_db(Some(dir.path().join("learning.db")));
        let a = project_db();
        a.execute(
            "UPDATE profile_models SET sample_count = 40, outcome_score_sum = 30.0",
            [],
        )
        .unwrap();
        share(&a, RunId::new(), "@code", "m", Outcome::Clean);
        sync_from_shared(&a);
        assert_eq!(row(&a).0, 40);
    }
}
