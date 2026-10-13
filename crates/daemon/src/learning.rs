//! Desempeño aprendido (P14.S3, ADR-0016): cada run que termina aporta, como mucho, una muestra
//! al (profile, modelo) con que se eligió. Barato: una consulta y una actualización dentro de
//! la misma transacción que cierra el run (CONSTRAINTS A11).

use rusqlite::{Connection, OptionalExtension, params};
use symphony_core::{RunId, learning::Outcome};
use symphony_store::repo::RepoError;
use symphony_store::suggestions;

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
    suggestions::record_outcome(conn, &run.to_string(), &profile, &model, outcome)
}
