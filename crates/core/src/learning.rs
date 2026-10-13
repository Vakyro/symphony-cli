//! Desempeño aprendido (P14.S3, ADR-0016): el ajuste que los resultados observados le dan al
//! puntaje base de un modelo en un profile. Puro y determinista: ninguna muestra, ningún LLM.
//!
//! `ajuste = clamp((media − BASELINE) · GAIN, ±MAX)`, y solo se aplica con al menos
//! [`MIN_SAMPLES`] muestras (IDEA §5.7: «un mínimo de muestras antes de influir en el router»).

/// Muestras mínimas por (profile, modelo) antes de que el ajuste influya.
pub const MIN_SAMPLES: i64 = 10;
/// Resultado medio con el que un modelo «cumple lo esperado»: ni sube ni baja.
pub const BASELINE: f64 = 0.75;
/// Cuánto pesa la desviación de la media respecto de la línea base.
pub const GAIN: f64 = 0.4;
/// Tope del ajuste (en puntaje 0–1): lo aprendido corrige, no manda.
pub const MAX_ADJUSTMENT: f64 = 0.10;

/// Cómo terminó un run, para aprender de él. Lo que no habla del modelo (cuota, auth, el
/// usuario cambió o detuvo, fase terminada) no es una muestra.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Completó la tarea y pasó la validación a la primera (o no había validación).
    Clean,
    /// Completó, pero hizo falta devolverle fallos de validación.
    NeededRetries,
    /// El run murió (crash, sin latido) o la tarea falló.
    Failed,
}

impl Outcome {
    /// El resultado del run en 0–1.
    pub fn score(self) -> f64 {
        match self {
            Self::Clean => 1.0,
            Self::NeededRetries => 0.5,
            Self::Failed => 0.0,
        }
    }
}

/// El ajuste aprendido, o `None` si todavía no hay muestras suficientes (o los datos no valen).
pub fn learned_adjustment(samples: i64, score_sum: f64) -> Option<f64> {
    if samples < MIN_SAMPLES || !score_sum.is_finite() {
        return None;
    }
    let mean = (score_sum / samples as f64).clamp(0.0, 1.0);
    Some(((mean - BASELINE) * GAIN).clamp(-MAX_ADJUSTMENT, MAX_ADJUSTMENT))
}

/// El puntaje con el que compite un modelo: base + ajuste aprendido (si aplica), en 0–1.
pub fn effective_score(base: f64, samples: i64, score_sum: f64) -> f64 {
    (base + learned_adjustment(samples, score_sum).unwrap_or(0.0)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_learned_before_the_minimum_of_samples() {
        assert_eq!(learned_adjustment(0, 0.0), None);
        assert_eq!(learned_adjustment(MIN_SAMPLES - 1, 9.0), None);
        assert_eq!(effective_score(0.6, MIN_SAMPLES - 1, 9.0), 0.6);
    }

    #[test]
    fn a_model_that_meets_expectations_is_left_alone() {
        // media = BASELINE
        let sum = BASELINE * MIN_SAMPLES as f64;
        assert!(learned_adjustment(MIN_SAMPLES, sum).unwrap().abs() < 1e-9);
    }

    #[test]
    fn good_results_raise_and_bad_results_lower_within_the_cap() {
        let up = learned_adjustment(20, 20.0).unwrap();
        let down = learned_adjustment(20, 0.0).unwrap();
        assert!(up > 0.0 && up <= MAX_ADJUSTMENT, "{up}");
        assert!((-MAX_ADJUSTMENT..0.0).contains(&down), "{down}");
        // El tope vale con cualquier cantidad de muestras.
        assert_eq!(learned_adjustment(10_000, 10_000.0), Some(0.1));
        assert_eq!(learned_adjustment(10_000, 0.0), Some(-0.1));
    }

    #[test]
    fn the_effective_score_stays_in_the_unit_interval_and_bad_data_is_ignored() {
        assert_eq!(effective_score(0.98, 50, 50.0), 1.0);
        assert_eq!(effective_score(0.03, 50, 0.0), 0.0);
        assert_eq!(learned_adjustment(50, f64::NAN), None);
        assert_eq!(learned_adjustment(50, f64::INFINITY), None);
    }

    #[test]
    fn outcomes_are_ordered() {
        assert!(Outcome::Clean.score() > Outcome::NeededRetries.score());
        assert!(Outcome::NeededRetries.score() > Outcome::Failed.score());
    }
}
