//! Router determinista de modelos (IDEA §5.7, STACK §19). Sin E/S y sin LLM.
//!
//! 1. **Filtro de disponibilidad:** descarta lo no utilizable con un motivo (`RejectReason`).
//! 2. **Puntaje por reglas:** suma de factores ponderados por los pesos del profile.
//! 3. **Explicación:** texto generado desde los mismos factores (`/explain-route`).
//!
//! La decisión es una función de sus entradas: el mismo conjunto de candidatos da el mismo
//! resultado en cualquier orden, y un modelo no elegible nunca gana.

use std::cmp::Ordering;

use symphony_core::{Health, ProviderState, QuotaCertainty, RejectReason, SpeedClass};

/// Pesos de un profile (`profiles.weights_json`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights {
    pub fit: f64,
    pub context: f64,
    pub health: f64,
    pub quota: f64,
    pub scarcity: f64,
    pub failures: f64,
    pub load: f64,
    pub speed: f64,
}

impl Default for Weights {
    /// Los de `@code`.
    fn default() -> Self {
        Self {
            fit: 1.0,
            context: 0.3,
            health: 0.5,
            quota: 0.4,
            scarcity: 0.4,
            failures: 0.5,
            load: 0.2,
            speed: 0.1,
        }
    }
}

impl Weights {
    /// Lee `{"fit":1.0,…}`. Un peso ausente vale 0; un JSON inválido, `None`.
    pub fn from_json(text: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let obj = v.as_object()?;
        let w = |k: &str| {
            obj.get(k)
                .and_then(serde_json::Value::as_f64)
                .filter(|x| x.is_finite())
                .unwrap_or(0.0)
        };
        Some(Self {
            fit: w("fit"),
            context: w("context"),
            health: w("health"),
            quota: w("quota"),
            scarcity: w("scarcity"),
            failures: w("failures"),
            load: w("load"),
            speed: w("speed"),
        })
    }
}

/// Un modelo con todo lo que el router necesita saber de él.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub model_id: String,
    pub provider_id: String,
    pub display_name: String,
    /// Motivo fijo por el que no se puede usar (deshabilitado, CLI sin instalar o sin sesión).
    pub unavailable: Option<RejectReason>,
    pub supports_tools: bool,
    pub context_window: Option<u64>,
    pub speed: Option<SpeedClass>,
    /// `profile_models.base_score` (0–1); sin fila, 0.5.
    pub base_score: f64,
    /// Salud del proveedor.
    pub health: Health,
    /// Salud propia del modelo (un límite por modelo), si la tiene.
    pub model_health: Option<Health>,
    /// Reserva de cuota de su proveedor (0–1).
    pub reserve: f64,
    /// Fallos recientes del modelo o de su proveedor.
    pub recent_failures: u32,
    /// Agentes vivos que ya usan a su proveedor.
    pub load: u32,
}

/// Algo que este agente ya intentó y no sirvió: no se vuelve a mandar ahí.
#[derive(Debug, Clone, PartialEq)]
pub struct Exclusion {
    pub model_id: Option<String>,
    pub provider_id: Option<String>,
    pub reason: RejectReason,
}

#[derive(Debug, Clone)]
pub struct Request<'a> {
    pub now: i64,
    /// Nombre del profile, solo para la explicación (`@code`).
    pub profile: &'a str,
    pub weights: Weights,
    /// Tokens que el modelo nuevo tendría que leer (el handoff), si se sabe.
    pub needed_context: Option<u64>,
    pub need_tools: bool,
    /// `true` cuando el usuario elige a mano: puede gastar la reserva. Un profile automático no.
    pub allow_reserve: bool,
    pub excluded: &'a [Exclusion],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Evaluated {
    pub model_id: String,
    pub provider_id: String,
    pub eligible: bool,
    pub reject: Option<RejectReason>,
    /// `None` si no es elegible.
    pub score: Option<f64>,
    /// Aporte de cada factor al puntaje (ya ponderado). Vacío si no es elegible.
    pub factors: Vec<(&'static str, f64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub selected: Option<String>,
    /// Elegibles por puntaje (de mayor a menor), después los descartados por id.
    pub evaluated: Vec<Evaluated>,
    pub explanation: String,
}

fn finite(x: f64) -> f64 {
    if x.is_finite() { x } else { 0.0 }
}

fn unit(x: f64) -> f64 {
    finite(x).clamp(0.0, 1.0)
}

/// Motivo por el que un estado de salud no se puede usar ahora, si no se puede.
fn health_reject(h: &Health, now: i64) -> Option<RejectReason> {
    match h.effective_state(now) {
        ProviderState::AuthError => Some(RejectReason::Auth),
        ProviderState::Exhausted => Some(RejectReason::Exhausted),
        ProviderState::Offline => Some(RejectReason::Offline),
        ProviderState::RateLimited | ProviderState::Throttled => Some(RejectReason::Cooldown),
        _ => None,
    }
}

