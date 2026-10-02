//! Máquina de estados de la salud de un proveedor (IDEA §5.8, DB §3.D, FLOW §12).
//!
//! Es una función pura: dado el estado anterior y un evento devuelve el siguiente. No hay
//! temporizadores: los cooldowns se guardan como instantes (`retry_after_at`, `reset_at`) y se
//! resuelven al leer con [`Health::effective_state`] (p. ej. `RATE_LIMITED` → `PROBING`).
//!
//! Reglas que no se rompen: un 429 **no** es «agotado»; el porcentaje de cuota solo existe si
//! el CLI lo informa (`QuotaCertainty::Known`); y un fallo de login no se reintenta solo.

use crate::{FailureType, ProviderState, QuotaCertainty};

/// Espera tras un límite temporal (RPM, TPM, 429) si el CLI no dice cuánto.
pub const RATE_LIMIT_COOLDOWN_MS: i64 = 60_000;
/// Espera tras varios límites temporales seguidos.
pub const THROTTLED_COOLDOWN_MS: i64 = 300_000;
/// Espera tras un fallo de red.
pub const NETWORK_COOLDOWN_MS: i64 = 30_000;
/// Espera tras un error genérico del proveedor.
pub const DEGRADED_COOLDOWN_MS: i64 = 120_000;
/// Un modelo que el CLI dice no tener se vuelve a mirar tras una hora.
pub const MODEL_UNAVAILABLE_COOLDOWN_MS: i64 = 3_600_000;
/// Una cuota agotada sin hora de reinicio se vuelve a sondear a esta cadencia.
pub const EXHAUSTED_PROBE_MS: i64 = 1_800_000;
/// Fallos temporales o genéricos recientes a partir de los cuales el estado pasa a `THROTTLED`.
pub const THROTTLE_AFTER: u32 = 3;
/// Fallos de red recientes a partir de los cuales el proveedor se considera `OFFLINE`.
pub const OFFLINE_AFTER: u32 = 2;

/// Estado de salud de un alcance (proveedor, o proveedor + modelo).
#[derive(Debug, Clone, PartialEq)]
pub struct Health {
    pub state: ProviderState,
    pub certainty: QuotaCertainty,
    /// 0–1. Solo con `certainty == Known` (la base lo exige con un CHECK).
    pub remaining: Option<f64>,
    pub retry_after_at: Option<i64>,
    pub reset_at: Option<i64>,
    pub evidence: Option<String>,
    pub updated_at: i64,
}

/// Lo que puede cambiar la salud.
#[derive(Debug, Clone, PartialEq)]
pub enum HealthEvent {
    /// Un turno terminó bien.
    Success,
    /// Un error ya clasificado por el adapter. `recent_same`: cuántos del mismo tipo hubo
    /// en los últimos minutos (lo cuenta quien llama, desde `provider_failures`).
    Failure {
        kind: FailureType,
        retry_after_at: Option<i64>,
        reset_at: Option<i64>,
        recent_same: u32,
    },
    /// Cuota que el CLI informó (`used_fraction` de su ventana más apretada) y la reserva
    /// configurada (`reserve`, 0–1).
    Quota {
        used_fraction: f64,
        reset_at: Option<i64>,
        reserve: f64,
    },
    /// El usuario volvió a iniciar sesión o a detectar el CLI: se olvida lo anterior.
    Reset,
}

impl Health {
    /// Sin información: el proveedor se trata como disponible.
    pub fn unknown(now: i64) -> Self {
        Self {
            state: ProviderState::Unknown,
            certainty: QuotaCertainty::Unknown,
            remaining: None,
            retry_after_at: None,
            reset_at: None,
            evidence: None,
            updated_at: now,
        }
    }

