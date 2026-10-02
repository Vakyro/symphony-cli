//! Routing en el daemon (P10.S4–S5): arma los candidatos desde la base, llama al router puro
//! (`symphony-router`) y guarda la decisión con todos los modelos evaluados (`/explain-route`).

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;
use symphony_core::{AgentId, Health, RejectReason, RoutingDecisionId, RoutingTrigger, SpeedClass};
use symphony_router::{Candidate, Decision, Exclusion, Request, Weights, route};
use symphony_store::health as db;
use symphony_store::repo::RepoError;

use crate::health::HealthConfig;

/// Fallos de los últimos minutos que restan puntaje al modelo y a su proveedor.
pub const RECENT_FAILURES_WINDOW_MS: i64 = 30 * 60 * 1000;

/// Cómo elegir: el profile y lo que ya se probó o no se permite.
#[derive(Debug, Clone, Default)]
pub struct Choose {
    pub profile: String,
    pub excluded: Vec<Exclusion>,
    /// Política `SAME_PROVIDER`: solo modelos de este proveedor.
    pub only_provider: Option<String>,
    /// Rotación a propósito (umbral de contexto del chat): ni este proveedor ni este modelo.
    /// No son «descartados»: simplemente no compiten.
    pub skip_provider: Option<String>,
    pub skip_model: Option<String>,
    /// Elección manual del usuario: puede gastar la reserva de cuota.
    pub allow_reserve: bool,
    pub needed_context: Option<u64>,
}

pub fn profile_weights(conn: &Connection, profile: &str) -> Result<Option<Weights>, RepoError> {
    let json: Option<String> = conn
        .query_row(
            "SELECT weights_json FROM profiles WHERE id = ?1",
            [profile],
            |r| r.get(0),
        )
        .optional()?;
    Ok(json.and_then(|j| Weights::from_json(&j)))
}

/// `(id, descripción, pesos en JSON)` de los profiles.
pub fn list_profiles(conn: &Connection) -> Result<Vec<(String, String, String)>, RepoError> {
    let mut stmt =
        conn.prepare("SELECT id, description, weights_json FROM profiles ORDER BY id")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Todos los modelos conocidos, con lo que el router necesita saber de cada uno.
pub fn candidates(
    conn: &Connection,
    has_adapter: &dyn Fn(&str) -> bool,
    profile: &str,
    cfg: &HealthConfig,
    now: i64,
) -> Result<Vec<Candidate>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.provider_id, m.display_name, m.enabled, m.supports_tools, m.context_window,
                m.speed_class, p.setup_state, p.enabled, COALESCE(pm.base_score, 0.5)
         FROM models m
         JOIN providers p ON p.id = m.provider_id
         LEFT JOIN profile_models pm ON pm.model_id = m.id AND pm.profile_id = ?1
         ORDER BY m.id",
    )?;
    type Row = (
        String,
        String,
        String,
        bool,
        bool,
        Option<i64>,
        Option<String>,
        String,
        bool,
        f64,
    );
    let rows: Vec<Row> = stmt
        .query_map([profile], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get::<_, i64>(3)? == 1,
                r.get::<_, i64>(4)? == 1,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get::<_, i64>(8)? == 1,
                r.get(9)?,
            ))
        })?
        .collect::<Result<_, _>>()?;

    // Fallos recientes: por modelo y a nivel proveedor (los que no son de un modelo).
    let failures = db::recent_failures_by_model(conn, now - RECENT_FAILURES_WINDOW_MS)?;
    let mut by_model: HashMap<String, u32> = HashMap::new();
    let mut by_provider: HashMap<String, u32> = HashMap::new();
    for (provider, model, n) in failures {
        match model {
            Some(m) => *by_model.entry(m).or_default() += n,
            None => *by_provider.entry(provider).or_default() += n,
        }
    }
    // Carga: agentes vivos por proveedor.
    let mut load: HashMap<String, u32> = HashMap::new();
    let mut live = conn.prepare(
        "SELECT provider_id, COUNT(DISTINCT agent_id) FROM agent_runs WHERE ended_at IS NULL GROUP BY provider_id",
    )?;
    for row in live.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
        let (p, n) = row?;
        load.insert(p, u32::try_from(n).unwrap_or(u32::MAX));
    }
    let mut provider_health: HashMap<String, Health> = HashMap::new();

    let mut out = Vec::with_capacity(rows.len());
    for (id, provider, name, model_on, tools, window, speed, setup, provider_on, base) in rows {
        let unavailable = if !model_on || !provider_on {
            Some(RejectReason::Disabled)
        } else {
            match setup.as_str() {
                "READY" if has_adapter(&provider) => None,
                "LOGIN_REQUIRED" => Some(RejectReason::Auth),
                _ => Some(RejectReason::Offline),
            }
        };
        let health = match provider_health.get(&provider) {
            Some(h) => h.clone(),
            None => {
                let h =
                    db::get_health(conn, &provider, None)?.unwrap_or_else(|| Health::unknown(now));
                provider_health.insert(provider.clone(), h.clone());
                h
            }
        };
        out.push(Candidate {
            model_health: db::get_health(conn, &provider, Some(&id))?,
            unavailable,
            supports_tools: tools,
            context_window: window.and_then(|w| u64::try_from(w).ok()),
            speed: speed.as_deref().and_then(|s| s.parse::<SpeedClass>().ok()),
            base_score: base,
            health,
            reserve: cfg.reserve_for(&provider),
            recent_failures: by_model.get(&id).copied().unwrap_or(0)
                + by_provider.get(&provider).copied().unwrap_or(0),
            load: load.get(&provider).copied().unwrap_or(0),
            model_id: id,
            display_name: name,
            provider_id: provider,
        });
    }
    Ok(out)
}