/// ¿La cuota está dentro de la reserva? Solo donde hay número: una cuota que el CLI informó, o
/// una estimación con presupuesto que ya la marcó `QUOTA_LOW`. Un aviso del proveedor sin cifra
/// (`QUOTA_LOW` con certeza `UNKNOWN`) no descarta a nadie: solo penaliza el puntaje.
fn in_reserve(c: &Candidate, now: i64) -> bool {
    let known_low = c.health.certainty == QuotaCertainty::Known
        && c.health.remaining.is_some_and(|r| r <= unit(c.reserve));
    let estimated_low = c.health.certainty == QuotaCertainty::Estimated
        && c.health.effective_state(now) == ProviderState::QuotaLow;
    known_low || estimated_low
}

fn reject_reason(c: &Candidate, req: &Request) -> Option<RejectReason> {
    if let Some(r) = c.unavailable {
        return Some(r);
    }
    for x in req.excluded {
        let model = x.model_id.as_deref().is_some_and(|m| m == c.model_id);
        let provider = x.provider_id.as_deref().is_some_and(|p| p == c.provider_id);
        if model || provider {
            return Some(x.reason);
        }
    }
    if let Some(r) = health_reject(&c.health, req.now) {
        return Some(r);
    }
    if let Some(r) = c
        .model_health
        .as_ref()
        .and_then(|h| health_reject(h, req.now))
    {
        return Some(r);
    }
    if req.need_tools && !c.supports_tools {
        return Some(RejectReason::Capability);
    }
    if let (Some(need), Some(window)) = (req.needed_context, c.context_window)
        && need > window
    {
        return Some(RejectReason::Context);
    }
    if !req.allow_reserve && in_reserve(c, req.now) {
        return Some(RejectReason::Reserve);
    }
    None
}

/// 0–1: cuánto se puede confiar en el estado de salud.
fn health_factor(s: ProviderState) -> f64 {
    match s {
        ProviderState::Healthy => 1.0,
        ProviderState::Unknown => 0.6,
        ProviderState::QuotaLow => 0.5,
        ProviderState::Probing => 0.3,
        ProviderState::Degraded => 0.2,
        _ => 0.0,
    }
}

/// 0–1: margen de cuota. Solo con certeza `KNOWN` es un número; lo estimado y lo desconocido
/// valen un valor neutro (nunca se finge precisión). Un aviso de cuota baja sin cifra, poco.
fn quota_margin(h: &Health, state: ProviderState, reserve: f64) -> f64 {
    match (h.certainty, h.remaining) {
        (QuotaCertainty::Known, Some(r)) => {
            let reserve = unit(reserve).min(0.99);
            ((unit(r) - reserve) / (1.0 - reserve)).clamp(0.0, 1.0)
        }
        (QuotaCertainty::Estimated, _) => 0.4,
        _ if state == ProviderState::QuotaLow => 0.2,
        _ => 0.5,
    }
}

/// 0–1: qué tan escasa es la cuota (preservarla pesa más con `@conserve`).
fn scarcity(h: &Health, state: ProviderState) -> f64 {
    match (h.certainty, h.remaining) {
        (QuotaCertainty::Known, Some(r)) => 1.0 - unit(r),
        (QuotaCertainty::Estimated, _) => 0.5,
        _ if state == ProviderState::QuotaLow => 0.8,
        _ => 0.3,
    }
}

fn evaluate(c: &Candidate, req: &Request) -> Evaluated {
    let base = Evaluated {
        model_id: c.model_id.clone(),
        provider_id: c.provider_id.clone(),
        eligible: false,
        reject: None,
        score: None,
        factors: Vec::new(),
    };
    if let Some(reject) = reject_reason(c, req) {
        return Evaluated {
            reject: Some(reject),
            ..base
        };
    }
    let w = &req.weights;
    let now = req.now;
    let state = c.health.effective_state(now);
    let model_state = c.model_health.as_ref().map(|h| h.effective_state(now));
    let health = health_factor(state).min(model_state.map_or(1.0, health_factor));
    let ctx = match (req.needed_context, c.context_window) {
        (Some(need), Some(window)) if window > 0 => {
            (1.0 - need as f64 / window as f64).clamp(0.0, 1.0)
        }
        _ => 0.0,
    };
    let speed = match c.speed {
        Some(SpeedClass::Fast) => 1.0,
        Some(SpeedClass::Slow) => 0.0,
        _ => 0.5,
    };
    let factors = vec![
        ("fit", finite(w.fit) * unit(c.base_score)),
        ("context", finite(w.context) * ctx),
        ("health", finite(w.health) * health),
        (
            "quota",
            finite(w.quota) * quota_margin(&c.health, state, c.reserve),
        ),
        ("scarcity", -finite(w.scarcity) * scarcity(&c.health, state)),
        (
            "failures",
            -finite(w.failures) * f64::from(c.recent_failures.min(5)) / 5.0,
        ),
        ("load", -finite(w.load) * f64::from(c.load.min(4)) / 4.0),
        ("speed", finite(w.speed) * speed),
    ];
    let score = factors.iter().map(|(_, v)| v).sum();
    Evaluated {
        eligible: true,
        score: Some(score),
        factors,
        ..base
    }
}