    /// Estado siguiente. Nunca falla y nunca inventa certeza.
    #[must_use]
    pub fn apply(&self, event: &HealthEvent, now: i64) -> Health {
        let mut next = self.clone();
        next.updated_at = now;
        match event {
            HealthEvent::Reset => return Health::unknown(now),
            HealthEvent::Success => {
                next.retry_after_at = None;
                next.evidence = None;
                // Si estaba agotado y funciona, la ventana se reinició: el número viejo ya no vale.
                if self.state == ProviderState::Exhausted {
                    next.certainty = QuotaCertainty::Unknown;
                    next.remaining = None;
                    next.reset_at = None;
                }
                next.state = if self.state == ProviderState::QuotaLow {
                    ProviderState::QuotaLow
                } else {
                    ProviderState::Healthy
                };
            }
            HealthEvent::Failure {
                kind,
                retry_after_at,
                reset_at,
                recent_same,
            } => {
                // El CLI puede decir «reintenta ya»: siempre queda un mínimo de espera.
                let retry = |cooldown: i64| {
                    Some(
                        retry_after_at
                            .filter(|t| *t > now)
                            .unwrap_or(now + cooldown),
                    )
                };
                let n = recent_same.saturating_add(1);
                next.evidence = Some(format!("{kind} x{n}"));
                match kind {
                    FailureType::Rpm | FailureType::Tpm | FailureType::TempRateLimit => {
                        // Un 429 no es «agotado»: espera y vuelve a probar.
                        if n >= THROTTLE_AFTER {
                            next.state = ProviderState::Throttled;
                            next.retry_after_at = retry(THROTTLED_COOLDOWN_MS);
                        } else {
                            next.state = ProviderState::RateLimited;
                            next.retry_after_at = retry(RATE_LIMIT_COOLDOWN_MS);
                        }
                    }
                    FailureType::DailyQuota
                    | FailureType::WeeklyQuota
                    | FailureType::AccountLimit
                    | FailureType::ModelLimit => {
                        next.state = ProviderState::Exhausted;
                        next.reset_at = reset_at.filter(|t| *t > now);
                        next.retry_after_at = next.reset_at;
                        // Agotado de verdad: la cuota restante es 0 solo si el CLI la informaba.
                        if next.certainty == QuotaCertainty::Known {
                            next.remaining = Some(0.0);
                        }
                    }
                    FailureType::Auth => {
                        next.state = ProviderState::AuthError;
                        next.retry_after_at = None;
                    }
                    FailureType::Network => {
                        if n >= OFFLINE_AFTER {
                            next.state = ProviderState::Offline;
                        } else {
                            next.state = ProviderState::Degraded;
                        }
                        next.retry_after_at = retry(NETWORK_COOLDOWN_MS);
                    }
                    FailureType::ModelUnavailable => {
                        next.state = ProviderState::Offline;
                        next.retry_after_at = retry(MODEL_UNAVAILABLE_COOLDOWN_MS);
                    }
                    FailureType::ProviderError | FailureType::Unknown => {
                        if n >= THROTTLE_AFTER {
                            next.state = ProviderState::Throttled;
                            next.retry_after_at = retry(THROTTLED_COOLDOWN_MS);
                        } else {
                            next.state = ProviderState::Degraded;
                            next.retry_after_at = retry(DEGRADED_COOLDOWN_MS);
                        }
                    }
                }
            }
            HealthEvent::Quota {
                used_fraction,
                reset_at,
                reserve,
            } => {
                let used = if used_fraction.is_finite() {
                    used_fraction.clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let remaining = 1.0 - used;
                next.certainty = QuotaCertainty::Known;
                next.remaining = Some(remaining);
                next.reset_at = reset_at.filter(|t| *t > now);
                // Un cooldown en curso (límite temporal, red, login) manda sobre la cuota.
                let in_cooldown = matches!(
                    self.state,
                    ProviderState::RateLimited
                        | ProviderState::Throttled
                        | ProviderState::Offline
                        | ProviderState::AuthError
                );
                if remaining <= 1e-9 {
                    next.state = ProviderState::Exhausted;
                    next.retry_after_at = next.reset_at;
                    next.evidence = Some("quota 100% used".into());
                } else if in_cooldown {
                    // Se conserva el estado; solo se actualiza la cuota.
                } else if remaining <= reserve.clamp(0.0, 1.0) {
                    next.state = ProviderState::QuotaLow;
                    next.evidence = Some("provider quota report".into());
                } else if matches!(
                    self.state,
                    ProviderState::QuotaLow | ProviderState::Exhausted
                ) {
                    next.state = ProviderState::Healthy;
                    next.evidence = None;
                    next.retry_after_at = None;
                }
            }
        }
        next
    }

    /// El estado que vale ahora: los cooldowns que ya pasaron se vuelven `PROBING`.
    pub fn effective_state(&self, now: i64) -> ProviderState {
        use ProviderState::*;
        match self.state {
            RateLimited | Throttled | Offline | Degraded
                if self.retry_after_at.is_some_and(|t| t <= now) =>
            {
                Probing
            }
            Exhausted => {
                let reset_passed = self.reset_at.is_some_and(|t| t <= now)
                    || self.retry_after_at.is_some_and(|t| t <= now);
                let no_time_known = self.reset_at.is_none() && self.retry_after_at.is_none();
                if reset_passed || (no_time_known && self.updated_at + EXHAUSTED_PROBE_MS <= now) {
                    Probing
                } else {
                    Exhausted
                }
            }
            other => other,
        }
    }

    /// `true` si el router puede mandarle trabajo ahora.
    pub fn is_usable(&self, now: i64) -> bool {
        use ProviderState::*;
        matches!(
            self.effective_state(now),
            Healthy | Degraded | QuotaLow | Probing | Unknown
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const NOW: i64 = 1_000_000;

    fn fail(kind: FailureType, recent_same: u32) -> HealthEvent {
        HealthEvent::Failure {
            kind,
            retry_after_at: None,
            reset_at: None,
            recent_same,
        }
    }

    #[test]
    fn a_429_is_rate_limited_never_exhausted() {
        for kind in [
            FailureType::Rpm,
            FailureType::Tpm,
            FailureType::TempRateLimit,
        ] {
            let h = Health::unknown(NOW).apply(&fail(kind, 0), NOW);
            assert_eq!(h.state, ProviderState::RateLimited, "{kind}");
            assert_eq!(h.retry_after_at, Some(NOW + RATE_LIMIT_COOLDOWN_MS));
            assert!(!h.is_usable(NOW));
        }
    }

    #[test]
    fn repeated_rate_limits_become_throttled_with_a_longer_cooldown() {
        let h = Health::unknown(NOW).apply(&fail(FailureType::TempRateLimit, 2), NOW);
        assert_eq!(h.state, ProviderState::Throttled);
        assert_eq!(h.retry_after_at, Some(NOW + THROTTLED_COOLDOWN_MS));
    }

    #[test]
    fn the_cli_retry_after_is_obeyed_but_never_in_the_past() {
        let ev = HealthEvent::Failure {
            kind: FailureType::TempRateLimit,
            retry_after_at: Some(NOW + 5_000),
            reset_at: None,
            recent_same: 0,
        };
        assert_eq!(
            Health::unknown(NOW).apply(&ev, NOW).retry_after_at,
            Some(NOW + 5_000)
        );
        let past = HealthEvent::Failure {
            kind: FailureType::TempRateLimit,
            retry_after_at: Some(NOW - 5_000),
            reset_at: None,
            recent_same: 0,
        };
        assert_eq!(
            Health::unknown(NOW).apply(&past, NOW).retry_after_at,
            Some(NOW + RATE_LIMIT_COOLDOWN_MS)
        );
    }

    #[test]
    fn quota_failures_exhaust_and_keep_the_reset_time() {
        let ev = HealthEvent::Failure {
            kind: FailureType::DailyQuota,
            retry_after_at: None,
            reset_at: Some(NOW + 3_600_000),
            recent_same: 0,
        };
        let h = Health::unknown(NOW).apply(&ev, NOW);
        assert_eq!(h.state, ProviderState::Exhausted);
        assert_eq!(h.reset_at, Some(NOW + 3_600_000));
        assert_eq!(h.effective_state(NOW + 1), ProviderState::Exhausted);
        // Pasada la hora de reinicio, se sondea.
        assert_eq!(h.effective_state(NOW + 3_600_001), ProviderState::Probing);
    }

    #[test]
    fn exhausted_without_a_reset_time_is_probed_on_a_cadence() {
        let h = Health::unknown(NOW).apply(&fail(FailureType::DailyQuota, 0), NOW);
        assert_eq!(
            h.effective_state(NOW + EXHAUSTED_PROBE_MS - 1),
            ProviderState::Exhausted
        );
        assert_eq!(
            h.effective_state(NOW + EXHAUSTED_PROBE_MS),
            ProviderState::Probing
        );
    }

    #[test]
    fn auth_errors_are_never_probed_on_their_own() {
        let h = Health::unknown(NOW).apply(&fail(FailureType::Auth, 0), NOW);
        assert_eq!(h.state, ProviderState::AuthError);
        assert_eq!(
            h.effective_state(NOW + 10 * 86_400_000),
            ProviderState::AuthError
        );
        // Solo un éxito o un reinicio explícito lo limpian.
        assert_eq!(
            h.apply(&HealthEvent::Success, NOW + 1).state,
            ProviderState::Healthy
        );
        assert_eq!(
            h.apply(&HealthEvent::Reset, NOW + 1).state,
            ProviderState::Unknown
        );
    }

    #[test]
    fn network_failures_degrade_then_go_offline() {
        let one = Health::unknown(NOW).apply(&fail(FailureType::Network, 0), NOW);
        assert_eq!(one.state, ProviderState::Degraded);
        assert!(one.is_usable(NOW), "degradado todavía sirve");
        let two = one.apply(&fail(FailureType::Network, 1), NOW + 1);
        assert_eq!(two.state, ProviderState::Offline);
        assert!(!two.is_usable(NOW + 1));
    }

    #[test]
    fn a_probe_that_succeeds_returns_to_healthy_and_one_that_fails_goes_back() {
        let h = Health::unknown(NOW).apply(&fail(FailureType::TempRateLimit, 0), NOW);
        let later = NOW + RATE_LIMIT_COOLDOWN_MS;
        assert_eq!(h.effective_state(later), ProviderState::Probing);
        assert!(h.is_usable(later));
        assert_eq!(
            h.apply(&HealthEvent::Success, later).state,
            ProviderState::Healthy
        );
        assert_eq!(
            h.apply(&fail(FailureType::TempRateLimit, 1), later).state,
            ProviderState::RateLimited
        );
    }

    #[test]
    fn known_quota_below_the_reserve_is_quota_low_and_never_invents_a_percentage() {
        let q = |used: f64| HealthEvent::Quota {
            used_fraction: used,
            reset_at: None,
            reserve: 0.2,
        };
        let h = Health::unknown(NOW).apply(&q(0.5), NOW);
        assert_eq!((h.state, h.remaining), (ProviderState::Unknown, Some(0.5)));
        let low = h.apply(&q(0.85), NOW + 1);
        assert_eq!(low.state, ProviderState::QuotaLow);
        assert_eq!(low.certainty, QuotaCertainty::Known);
        assert!(
            low.is_usable(NOW + 1),
            "QUOTA_LOW todavía se puede usar a mano"
        );
        let gone = low.apply(&q(1.0), NOW + 2);
        assert_eq!(gone.state, ProviderState::Exhausted);
        // La cuota se recupera: vuelve a sano.
        assert_eq!(gone.apply(&q(0.1), NOW + 3).state, ProviderState::Healthy);
    }

    #[test]
    fn a_cooldown_in_progress_wins_over_a_quota_report() {
        let limited = Health::unknown(NOW).apply(&fail(FailureType::TempRateLimit, 0), NOW);
        let h = limited.apply(
            &HealthEvent::Quota {
                used_fraction: 0.1,
                reset_at: None,
                reserve: 0.2,
            },
            NOW + 1,
        );
        assert_eq!(h.state, ProviderState::RateLimited);
        assert_eq!(h.remaining, Some(0.9));
    }

    #[test]
    fn success_after_exhaustion_forgets_the_stale_quota() {
        let h = Health {
            certainty: QuotaCertainty::Known,
            remaining: Some(0.0),
            state: ProviderState::Exhausted,
            ..Health::unknown(NOW)
        };
        let ok = h.apply(&HealthEvent::Success, NOW + 1);
        assert_eq!(ok.state, ProviderState::Healthy);
        assert_eq!(
            (ok.certainty, ok.remaining),
            (QuotaCertainty::Unknown, None)
        );
    }

    fn arb_failure() -> impl Strategy<Value = FailureType> {
        proptest::sample::select(FailureType::ALL.to_vec())
    }

    fn arb_event() -> impl Strategy<Value = HealthEvent> {
        prop_oneof![
            Just(HealthEvent::Success),
            Just(HealthEvent::Reset),
            (
                arb_failure(),
                proptest::option::of(-10_000_000i64..10_000_000),
                proptest::option::of(-10_000_000i64..10_000_000),
                0u32..10
            )
                .prop_map(|(kind, retry_after_at, reset_at, recent_same)| {
                    HealthEvent::Failure {
                        kind,
                        retry_after_at,
                        reset_at,
                        recent_same,
                    }
                }),
            (
                -1.0f64..3.0,
                proptest::option::of(0i64..10_000_000),
                -1.0f64..2.0
            )
                .prop_map(|(used_fraction, reset_at, reserve)| HealthEvent::Quota {
                    used_fraction,
                    reset_at,
                    reserve
                }),
        ]
    }

    proptest! {
        /// Un 429 nunca produce `EXHAUSTED`, y `EXHAUSTED` solo sale de un fallo de cuota o de
        /// una cuota informada al 100 %.
        #[test]
        fn exhausted_only_comes_from_quota(events in proptest::collection::vec(arb_event(), 1..12)) {
            let mut h = Health::unknown(NOW);
            for (i, ev) in events.iter().enumerate() {
                let prev = h.clone();
                h = h.apply(ev, NOW + i as i64);
                if h.state == ProviderState::Exhausted && prev.state != ProviderState::Exhausted {
                    let is_quota = matches!(
                        ev,
                        HealthEvent::Failure {
                            kind: FailureType::DailyQuota
                                | FailureType::WeeklyQuota
                                | FailureType::AccountLimit
                                | FailureType::ModelLimit,
                            ..
                        }
                    ) || matches!(ev, HealthEvent::Quota { used_fraction, .. } if *used_fraction >= 1.0 - 1e-9);
                    prop_assert!(is_quota, "{ev:?} dejó EXHAUSTED");
                }
            }
        }

        /// El porcentaje de cuota solo existe con certeza `KNOWN` (el CHECK de la base).
        #[test]
        fn a_percentage_always_comes_with_known_certainty(events in proptest::collection::vec(arb_event(), 1..12)) {
            let mut h = Health::unknown(NOW);
            for (i, ev) in events.iter().enumerate() {
                h = h.apply(ev, NOW + i as i64);
                prop_assert!(h.remaining.is_none() || h.certainty == QuotaCertainty::Known);
                if let Some(r) = h.remaining {
                    prop_assert!((0.0..=1.0).contains(&r));
                }
            }
        }

        /// Los cooldowns siempre quedan en el futuro y el tiempo solo los hace pasar a `PROBING`.
        #[test]
        fn cooldowns_are_in_the_future_and_expire_into_probing(kind in arb_failure(), recent in 0u32..8, wait in 0i64..100_000_000) {
            let h = Health::unknown(NOW).apply(&fail(kind, recent), NOW);
            if let Some(t) = h.retry_after_at {
                prop_assert!(t > NOW);
            }
            let state = h.effective_state(NOW + wait);
            if matches!(h.state, ProviderState::RateLimited | ProviderState::Throttled | ProviderState::Offline | ProviderState::Degraded) {
                let expired = h.retry_after_at.is_some_and(|t| t <= NOW + wait);
                prop_assert_eq!(state == ProviderState::Probing, expired);
            }
            // Un éxito siempre deja un estado utilizable.
            prop_assert!(h.apply(&HealthEvent::Success, NOW + wait).is_usable(NOW + wait));
        }

        /// `AUTH_ERROR` no se resuelve con el paso del tiempo.
        #[test]
        fn auth_error_never_expires(wait in 0i64..10_000_000_000) {
            let h = Health::unknown(NOW).apply(&fail(FailureType::Auth, 0), NOW);
            prop_assert_eq!(h.effective_state(NOW + wait), ProviderState::AuthError);
            prop_assert!(!h.is_usable(NOW + wait));
        }
    }
}