/// Elige un modelo para `choose.profile`. `Err(NotFound)` si el profile no existe.
pub fn choose(
    conn: &Connection,
    has_adapter: &dyn Fn(&str) -> bool,
    cfg: &HealthConfig,
    now: i64,
    c: &Choose,
) -> Result<Decision, RepoError> {
    let weights = profile_weights(conn, &c.profile)?.ok_or_else(|| RepoError::NotFound {
        entity: "profile",
        id: c.profile.clone(),
    })?;
    let mut cands = candidates(conn, has_adapter, &c.profile, cfg, now)?;
    if let Some(only) = &c.only_provider {
        cands.retain(|m| &m.provider_id == only);
    }
    if let Some(skip) = &c.skip_provider {
        cands.retain(|m| &m.provider_id != skip);
    }
    if let Some(skip) = &c.skip_model {
        cands.retain(|m| &m.model_id != skip);
    }
    Ok(route(
        &cands,
        &Request {
            now,
            profile: &c.profile,
            weights,
            needed_context: c.needed_context,
            need_tools: true,
            allow_reserve: c.allow_reserve,
            excluded: &c.excluded,
        },
    ))
}

/// Lo que identifica una decisión al guardarla.
pub struct NewDecision<'a> {
    pub agent: AgentId,
    pub trigger: RoutingTrigger,
    /// `None` en una elección exacta del usuario.
    pub profile: Option<&'a str>,
    /// Puede diferir del elegido por el router: una elección exacta del usuario.
    pub selected: Option<&'a str>,
    pub explanation: &'a str,
}

/// Guarda la decisión y todos los candidatos evaluados (con su motivo de descarte y factores).
pub fn persist(
    conn: &Connection,
    d: &NewDecision<'_>,
    decision: &Decision,
    now: i64,
) -> Result<RoutingDecisionId, RepoError> {
    let id = RoutingDecisionId::new();
    conn.execute(
        "INSERT INTO routing_decisions (id, agent_id, trigger, profile_id, selected_model_id, engine, explanation, decided_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'RULES', ?6, ?7)",
        params![
            id.to_string(),
            d.agent.to_string(),
            d.trigger.as_str(),
            d.profile,
            d.selected,
            d.explanation,
            now
        ],
    )?;
    for e in &decision.evaluated {
        let factors = e.eligible.then(|| {
            let map: serde_json::Map<String, serde_json::Value> = e
                .factors
                .iter()
                .map(|(k, v)| ((*k).to_string(), json!(((v * 1000.0).round()) / 1000.0)))
                .collect();
            serde_json::Value::Object(map).to_string()
        });
        conn.execute(
            "INSERT INTO routing_candidates (decision_id, model_id, eligible, reject_reason, score, factors_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id.to_string(),
                e.model_id,
                i64::from(e.eligible),
                e.reject.map(|r| r.as_str()),
                e.score,
                factors
            ],
        )?;
    }
    Ok(id)
}