fn reject_text(r: RejectReason) -> &'static str {
    match r {
        RejectReason::Offline => "proveedor sin conexión o no instalado",
        RejectReason::Auth => "sesión inválida o vencida",
        RejectReason::Exhausted => "cuota agotada (o ya agotada en este agente)",
        RejectReason::Context => "contexto insuficiente para el handoff",
        RejectReason::Capability => "no soporta herramientas",
        RejectReason::Cooldown => "en espera tras un límite temporal",
        RejectReason::Reserve => "su reserva de cuota está protegida",
        RejectReason::Disabled => "deshabilitado",
    }
}

fn factor_label(name: &str, positive: bool) -> &'static str {
    match (name, positive) {
        ("fit", _) => "buen ajuste con el profile",
        ("context", _) => "holgura de contexto",
        ("health", _) => "proveedor sano",
        ("quota", _) => "margen de cuota",
        ("scarcity", _) => "cuota escasa",
        ("failures", _) => "fallos recientes",
        ("load", _) => "carga actual del proveedor",
        ("speed", _) => "velocidad",
        _ => "otro factor",
    }
}

fn quota_text(h: &Health) -> String {
    match (h.certainty, h.remaining) {
        (QuotaCertainty::Known, Some(r)) => format!("cuota {:.0} % restante", unit(r) * 100.0),
        (QuotaCertainty::Estimated, _) => "cuota estimada".into(),
        _ => "cuota desconocida".into(),
    }
}

/// Aporte mínimo (en puntaje) para que un factor aparezca en «Por qué».
const EXPLAIN_MIN: f64 = 0.1;

fn explain(
    req: &Request,
    candidates: &[Candidate],
    evaluated: &[Evaluated],
    selected: Option<&str>,
) -> String {
    let by_id = |id: &str| candidates.iter().find(|c| c.model_id == id);
    let name = |id: &str| by_id(id).map_or(id.to_string(), |c| c.display_name.clone());
    let mut out = vec![format!("Profile: {}", req.profile)];
    for e in evaluated {
        let Some(c) = by_id(&e.model_id) else {
            continue;
        };
        let line = match e.reject {
            Some(r) => format!("{} — descartado: {}", c.display_name, reject_text(r)),
            None => format!(
                "{} — elegible · salud {} · {}",
                c.display_name,
                c.health.effective_state(req.now),
                quota_text(&c.health)
            ),
        };
        out.push(line);
    }
    match selected {
        None => out.push("Ningún modelo es elegible ahora.".into()),
        Some(id) => {
            out.push(format!("Elegido: {}", name(id)));
            out.push("Por qué:".into());
            if let Some(e) = evaluated.iter().find(|e| e.model_id == id) {
                let mut sorted = e.factors.clone();
                sorted.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()).then_with(|| a.0.cmp(b.0)));
                for (f, v) in sorted.iter().filter(|(_, v)| v.abs() >= EXPLAIN_MIN) {
                    out.push(format!(
                        "{} {}",
                        if *v > 0.0 { "+" } else { "-" },
                        factor_label(f, *v > 0.0)
                    ));
                }
            }
            // Lo que se deja descansar a propósito (FLOW §8.4).
            for e in evaluated.iter().filter(|e| e.eligible && e.model_id != id) {
                if let Some(c) = by_id(&e.model_id)
                    && c.health.effective_state(req.now) == ProviderState::QuotaLow
                {
                    out.push(format!(
                        "- {} se conserva porque su cuota es baja",
                        c.display_name
                    ));
                }
            }
        }
    }
    out.join("\n")
}

/// Elige el modelo para `req` entre `candidates`.
pub fn route(candidates: &[Candidate], req: &Request) -> Decision {
    let mut evaluated: Vec<Evaluated> = candidates.iter().map(|c| evaluate(c, req)).collect();
    evaluated.sort_by(|a, b| match (a.eligible, b.eligible) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (true, true) => b
            .score
            .unwrap_or(f64::MIN)
            .total_cmp(&a.score.unwrap_or(f64::MIN))
            .then_with(|| a.model_id.cmp(&b.model_id)),
        (false, false) => a.model_id.cmp(&b.model_id),
    });
    let selected = evaluated
        .first()
        .filter(|e| e.eligible)
        .map(|e| e.model_id.clone());
    let explanation = explain(req, candidates, &evaluated, selected.as_deref());
    Decision {
        selected,
        evaluated,
        explanation,
    }
}

#[cfg(test)]
mod tests;
