//! Salud de proveedores, cuota y uso en el daemon (P10.S2–S3, IDEA §5.8).
//!
//! Aplica la máquina de estados pura de `symphony_core::health` a lo que ocurre en los runs.
//! Todo corre dentro de una escritura del writer único: nunca se abre la base desde otro sitio.

use std::collections::HashMap;

use rusqlite::Connection;
use symphony_adapter_common::{ProviderError, QuotaSnapshot};
use symphony_core::{FailureType, Health, HealthEvent, ProviderFailureId, RunId, UsageSource};
use symphony_store::health as db;
use symphony_store::repo::{self, RepoError};

/// Ventana en la que se cuentan fallos «recientes» del mismo tipo.
pub const RECENT_FAILURES_MS: i64 = 10 * 60 * 1000;

/// Reserva y presupuestos por proveedor (de `config.toml`).
#[derive(Debug, Clone, PartialEq)]
pub struct HealthConfig {
    pub default_reserve: f64,
    pub reserve: HashMap<String, f64>,
    /// Presupuesto opcional por proveedor para la cuota `ESTIMATED`: `(horas de ventana, tokens)`.
    pub budgets: HashMap<String, (u64, u64)>,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            default_reserve: 0.20,
            reserve: HashMap::new(),
            budgets: HashMap::new(),
        }
    }
}

impl HealthConfig {
    /// La reserva y los presupuestos de `config.toml`.
    pub fn from_config(c: &symphony_core::Config) -> Self {
        let mut out = Self {
            default_reserve: c.providers.quota_reserve,
            ..Self::default()
        };
        for (id, l) in &c.providers.limits {
            if let Some(r) = l.reserve {
                out.reserve.insert(id.clone(), r);
            }
            if let (Some(h), Some(t)) = (l.window_hours, l.window_tokens) {
                out.budgets.insert(id.clone(), (h, t));
            }
        }
        out
    }

    pub fn reserve_for(&self, provider: &str) -> f64 {
        self.reserve
            .get(provider)
            .copied()
            .unwrap_or(self.default_reserve)
    }
}

/// Los CLIs informan los reinicios en segundos desde la época; la base guarda milisegundos.
pub fn epoch_ms(t: i64) -> i64 {
    if t < 100_000_000_000 { t * 1000 } else { t }
}

/// ¿El error afecta solo a un modelo (y no a todo el proveedor)?
fn model_scoped(kind: FailureType) -> bool {
    matches!(
        kind,
        FailureType::ModelLimit | FailureType::ModelUnavailable
    )
}

fn load(
    conn: &Connection,
    provider: &str,
    model: Option<&str>,
    now: i64,
) -> Result<Health, RepoError> {
    Ok(db::get_health(conn, provider, model)?.unwrap_or_else(|| Health::unknown(now)))
}

/// Un error del proveedor: actualiza su salud (y la del modelo si el límite es del modelo).
pub fn on_failure(
    conn: &Connection,
    provider: &str,
    model: Option<&str>,
    err: &ProviderError,
    now: i64,
) -> Result<(), RepoError> {
    let scope = if model_scoped(err.failure_type) {
        model
    } else {
        None
    };
    // El llamador ya guardó este fallo en `provider_failures`: lo que cuenta es lo anterior
    // (la máquina de estados suma el actual).
    let recent = db::recent_failure_count(
        conn,
        provider,
        scope,
        err.failure_type,
        now - RECENT_FAILURES_MS,
    )?
    .saturating_sub(1);
    let event = HealthEvent::Failure {
        kind: err.failure_type,
        retry_after_at: err
            .retry_after_ms
            .map(|ms| now + i64::try_from(ms).unwrap_or(i64::MAX / 2)),
        reset_at: err.resets_at.map(epoch_ms),
        recent_same: recent,
    };
    let next = load(conn, provider, scope, now)?.apply(&event, now);
    db::put_health(conn, provider, scope, &next)
}

/// Guarda el fallo en `provider_failures` (con la cuenta `default`).
pub fn record_failure_row(
    conn: &Connection,
    id: ProviderFailureId,
    run: Option<RunId>,
    provider: &str,
    model: Option<&str>,
    err: &ProviderError,
    now: i64,
) -> Result<(), RepoError> {
    let failure = repo::NewProviderFailure {
        id,
        provider_id: provider.to_string(),
        model_id: model.map(str::to_string),
        run_id: run,
        failure_type: err.failure_type,
        raw_code: err.raw_code.clone(),
        message: symphony_core::redact(&err.message).into_owned(),
        reset_at: err.resets_at.map(epoch_ms),
        retry_after_at: err
            .retry_after_ms
            .map(|ms| now + i64::try_from(ms).unwrap_or(i64::MAX / 2)),
    };
    repo::insert_provider_failure(conn, &failure, now)
}