/// Una decisión ya tomada que se guarda junto con el cambio que la origina (el run nuevo o la
/// espera del agente), en la misma escritura.
#[derive(Debug, Clone)]
pub struct Pending {
    pub decision: Decision,
    pub trigger: RoutingTrigger,
    /// `None` en una elección exacta del usuario.
    pub profile: Option<String>,
    pub selected: Option<String>,
    pub explanation: String,
}

impl Pending {
    /// Guarda la decisión y, si hay run, la ata a él.
    pub fn save(
        &self,
        conn: &Connection,
        agent: AgentId,
        run: Option<symphony_core::RunId>,
        now: i64,
    ) -> Result<(), RepoError> {
        let id = persist(
            conn,
            &NewDecision {
                agent,
                trigger: self.trigger,
                profile: self.profile.as_deref(),
                selected: self.selected.as_deref(),
                explanation: &self.explanation,
            },
            &self.decision,
            now,
        )?;
        if let Some(run) = run {
            set_run_decision(conn, run, id)?;
        }
        Ok(())
    }
}

/// Texto de una decisión cuando el usuario eligió el modelo a mano.
pub fn exact_explanation(model: &str, others: &str) -> String {
    format!(
        "Modelo exacto elegido por el usuario: {model}. Symphony no lo sustituye en silencio.\n\n{others}"
    )
}

/// Ata la decisión al run que arrancó con ella.
pub fn set_run_decision(
    conn: &Connection,
    run: symphony_core::RunId,
    decision: RoutingDecisionId,
) -> Result<(), RepoError> {
    conn.execute(
        "UPDATE agent_runs SET routing_decision_id = ?2 WHERE id = ?1",
        params![run.to_string(), decision.to_string()],
    )?;
    Ok(())
}

