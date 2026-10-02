use proptest::prelude::*;
use symphony_core::{
    FailureType, Health, HealthEvent, ProviderState, QuotaCertainty, RejectReason, SpeedClass,
};

use super::*;

const NOW: i64 = 1_000_000;

fn cand(model: &str, provider: &str, name: &str) -> Candidate {
    Candidate {
        model_id: model.into(),
        provider_id: provider.into(),
        display_name: name.into(),
        unavailable: None,
        supports_tools: true,
        context_window: Some(200_000),
        speed: Some(SpeedClass::Medium),
        base_score: 0.7,
        health: Health {
            state: ProviderState::Healthy,
            ..Health::unknown(NOW)
        },
        model_health: None,
        reserve: 0.2,
        recent_failures: 0,
        load: 0,
    }
}

fn req(excluded: &[Exclusion]) -> Request<'_> {
    Request {
        now: NOW,
        profile: "@code",
        weights: Weights::default(),
        needed_context: None,
        need_tools: true,
        allow_reserve: false,
        excluded,
    }
}

fn limited(kind: FailureType) -> Health {
    Health::unknown(NOW).apply(
        &HealthEvent::Failure {
            kind,
            retry_after_at: None,
            reset_at: None,
            recent_same: 0,
        },
        NOW,
    )
}

fn known(remaining: f64) -> Health {
    Health::unknown(NOW).apply(
        &HealthEvent::Quota {
            used_fraction: 1.0 - remaining,
            reset_at: None,
            reserve: 0.2,
        },
        NOW,
    )
}

fn rejected(d: &Decision, model: &str) -> Option<RejectReason> {
    d.evaluated.iter().find(|e| e.model_id == model)?.reject
}

#[test]
fn flow_example_sonnet_quota_low_sol_healthy_kimi_recent_failure() {
    // FLOW §8.4: Sonnet elegible pero con cuota baja (aviso del proveedor, sin cifra),
    // Sol elegible y sano, Kimi elegible pero con un fallo reciente → gana Sol.
    let mut sonnet = cand("claude/sonnet", "anthropic", "Claude Sonnet");
    sonnet.health = Health {
        state: ProviderState::QuotaLow,
        evidence: Some("provider warning".into()),
        ..Health::unknown(NOW)
    };
    let sol = cand("openai/sol", "openai", "Codex Sol");
    let mut kimi = cand("moonshot/default", "moonshot", "Kimi");
    kimi.recent_failures = 3;

    let d = route(&[sonnet, sol, kimi], &req(&[]));
    assert_eq!(d.selected.as_deref(), Some("openai/sol"));
    let order: Vec<&str> = d.evaluated.iter().map(|e| e.model_id.as_str()).collect();
    assert_eq!(order[0], "openai/sol");
    assert!(
        d.explanation.contains("Elegido: Codex Sol"),
        "{}",
        d.explanation
    );
    assert!(
        d.explanation
            .contains("Claude Sonnet — elegible · salud QUOTA_LOW · cuota desconocida")
    );
    assert!(
        d.explanation
            .contains("- Claude Sonnet se conserva porque su cuota es baja")
    );
    assert!(d.explanation.contains("+ proveedor sano"));
}

#[test]
fn each_reject_reason_has_its_own_case() {
    let mut c: Vec<Candidate> = Vec::new();
    let mut add = |id: &str, f: &dyn Fn(&mut Candidate)| {
        let mut m = cand(id, &format!("p-{id}"), id);
        f(&mut m);
        c.push(m);
    };
    add("disabled", &|m| {
        m.unavailable = Some(RejectReason::Disabled)
    });
    add("not-installed", &|m| {
        m.unavailable = Some(RejectReason::Offline)
    });
    add("auth", &|m| m.health = limited(FailureType::Auth));
    add("exhausted", &|m| {
        m.health = limited(FailureType::DailyQuota)
    });
    add("offline", &|m| {
        m.health = limited(FailureType::ModelUnavailable)
    });
    add("cooldown", &|m| {
        m.health = limited(FailureType::TempRateLimit)
    });
    add("model-limit", &|m| {
        m.model_health = Some(limited(FailureType::ModelLimit))
    });
    add("no-tools", &|m| m.supports_tools = false);
    add("small-context", &|m| m.context_window = Some(1_000));
    add("reserve", &|m| m.health = known(0.1));
    add("ok", &|_| {});

    let mut r = req(&[]);
    r.needed_context = Some(50_000);
    let d = route(&c, &r);
    assert_eq!(d.selected.as_deref(), Some("ok"));
    use RejectReason::*;
    for (model, want) in [
        ("disabled", Disabled),
        ("not-installed", Offline),
        ("auth", Auth),
        ("exhausted", Exhausted),
        ("offline", Offline),
        ("cooldown", Cooldown),
        ("model-limit", Exhausted),
        ("no-tools", Capability),
        ("small-context", Context),
        ("reserve", Reserve),
    ] {
        assert_eq!(rejected(&d, model), Some(want), "{model}");
    }
}