/// Un turno terminó bien: el proveedor (y el modelo, si tenía una fila propia) vuelven a sanos.
pub fn on_success(
    conn: &Connection,
    provider: &str,
    model: Option<&str>,
    now: i64,
) -> Result<(), RepoError> {
    let next = load(conn, provider, None, now)?.apply(&HealthEvent::Success, now);
    db::put_health(conn, provider, None, &next)?;
    if let Some(m) = model
        && let Some(cur) = db::get_health(conn, provider, Some(m))?
    {
        db::put_health(
            conn,
            provider,
            Some(m),
            &cur.apply(&HealthEvent::Success, now),
        )?;
    }
    Ok(())
}

/// Una cuota que el CLI informó. Se usa la ventana más apretada de las que se conocen: una
/// ventana que ya se reinició (su `resets_at` pasó) no cuenta.
pub fn on_quota(
    conn: &Connection,
    provider: &str,
    reserve: f64,
    _latest: &QuotaSnapshot,
    now: i64,
) -> Result<(), RepoError> {
    let windows = db::latest_quota_windows(conn, provider)?;
    if windows.is_empty() {
        return Ok(());
    }
    // Una ventana que ya se reinició no cuenta; si todas se reiniciaron, la cuota está entera.
    let tightest = windows
        .into_iter()
        .filter(|(_, _, reset)| reset.is_none_or(|t| epoch_ms(t) > now))
        .max_by(|a, b| a.1.total_cmp(&b.1));
    let (used, reset) = tightest.map_or((0.0, None), |(_, used, reset)| (used, reset));
    let event = HealthEvent::Quota {
        used_fraction: used,
        reset_at: reset.map(epoch_ms),
        reserve,
    };
    let next = load(conn, provider, None, now)?.apply(&event, now);
    db::put_health(conn, provider, None, &next)
}

/// Tokens de contexto que el CLI informó al terminar un turno (`REPORTED`). El contexto de cada
/// turno incluye al del anterior: sumarlos contaría varias veces lo mismo, así que cada run
/// guarda una sola fila con su pico de contexto (un mínimo del consumo, nunca un múltiplo).
pub fn on_usage(
    conn: &Connection,
    run: RunId,
    context_tokens: u64,
    now: i64,
) -> Result<(), RepoError> {
    let (provider_id, model_id) = db::run_scope(conn, run)?;
    db::upsert_reported_usage(conn, run, &provider_id, &model_id, context_tokens, now)
}

/// Estimación de cuota para un proveedor con presupuesto en la config: tokens usados en la
/// ventana frente a `window_tokens`. Es `ESTIMATED`, nunca un porcentaje, y no pisa lo que el CLI
/// informa (la máquina de estados lo garantiza).
pub fn refresh_estimate(
    conn: &Connection,
    provider: &str,
    cfg: &HealthConfig,
    now: i64,
) -> Result<(), RepoError> {
    let Some(&(hours, tokens)) = cfg.budgets.get(provider) else {
        return Ok(());
    };
    let since = now
        - i64::try_from(hours)
            .unwrap_or(i64::MAX / 4)
            .saturating_mul(3_600_000);
    let used = db::provider_tokens_since(conn, provider, since)?;
    let event = HealthEvent::Estimate {
        used_fraction: used as f64 / tokens as f64,
        reserve: cfg.reserve_for(provider),
    };
    let next = load(conn, provider, None, now)?.apply(&event, now);
    db::put_health(conn, provider, None, &next)
}

/// Un run terminó: se registra su uso (estimado si el CLI no lo informó) y se refresca la
/// estimación de cuota de su proveedor.
pub fn on_run_finished(
    conn: &Connection,
    run: RunId,
    cfg: &HealthConfig,
    now: i64,
) -> Result<(), RepoError> {
    finish_usage(conn, run, now)?;
    let (provider, _) = db::run_scope(conn, run)?;
    refresh_estimate(conn, &provider, cfg, now)
}

/// Al terminar un run cuyo CLI no informó tokens se registra una estimación (`ESTIMATED`).
pub fn finish_usage(conn: &Connection, run: RunId, now: i64) -> Result<(), RepoError> {
    if db::run_has_reported_usage(conn, run)? {
        return Ok(());
    }
    let (tokens_in, tokens_out) = db::estimate_run_tokens(conn, run)?;
    if tokens_in == 0 && tokens_out == 0 {
        return Ok(());
    }
    let (provider_id, model_id) = db::run_scope(conn, run)?;
    db::insert_usage(
        conn,
        &db::NewUsage {
            run_id: run,
            provider_id,
            model_id,
            tokens_in: Some(tokens_in),
            tokens_out: Some(tokens_out),
            source: UsageSource::Estimated,
        },
        now,
    )
}