/// Lo que se sabe de cada decisión de un agente (más reciente primero): para `/explain-route`.
pub fn explain(
    conn: &Connection,
    agent: AgentId,
    limit: i64,
) -> Result<serde_json::Value, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT id, trigger, profile_id, selected_model_id, explanation, decided_at
         FROM routing_decisions WHERE agent_id = ?1 ORDER BY decided_at DESC, id DESC LIMIT ?2",
    )?;
    let decisions: Vec<(String, serde_json::Value)> = stmt
        .query_map(params![agent.to_string(), limit], |r| {
            let id: String = r.get(0)?;
            Ok((
                id.clone(),
                json!({
                    "id": id, "trigger": r.get::<_, String>(1)?, "profile": r.get::<_, Option<String>>(2)?,
                    "selected": r.get::<_, Option<String>>(3)?, "explanation": r.get::<_, Option<String>>(4)?,
                    "decided_at": r.get::<_, i64>(5)?,
                }),
            ))
        })?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, mut d) in decisions {
        let mut cs = conn.prepare(
            "SELECT model_id, eligible, reject_reason, score, factors_json FROM routing_candidates
             WHERE decision_id = ?1 ORDER BY eligible DESC, score DESC, model_id",
        )?;
        d["candidates"] = json!(
            cs.query_map([&id], |r| {
                Ok(json!({
                    "model_id": r.get::<_, String>(0)?,
                    "eligible": r.get::<_, i64>(1)? == 1,
                    "reject_reason": r.get::<_, Option<String>>(2)?,
                    "score": r.get::<_, Option<f64>>(3)?,
                    "factors": r.get::<_, Option<String>>(4)?
                        .and_then(|f| serde_json::from_str::<serde_json::Value>(&f).ok()),
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?
        );
        out.push(d);
    }
    Ok(json!(out))
}

// --- puntajes iniciales (bootstrap, editables) -------------------------------------------

/// Orden de los siete profiles de `002_health.sql`.
const PROFILES: [&str; 7] = [
    "@code",
    "@debug",
    "@fast",
    "@reasoning",
    "@docs",
    "@review",
    "@conserve",
];

struct Traits {
    window: Option<i64>,
    speed: Option<SpeedClass>,
    scores: [f64; 7],
}

/// Metadata aproximada por familia de modelo. Son puntos de partida editables, no verdades:
/// los números salen de lo que cada familia hace bien (IDEA §5.7: «score bootstrap»).
fn traits(model_id: &str) -> Traits {
    let id = model_id.to_lowercase();
    let has = |s: &str| id.contains(s);
    //            code  debug fast  reason docs  review conserve
    let (window, speed, scores): (Option<i64>, Option<SpeedClass>, [f64; 7]) =
        if has("opus") || has("fable") {
            (
                Some(200_000),
                Some(SpeedClass::Slow),
                [0.95, 0.95, 0.3, 1.0, 0.8, 0.95, 0.3],
            )
        } else if has("sonnet") {
            (
                Some(200_000),
                Some(SpeedClass::Medium),
                [0.9, 0.9, 0.6, 0.85, 0.85, 0.9, 0.6],
            )
        } else if has("haiku") {
            (
                Some(200_000),
                Some(SpeedClass::Fast),
                [0.6, 0.6, 0.9, 0.5, 0.7, 0.6, 0.95],
            )
        } else if has("sol") {
            (
                None,
                Some(SpeedClass::Slow),
                [0.95, 0.95, 0.5, 0.95, 0.85, 0.9, 0.5],
            )
        } else if has("terra") || has("gpt-5.5") {
            (
                None,
                Some(SpeedClass::Medium),
                [0.85, 0.85, 0.6, 0.85, 0.85, 0.85, 0.7],
            )
        } else if has("luna") {
            (
                None,
                Some(SpeedClass::Fast),
                [0.65, 0.65, 0.9, 0.55, 0.7, 0.6, 0.95],
            )
        } else if has("gemini") && has("pro") {
            (
                Some(1_000_000),
                Some(SpeedClass::Slow),
                [0.85, 0.85, 0.5, 0.85, 0.85, 0.8, 0.6],
            )
        } else if has("gemini") && has("flash") && has("high") {
            (
                Some(1_000_000),
                Some(SpeedClass::Medium),
                [0.7, 0.7, 0.85, 0.6, 0.75, 0.65, 0.9],
            )
        } else if has("gemini") && has("flash") {
            (
                Some(1_000_000),
                Some(SpeedClass::Fast),
                [0.6, 0.6, 0.95, 0.5, 0.7, 0.55, 0.95],
            )
        } else if has("moonshot") || has("kimi") {
            (
                None,
                Some(SpeedClass::Medium),
                [0.7, 0.7, 0.6, 0.7, 0.7, 0.65, 0.8],
            )
        } else if has("github") {
            (
                None,
                Some(SpeedClass::Medium),
                [0.75, 0.75, 0.6, 0.7, 0.75, 0.7, 0.8],
            )
        } else {
            (None, None, [0.5; 7])
        };
    Traits {
        window,
        speed,
        scores,
    }
}

/// Completa lo que falte de cada modelo (ventana, velocidad) y siembra `profile_models`. No
/// pisa nada que ya tenga valor: lo que el usuario edite se respeta.
pub fn bootstrap(conn: &Connection) -> Result<(), RepoError> {
    let mut stmt = conn.prepare("SELECT id FROM models")?;
    let ids: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for id in ids {
        let t = traits(&id);
        conn.execute(
            "UPDATE models SET context_window = COALESCE(context_window, ?2), speed_class = COALESCE(speed_class, ?3) WHERE id = ?1",
            params![id, t.window, t.speed.map(|s| s.as_str())],
        )?;
        for (profile, score) in PROFILES.iter().zip(t.scores) {
            conn.execute(
                "INSERT OR IGNORE INTO profile_models (profile_id, model_id, base_score) VALUES (?1, ?2, ?3)",
                params![profile, id, score],
            )?;
        }
    }
    Ok(())
}