#[test]
fn an_automatic_profile_keeps_the_reserve_but_a_manual_choice_may_spend_it() {
    let mut low = cand("claude/sonnet", "anthropic", "Claude Sonnet");
    low.health = known(0.1);
    let other = cand("openai/sol", "openai", "Codex Sol");
    let auto = route(&[low.clone(), other.clone()], &req(&[]));
    assert_eq!(auto.selected.as_deref(), Some("openai/sol"));
    assert_eq!(
        rejected(&auto, "claude/sonnet"),
        Some(RejectReason::Reserve)
    );

    // Elegido a mano por el usuario (y sin alternativa): sí se puede usar.
    let mut manual = req(&[]);
    manual.allow_reserve = true;
    let alone = route(&[low], &manual);
    assert_eq!(alone.selected.as_deref(), Some("claude/sonnet"));
}

#[test]
fn what_the_agent_already_tried_is_excluded_with_its_reason() {
    let a = cand("claude/sonnet", "anthropic", "Claude Sonnet");
    let b = cand("claude/haiku", "anthropic", "Claude Haiku");
    let c = cand("openai/sol", "openai", "Codex Sol");
    let ex = [
        Exclusion {
            model_id: None,
            provider_id: Some("anthropic".into()),
            reason: RejectReason::Exhausted,
        },
        Exclusion {
            model_id: Some("openai/sol".into()),
            provider_id: None,
            reason: RejectReason::Auth,
        },
    ];
    let d = route(&[a, b, c], &req(&ex));
    assert_eq!(d.selected, None);
    assert_eq!(rejected(&d, "claude/sonnet"), Some(RejectReason::Exhausted));
    assert_eq!(rejected(&d, "claude/haiku"), Some(RejectReason::Exhausted));
    assert_eq!(rejected(&d, "openai/sol"), Some(RejectReason::Auth));
    assert!(d.explanation.contains("Ningún modelo es elegible ahora."));
}

#[test]
fn a_probing_provider_is_usable_but_loses_to_a_healthy_one() {
    let mut probing = cand("a/m", "a", "A");
    probing.health = limited(FailureType::TempRateLimit);
    // Pasado el cooldown, el estado vigente es PROBING.
    let later = NOW + symphony_core::RATE_LIMIT_COOLDOWN_MS;
    let healthy = cand("b/m", "b", "B");
    let mut r = req(&[]);
    r.now = later;
    let mut probing_later = probing.clone();
    probing_later.health.updated_at = later;
    let d = route(&[probing_later, healthy], &r);
    assert_eq!(d.selected.as_deref(), Some("b/m"));
    assert!(d.evaluated.iter().all(|e| e.eligible), "ambos elegibles");
    // Ahora mismo, en cooldown, no es elegible.
    let d = route(&[probing], &req(&[]));
    assert_eq!(rejected(&d, "a/m"), Some(RejectReason::Cooldown));
}

#[test]
fn profile_weights_change_the_winner() {
    let mut smart = cand("a/smart", "a", "Smart");
    smart.base_score = 0.95;
    smart.speed = Some(SpeedClass::Slow);
    let mut quick = cand("b/quick", "b", "Quick");
    quick.base_score = 0.5;
    quick.speed = Some(SpeedClass::Fast);

    let code = route(&[smart.clone(), quick.clone()], &req(&[]));
    assert_eq!(code.selected.as_deref(), Some("a/smart"));

    let mut fast = req(&[]);
    fast.profile = "@fast";
    fast.weights = Weights::from_json(
        r#"{"fit":0.4,"context":0.1,"health":0.5,"quota":0.3,"scarcity":0.2,"failures":0.4,"load":0.3,"speed":1.0}"#,
    )
    .unwrap();
    assert_eq!(
        route(&[smart, quick], &fast).selected.as_deref(),
        Some("b/quick")
    );
}

