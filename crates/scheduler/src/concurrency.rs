//! Cuántos agentes conviene tener corriendo a la vez según la cuota que queda (F4,
//! ADR-0013). La oleada 2 de Signalia agotó la cuota en 22 minutos por lanzar 5 agentes a
//! ciegas; aquí el tope del lote baja cuando la cuota conocida baja.
//!
//! Solo cuenta la cuota **conocida** (`quota_certainty = KNOWN`): sin dato, no se inventa
//! un recorte (DB §3.D: nunca falsa precisión) y el tope del lote queda como está.

/// Fracción de cuota restante (0.0–1.0) por debajo de la cual se recorta.
const HALF: f64 = 0.50;
const QUARTER: f64 = 0.25;
const LOW: f64 = 0.10;

/// `max_agents`: el tope que pidió el lote o el perfil. `known_remaining`: la fracción
/// restante de cada proveedor que trabaja en el lote y cuya cuota se conoce.
///
/// - `≥ 50 %` → el tope completo.
/// - `≥ 25 %` → la mitad (redondeada hacia arriba).
/// - `≥ 10 %` → uno.
/// - `< 10 %` → ninguno nuevo: se espera al reinicio de la ventana.
pub fn agent_concurrency(max_agents: usize, known_remaining: &[f64]) -> usize {
    let Some(lowest) = known_remaining
        .iter()
        .copied()
        .filter(|f| f.is_finite())
        .map(|f| f.clamp(0.0, 1.0))
        .min_by(f64::total_cmp)
    else {
        return max_agents;
    };
    if lowest >= HALF {
        max_agents
    } else if lowest >= QUARTER {
        max_agents.div_ceil(2)
    } else if lowest >= LOW {
        max_agents.min(1)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_known_quota_keeps_the_cap() {
        assert_eq!(agent_concurrency(5, &[]), 5);
        assert_eq!(agent_concurrency(5, &[f64::NAN]), 5);
    }

    #[test]
    fn the_scarcest_known_provider_sets_the_pace() {
        assert_eq!(agent_concurrency(5, &[0.9, 0.8]), 5);
        assert_eq!(agent_concurrency(5, &[0.9, 0.4]), 3);
        assert_eq!(agent_concurrency(4, &[0.3]), 2);
        assert_eq!(agent_concurrency(5, &[0.9, 0.15]), 1);
        assert_eq!(agent_concurrency(5, &[0.05]), 0);
        assert_eq!(agent_concurrency(0, &[0.9]), 0);
    }

    #[test]
    fn never_exceeds_the_cap_and_never_grows_as_quota_drops() {
        let mut last = usize::MAX;
        for pct in (0..=100).rev() {
            let n = agent_concurrency(5, &[f64::from(pct) / 100.0]);
            assert!(n <= 5 && n <= last, "{pct} % → {n}");
            last = n;
        }
        // Valores fuera de rango no rompen nada.
        assert_eq!(agent_concurrency(3, &[7.0]), 3);
        assert_eq!(agent_concurrency(3, &[-1.0]), 0);
    }
}