#[test]
fn known_quota_beats_unknown_and_scarcity_is_penalised_for_conserve() {
    let mut rich = cand("a/m", "a", "Rica");
    rich.health = known(0.9);
    let unknown = cand("b/m", "b", "Desconocida");
    assert_eq!(
        route(&[rich.clone(), unknown.clone()], &req(&[]))
            .selected
            .as_deref(),
        Some("a/m")
    );

    // Con 25 % restante y @conserve gana la que no escasea.
    let mut scarce = cand("c/m", "c", "Escasa");
    scarce.health = known(0.25);
    scarce.base_score = 0.9;
    let mut conserve = req(&[]);
    conserve.profile = "@conserve";
    conserve.weights = Weights::from_json(
        r#"{"fit":0.5,"context":0.2,"health":0.5,"quota":1.0,"scarcity":1.0,"failures":0.5,"load":0.3,"speed":0.2}"#,
    )
    .unwrap();
    assert_eq!(
        route(&[scarce, rich], &conserve).selected.as_deref(),
        Some("a/m")
    );
}

#[test]
fn weights_parse_from_the_profile_json() {
    let w = Weights::from_json(r#"{"fit":1.0,"context":0.3,"health":0.5,"quota":0.4,"scarcity":0.4,"failures":0.5,"load":0.2,"speed":0.1}"#).unwrap();
    assert_eq!(w, Weights::default());
    assert_eq!(Weights::from_json("{}").unwrap().fit, 0.0);
    assert!(Weights::from_json("no es json").is_none());
    assert!(Weights::from_json("[1,2]").is_none());
}

fn arb_health() -> impl Strategy<Value = Health> {
    (
        proptest::sample::select(ProviderState::ALL.to_vec()),
        proptest::sample::select(QuotaCertainty::ALL.to_vec()),
        proptest::option::of(0.0f64..1.0),
        proptest::option::of(0i64..3_000_000),
    )
        .prop_map(|(state, certainty, remaining, retry)| Health {
            state,
            certainty,
            remaining: (certainty == QuotaCertainty::Known)
                .then_some(remaining)
                .flatten(),
            retry_after_at: retry,
            reset_at: None,
            evidence: None,
            updated_at: NOW,
        })
}

fn arb_candidate(i: usize) -> impl Strategy<Value = Candidate> {
    (
        arb_health(),
        proptest::option::of(arb_health()),
        0.0f64..1.0,
        proptest::bool::ANY,
        proptest::option::of(1_000u64..2_000_000),
        proptest::option::of(proptest::sample::select(vec![
            SpeedClass::Fast,
            SpeedClass::Medium,
            SpeedClass::Slow,
        ])),
        0u32..9,
        0u32..9,
        proptest::option::of(proptest::sample::select(RejectReason::ALL.to_vec())),
    )
        .prop_map(
            move |(
                health,
                model_health,
                base,
                tools,
                window,
                speed,
                failures,
                load,
                unavailable,
            )| {
                Candidate {
                    model_id: format!("p{}/m{i}", i % 3),
                    provider_id: format!("p{}", i % 3),
                    display_name: format!("M{i}"),
                    unavailable,
                    supports_tools: tools,
                    context_window: window,
                    speed,
                    base_score: base,
                    health,
                    model_health,
                    reserve: 0.2,
                    recent_failures: failures,
                    load,
                }
            },
        )
}

fn arb_candidates() -> impl Strategy<Value = Vec<Candidate>> {
    (1usize..8).prop_flat_map(|n| (0..n).map(arb_candidate).collect::<Vec<_>>())
}

fn arb_weights() -> impl Strategy<Value = Weights> {
    proptest::collection::vec(-2.0f64..2.0, 8).prop_map(|v| Weights {
        fit: v[0],
        context: v[1],
        health: v[2],
        quota: v[3],
        scarcity: v[4],
        failures: v[5],
        load: v[6],
        speed: v[7],
    })
}

proptest! {
    /// Un modelo no elegible nunca gana; y si hay uno elegible, siempre se elige el de mayor puntaje.
    #[test]
    fn an_ineligible_model_never_wins(
        cands in arb_candidates(),
        weights in arb_weights(),
        need in proptest::option::of(0u64..3_000_000),
        tools in proptest::bool::ANY,
        reserve_ok in proptest::bool::ANY,
    ) {
        let r = Request { now: NOW, profile: "@x", weights, needed_context: need, need_tools: tools, allow_reserve: reserve_ok, excluded: &[] };
        let d = route(&cands, &r);
        let eligible: Vec<&Evaluated> = d.evaluated.iter().filter(|e| e.eligible).collect();
        match &d.selected {
            Some(id) => {
                let winner = d.evaluated.iter().find(|e| &e.model_id == id).unwrap();
                prop_assert!(winner.eligible && winner.reject.is_none());
                let best = eligible.iter().map(|e| e.score.unwrap()).fold(f64::MIN, f64::max);
                prop_assert!((winner.score.unwrap() - best).abs() < 1e-12);
            }
            None => prop_assert!(eligible.is_empty(), "había elegibles y no se eligió ninguno"),
        }
        for e in &d.evaluated {
            prop_assert_eq!(e.eligible, e.reject.is_none());
            prop_assert_eq!(e.score.is_some(), e.eligible);
            prop_assert!(e.score.is_none_or(f64::is_finite));
        }
        prop_assert_eq!(d.evaluated.len(), cands.len());
    }

    /// La decisión no depende del orden en que llegan los candidatos.
    #[test]
    fn the_decision_is_independent_of_input_order(cands in arb_candidates(), weights in arb_weights()) {
        let r = Request { now: NOW, profile: "@x", weights, needed_context: None, need_tools: true, allow_reserve: false, excluded: &[] };
        let forward = route(&cands, &r);
        let mut reversed = cands.clone();
        reversed.reverse();
        let backward = route(&reversed, &r);
        prop_assert_eq!(&forward.selected, &backward.selected);
        prop_assert_eq!(&forward.evaluated, &backward.evaluated);
        prop_assert_eq!(&forward.explanation, &backward.explanation);
    }

    /// Un profile automático nunca elige a quien tiene su cuota informada dentro de la reserva.
    #[test]
    fn automatic_profiles_never_spend_a_known_reserve(cands in arb_candidates()) {
        let r = Request { now: NOW, profile: "@x", weights: Weights::default(), needed_context: None, need_tools: false, allow_reserve: false, excluded: &[] };
        let d = route(&cands, &r);
        if let Some(id) = &d.selected {
            let c = cands.iter().find(|c| &c.model_id == id).unwrap();
            let in_reserve = c.health.certainty == QuotaCertainty::Known && c.health.remaining.is_some_and(|x| x <= 0.2);
            prop_assert!(!in_reserve);
        }
    }

    /// Para cualquier entrada la explicación nombra al elegido y no entra en pánico.
    #[test]
    fn the_explanation_names_the_selection(cands in arb_candidates()) {
        let r = Request { now: NOW, profile: "@x", weights: Weights::default(), needed_context: None, need_tools: false, allow_reserve: true, excluded: &[] };
        let d = route(&cands, &r);
        match &d.selected {
            Some(id) => {
                let name = &cands.iter().find(|c| &c.model_id == id).unwrap().display_name;
                let expected = format!("Elegido: {name}");
                prop_assert!(d.explanation.contains(&expected));
            }
            None => prop_assert!(d.explanation.contains("Ningún modelo es elegible")),
        }
    }
}

#[test]
fn routing_twenty_candidates_is_fast_enough_to_run_on_every_event() {
    // Coarse guard para el modo debug de CI (el benchmark real es `cargo bench -p symphony-router`).
    let cands: Vec<Candidate> = (0..20)
        .map(|i| cand(&format!("p{i}/m"), &format!("p{i}"), &format!("M{i}")))
        .collect();
    let r = req(&[]);
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        std::hint::black_box(route(&cands, &r));
    }
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "1000 decisiones de 20 candidatos tardaron {:?}",
        start.elapsed()
    );
}
