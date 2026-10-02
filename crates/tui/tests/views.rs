//! Snapshots de cada vista (FLOW §18) y ramas de FLOW §4, §6, §8, §16, sin daemon:
//! el estado se arma con las mismas respuestas IPC que manda el daemon.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};
use symphony_tui::app::{App, Call, Exec, Failure, Msg, Notice, Req, Screen, Tab};
use symphony_tui::ui;

const NOW: i64 = 1_800_000_000_000;

fn key(code: KeyCode) -> Msg {
    Msg::Key(KeyEvent::from(code))
}

fn press(app: &mut App, code: KeyCode) -> Vec<Call> {
    app.update(key(code))
}

fn typed(app: &mut App, text: &str) {
    for c in text.chars() {
        app.update(key(KeyCode::Char(c)));
    }
}

fn ok(app: &mut App, req: Req, v: Value) -> Vec<Call> {
    app.update(Msg::Reply(req, Ok(v)))
}

fn methods(calls: &[Call]) -> Vec<&str> {
    calls.iter().map(|c| c.method).collect()
}

fn draw(app: &App) -> String {
    let mut t = Terminal::new(TestBackend::new(96, 28)).unwrap();
    t.draw(|f| ui::render(app, f)).unwrap();
    t.backend().to_string()
}

fn status(initialized: bool, recovery: i64) -> Value {
    json!({
        "path": "/work/arete-mobile",
        "is_repo": true,
        "root": "/work/arete-mobile",
        "name": "arete-mobile",
        "branch": "main",
        "initialized": initialized,
        "project_id": if initialized { json!("01PROJECT") } else { Value::Null },
        "recovery_open": recovery,
    })
}

fn providers() -> Value {
    json!({ "providers": [
        { "id": "anthropic", "display_name": "Claude", "setup_state": "READY", "cli_version": "2.1.0",
          "cli_path": "/usr/bin/claude", "enabled": true, "models": 4,
          "model_names": ["Claude Haiku", "Claude Opus", "Claude Sonnet"], "agents_active": 1,
          "health": { "state": "QUOTA_LOW", "stored_state": "QUOTA_LOW", "certainty": "KNOWN", "remaining": 0.15,
                      "evidence": "provider quota report", "retry_after_at": null, "reset_at": NOW + 7_200_000, "reserve": 0.2 },
          "last_failure": null,
          "usage_7d": { "reported_tokens": 1_234_567, "estimated_tokens": 0 } },
        { "id": "openai", "display_name": "Codex", "setup_state": "LOGIN_REQUIRED", "cli_version": "0.40.0",
          "cli_path": "/usr/bin/codex", "enabled": true, "models": 0,
          "model_names": [], "agents_active": 0,
          "health": { "state": "UNKNOWN", "stored_state": "UNKNOWN", "certainty": "UNKNOWN", "remaining": null,
                      "evidence": null, "retry_after_at": null, "reset_at": null, "reserve": 0.2 },
          "last_failure": null, "usage_7d": { "reported_tokens": 0, "estimated_tokens": 0 } },
        { "id": "github", "display_name": "Copilot", "setup_state": "READY", "cli_version": "1.0.60",
          "cli_path": "/usr/bin/copilot", "enabled": true, "models": 1,
          "model_names": ["Copilot (automático)"], "agents_active": 0,
          "health": { "state": "RATE_LIMITED", "stored_state": "RATE_LIMITED", "certainty": "ESTIMATED", "remaining": null,
                      "evidence": "TEMP_RATE_LIMIT x1", "retry_after_at": NOW + 45_000, "reset_at": null, "reserve": 0.2 },
          "last_failure": { "type": "TEMP_RATE_LIMIT", "message": "429 Too Many Requests", "at": NOW - 180_000 },
          "usage_7d": { "reported_tokens": 0, "estimated_tokens": 48_000 } },
    ]})
}

fn agents() -> Value {
    json!({ "agents": [
        { "agent_id": "01AGENT1", "number": 1, "state": "RUNNING", "state_reason": null,
          "task_code": "T-1", "task_title": "Supabase authentication", "provider_id": "anthropic", "model_id": "claude/sonnet" },
        { "agent_id": "01AGENT2", "number": 2, "state": "WAITING_PROVIDER",
          "state_reason": "Codex agotó su cuota y el failover está desactivado.",
          "task_code": "T-2", "task_title": "Responsive modals", "provider_id": null, "model_id": null },
        { "agent_id": "01AGENT3", "number": 3, "state": "COMPLETED", "state_reason": null,
          "task_code": "T-3", "task_title": "Integration tests", "provider_id": null, "model_id": null },
    ]})
}

fn recovery() -> Value {
    json!({ "items": [
        { "id": "01REC1", "kind": "EXECUTOR_EXITED", "detail": "Claude terminó con código 1.",
          "created_at": NOW - 90_000, "agent_id": "01AGENT2", "agent_number": 2, "agent_state": "FAILED" },
        { "id": "01REC2", "kind": "SESSION_INTERRUPTED", "detail": "La sesión anterior se cortó sin cerrarse.",
          "created_at": NOW - 7_200_000, "agent_id": null, "agent_number": null, "agent_state": null },
    ]})
}

fn models() -> Value {
    json!({ "models": [
        { "id": "claude/opus", "display_name": "Claude Opus", "provider_id": "anthropic", "provider": "Claude",
          "setup_state": "READY", "available": true },
        { "id": "claude/sonnet", "display_name": "Claude Sonnet", "provider_id": "anthropic", "provider": "Claude",
          "setup_state": "READY", "available": true },
        { "id": "codex/gpt-5", "display_name": "GPT-5", "provider_id": "openai", "provider": "Codex",
          "setup_state": "LOGIN_REQUIRED", "available": false },
    ]})
}

fn profiles() -> Value {
    json!({ "profiles": [
        { "id": "@code", "description": "Best available for implementation", "weights": {} },
        { "id": "@fast", "description": "Prioritize response speed", "weights": {} },
        { "id": "@conserve", "description": "Preserve scarce quota", "weights": {} },
    ]})
}

fn explain() -> Value {
    json!({ "agent_id": "01AGENT1", "decisions": [
        { "id": "01D2", "trigger": "FAILOVER", "profile": "@code", "selected": "openai/sol", "decided_at": NOW - 120_000,
          "explanation": "Profile: @code\nClaude Sonnet — descartado: cuota agotada (o ya agotada en este agente)\nCodex Sol — elegible · salud HEALTHY · cuota desconocida\nKimi — descartado: en espera tras un límite temporal\nElegido: Codex Sol\nPor qué:\n+ buen ajuste con el profile\n+ proveedor sano",
          "candidates": [
            { "model_id": "openai/sol", "eligible": true, "reject_reason": null, "score": 1.42, "factors": {} },
            { "model_id": "claude/sonnet", "eligible": false, "reject_reason": "EXHAUSTED", "score": null, "factors": null },
            { "model_id": "moonshot/default", "eligible": false, "reject_reason": "COOLDOWN", "score": null, "factors": null },
          ] },
        { "id": "01D1", "trigger": "SPAWN", "profile": null, "selected": "claude/sonnet", "decided_at": NOW - 900_000,
          "explanation": "Modelo exacto elegido por el usuario: claude/sonnet.", "candidates": [] },
    ]})
}

fn inspect() -> Value {
    json!({
        "agent": { "id": "01AGENT1", "number": 1, "state": "RUNNING", "state_reason": null,
                   "failover_policy": "ANY", "context_mode": "BALANCED" },
        "task": { "code": "T-1", "title": "Fix refresh token expiration" },
        "worktree": { "path": "/home/leo/.symphony/worktrees/p/agent-001", "branch": "symphony/8e4f/agent-001" },
        "current_run": { "seq": 2, "model_id": "codex/gpt-5", "status": "RUNNING" },
        "latest_checkpoint": { "seq": 14, "created_at": NOW - 14_000,
            "current_step": "Running targeted tests...",
            "next_step": "Investigate refresh middleware response",
            "plan_tail": "1. Reproduce\n2. Fix rotation\n- [ ] Investigate refresh middleware response" },
        "runs_count": 2,
    })
}

/// Home de un proyecto configurado, con agentes y un problema abierto.
fn home_app() -> App {
    let mut app = App::new("/work/arete-mobile");
    app.now_ms = NOW;
    app.start();
    ok(&mut app, Req::Providers, providers());
    ok(&mut app, Req::ProjectStatus, status(true, 0));
    ok(&mut app, Req::Agents, agents());
    ok(&mut app, Req::Recovery, json!({ "items": [] }));
    ok(&mut app, Req::Providers, providers());
    // El inicio es el Chat (P07.5.S7); Esc lleva a agentes y tareas.
    assert_eq!(app.screen, Screen::Chat);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen, Screen::Home);
    ok(&mut app, Req::Agents, agents());
    ok(&mut app, Req::Providers, providers());
    ok(&mut app, Req::Recovery, json!({ "items": [] }));
    ok(&mut app, Req::SkillsAnthropic, json!({ "skills": [] }));
    ok(&mut app, Req::SkillsOpenai, json!({ "skills": [] }));
    app
}

fn agent_app(tab: Tab) -> App {
    let mut app = home_app();
    let calls = press(&mut app, KeyCode::Enter);
    assert_eq!(methods(&calls), ["agent.inspect"]);
    ok(&mut app, Req::Inspect, inspect());
    let a = app.agent.as_mut().unwrap();
    a.tab = tab;
    a.messages = serde_json::from_value(json!([
        { "role": "USER", "content": "Fix refresh token expiration" },
        { "role": "ASSISTANT", "content": "Voy a reproducir el bug primero.\nNext: correr los tests" },
        { "role": "EXECUTOR_CHANGE", "content": "── Executor changed: Claude / sonnet → Codex / gpt-5\nReason: quota exhausted\nAgent #1, task and workspace unchanged\nHandoff restored from checkpoint #14" },
        { "role": "ASSISTANT", "content": "Sigo con los tests del middleware." },
    ]))
    .unwrap();
    a.activity = serde_json::from_value(json!([
        { "kind": "tool", "at": NOW - 5_000, "tool": "Bash", "command": "npm test -- auth", "status": "FAILED", "exit_code": 1 },
        { "kind": "checkpoint", "at": NOW - 14_000, "seq": 14, "current_step": "Running targeted tests..." },
        { "kind": "tool", "at": NOW - 30_000, "tool": "Edit", "command": null, "status": "DONE", "exit_code": null },
    ]))
    .unwrap();
    a.diff = "diff --git a/src/auth.ts b/src/auth.ts\n--- a/src/auth.ts\n+++ b/src/auth.ts\n@@ -1,3 +1,4 @@\n import { token } from './t';\n-const ttl = 60;\n+const ttl = 3600;\n+export const rotate = true;\n".into();
    a.history = json!({
        "runs": [
            { "seq": 1, "model_id": "claude/sonnet", "status": "EXITED", "end_reason": "QUOTA_EXHAUSTED" },
            { "seq": 2, "model_id": "codex/gpt-5", "status": "RUNNING", "end_reason": null },
        ],
        "changes": [
            { "reason": "FAILOVER", "at": NOW - 60_000, "checkpoint_age_ms": 8_000,
              "from_model": "claude/sonnet", "to_model": "codex/gpt-5", "failure": "DAILY_QUOTA" },
        ],
    });
    app
}

// --- snapshots ---------------------------------------------------------------

#[test]
fn view_01_launch_checking() {
    let mut app = App::new("/work/arete-mobile");
    assert_eq!(methods(&app.start()), ["project.status", "providers.list"]);
    insta::assert_snapshot!(draw(&app));
}

#[test]
fn view_01_launch_project_without_symphony() {
    let mut app = App::new("/work/arete-mobile");
    app.start();
    ok(&mut app, Req::Providers, providers());
    ok(&mut app, Req::ProjectStatus, status(false, 0));
    assert_eq!(app.screen, Screen::Launch);
    insta::assert_snapshot!(draw(&app));
}

#[test]
fn view_01_launch_not_a_repo() {
    let mut app = App::new("/tmp/fotos");
    app.start();
    ok(
        &mut app,
        Req::ProjectStatus,
        json!({ "path": "/tmp/fotos", "is_repo": false }),
    );
    assert_eq!(app.screen, Screen::NotARepo);
    insta::assert_snapshot!(draw(&app));
    press(&mut app, KeyCode::Char('q'));
    assert!(app.quit);
}

#[test]
fn view_02_first_run() {
    let mut app = App::new("/work/arete-mobile");
    app.start();
    ok(&mut app, Req::Providers, providers());
    ok(&mut app, Req::ProjectStatus, status(false, 0));
    press(&mut app, KeyCode::Char('i'));
    assert_eq!(app.screen, Screen::FirstRun);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Left); // Balanced → Eco
    insta::assert_snapshot!(draw(&app));
}

#[test]
fn view_03_provider_setup() {
    let mut app = App::new("/work/arete-mobile");
    app.start();
    ok(&mut app, Req::Providers, providers());
    ok(&mut app, Req::ProjectStatus, status(false, 0));
    press(&mut app, KeyCode::Char('i'));
    for _ in 0..3 {
        press(&mut app, KeyCode::Enter);
    }
    let calls = press(&mut app, KeyCode::Enter);
    assert_eq!(methods(&calls), ["project.init"]);
    assert_eq!(calls[0].params["name"], "arete-mobile");
    assert_eq!(calls[0].params["performance"], "BALANCED");
    assert_eq!(calls[0].params["failover"], "ANY");
    let calls = ok(
        &mut app,
        Req::ProjectInit,
        json!({ "root": "/work/arete-mobile", "name": "arete-mobile" }),
    );
    // Codex necesita login → Provider Setup antes del Home.
    assert_eq!(app.screen, Screen::ProviderSetup);
    assert_eq!(methods(&calls), ["project.status", "providers.list"]);
    press(&mut app, KeyCode::Down);
    insta::assert_snapshot!(draw(&app));
    // Se puede seguir con un solo proveedor.
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::Chat);
}

#[test]
fn view_04_home_empty() {
    let mut app = home_app();
    ok(&mut app, Req::Agents, json!({ "agents": [] }));
    insta::assert_snapshot!(draw(&app));
}

#[test]
fn view_04_home_with_agents_and_a_problem() {
    let mut app = home_app();
    ok(&mut app, Req::Recovery, recovery());
    press(&mut app, KeyCode::Down);
    insta::assert_snapshot!(draw(&app));
}

#[test]
fn view_05_new_agent() {
    let mut app = home_app();
    press(&mut app, KeyCode::Char('n'));
    assert_eq!(app.screen, Screen::NewAgent);
    typed(
        &mut app,
        "Fix refresh token rotation and add regression tests",
    );
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Right); // exacto
    insta::assert_snapshot!(draw(&app));
}

#[test]
fn view_13_model_picker() {
    let mut app = home_app();
    press(&mut app, KeyCode::Char('n'));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Right);
    let calls = press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::ModelPicker);
    assert_eq!(methods(&calls), ["models.list", "profiles.list"]);
    ok(&mut app, Req::Models, models());
    ok(&mut app, Req::Profiles, profiles());
    // Los profiles van primero y luego los modelos exactos, con su estado de salud.
    for _ in 0..4 {
        press(&mut app, KeyCode::Down);
    }
    insta::assert_snapshot!(draw(&app));
}

#[test]
fn picking_a_profile_for_a_new_agent_creates_it_with_that_profile() {
    let mut app = home_app();
    app.new_agent.task = "Arreglar el login".into();
    press(&mut app, KeyCode::Char('n'));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Right); // Later → Exact
    press(&mut app, KeyCode::Right); // Exact → Profile
    assert_eq!(app.new_agent.exec, Exec::Profile);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::ModelPicker);
    ok(&mut app, Req::Models, models());
    ok(&mut app, Req::Profiles, profiles());
    press(&mut app, KeyCode::Down); // @fast
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::NewAgent);
    assert_eq!(app.new_agent.profile.as_deref(), Some("@fast"));
    // Crear manda el profile, no un modelo.
    app.new_agent.field = 4;
    let calls = press(&mut app, KeyCode::Enter);
    let create = calls.iter().find(|c| c.method == "agent.create").unwrap();
    assert_eq!(create.params["profile"], "@fast");
    assert!(create.params["model"].is_null());
}

#[test]
fn creating_with_a_profile_requires_choosing_one() {
    let mut app = home_app();
    app.new_agent.task = "x".into();
    app.new_agent.exec = Exec::Profile;
    app.new_agent.field = 4;
    let calls = press(&mut app, KeyCode::Enter);
    assert!(calls.iter().all(|c| c.method != "agent.create"));
}

#[test]
fn switching_by_profile_sends_the_profile_to_the_daemon() {
    let mut app = agent_app(Tab::Overview);
    press(&mut app, KeyCode::Char('s'));
    assert_eq!(app.screen, Screen::ModelPicker);
    ok(&mut app, Req::Models, models());
    ok(&mut app, Req::Profiles, profiles());
    let calls = press(&mut app, KeyCode::Enter); // el primer profile
    let switch = calls.iter().find(|c| c.method == "agent.switch").unwrap();
    assert_eq!(switch.params["profile"], "@code");
    assert!(switch.params.get("model").is_none());
}

#[test]
fn view_14_explain_route() {
    let mut app = agent_app(Tab::Overview);
    let calls = press(&mut app, KeyCode::Char('?'));
    assert_eq!(app.screen, Screen::ExplainRoute);
    assert_eq!(methods(&calls), ["route.explain"]);
    ok(&mut app, Req::Explain, explain());
    insta::assert_snapshot!(draw(&app));
    // Esc vuelve al agente.
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen, Screen::Agent);
}

#[test]
fn explain_route_without_decisions_says_so() {
    let mut app = agent_app(Tab::Overview);
    press(&mut app, KeyCode::Char('?'));
    ok(
        &mut app,
        Req::Explain,
        json!({ "agent_id": "01AGENT1", "decisions": [] }),
    );
    assert!(draw(&app).contains("todavía no tiene decisiones de routing"));
}

#[test]
fn view_06_agent_overview() {
    insta::assert_snapshot!(draw(&agent_app(Tab::Overview)));
}

#[test]
fn view_06_agent_waiting_for_an_executor() {
    let mut app = agent_app(Tab::Overview);
    let mut ready = inspect();
    ready["agent"]["state"] = json!("READY");
    ready["current_run"] = Value::Null;
    ready["runs_count"] = json!(0);
    ready["runs"] = json!([]);
    ok(&mut app, Req::Inspect, ready);
    let screen = draw(&app);
    assert!(screen.contains("pulsa s para elegir un modelo"), "{screen}");
    assert!(screen.contains("sin executor"), "{screen}");

    // Terminado: muestra el último executor que tuvo.
    let mut done = inspect();
    done["agent"]["state"] = json!("COMPLETED");
    done["current_run"] = Value::Null;
    done["runs"] = json!([{ "seq": 1, "model_id": "claude/haiku", "status": "EXITED" }]);
    ok(&mut app, Req::Inspect, done);
    let screen = draw(&app);
    assert!(
        screen.contains("claude/haiku (run #1 terminado)"),
        "{screen}"
    );
    assert!(!screen.contains("pulsa s para elegir"), "{screen}");
}

#[test]
fn view_07_agent_conversation_with_executor_change() {
    let mut app = agent_app(Tab::Conversation);
    press(&mut app, KeyCode::Char('m'));
    typed(&mut app, "revisa también el logout");
    insta::assert_snapshot!(draw(&app));
}

#[test]
fn view_08_agent_activity() {
    insta::assert_snapshot!(draw(&agent_app(Tab::Activity)));
}

#[test]
fn view_09_agent_changes() {
    insta::assert_snapshot!(draw(&agent_app(Tab::Changes)));
}

#[test]
fn view_12_24_agent_history_and_failover_event() {
    insta::assert_snapshot!(draw(&agent_app(Tab::History)));
}

#[test]
fn view_22_providers() {
    let mut app = home_app();
    let calls = press(&mut app, KeyCode::Char('p'));
    assert_eq!(methods(&calls), ["providers.list"]);
    insta::assert_snapshot!(draw(&app));
}

#[test]
fn view_22_provider_detail_with_a_temporary_limit_and_estimated_quota() {
    let mut app = home_app();
    press(&mut app, KeyCode::Char('p'));
    ok(&mut app, Req::Providers, providers());
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    insta::assert_snapshot!(draw(&app));
}

#[test]
fn view_30_recovery_center() {
    let mut app = home_app();
    press(&mut app, KeyCode::Char('r'));
    ok(&mut app, Req::Recovery, recovery());
    insta::assert_snapshot!(draw(&app));
}

// --- ramas de FLOW -------------------------------------------------------------

#[test]
fn inconsistent_state_opens_recovery_before_home() {
    let mut app = App::new("/work/arete-mobile");
    app.start();
    let calls = ok(&mut app, Req::ProjectStatus, status(true, 2));
    assert_eq!(app.screen, Screen::Recovery);
    assert_eq!(methods(&calls), ["recovery.list", "agent.list"]);
}

#[test]
fn open_without_setup_goes_home_without_listing_other_projects() {
    let mut app = App::new("/work/arete-mobile");
    app.start();
    ok(&mut app, Req::ProjectStatus, status(false, 0));
    let calls = press(&mut app, KeyCode::Char('o'));
    assert_eq!(app.screen, Screen::Chat);
    // Sin proyecto en la base, `agent.list` traería agentes de otros proyectos.
    assert_eq!(
        methods(&calls),
        ["chat.get", "models.list", "skills.list", "skills.list"]
    );
}

#[test]
fn no_eligible_provider_keeps_the_task_and_comes_back() {
    let mut app = home_app();
    press(&mut app, KeyCode::Char('n'));
    typed(&mut app, "Agregar login");
    for _ in 0..4 {
        press(&mut app, KeyCode::Enter);
    }
    let calls = press(&mut app, KeyCode::Enter);
    assert_eq!(methods(&calls), ["agent.create"]);
    assert_eq!(calls[0].params["model"], Value::Null);
    let calls = app.update(Msg::Reply(
        Req::Create,
        Err(Failure {
            code: "no_eligible_provider".into(),
            message: "No hay ningún proveedor listo.".into(),
        }),
    ));
    assert_eq!(app.screen, Screen::ProviderSetup);
    assert_eq!(methods(&calls), ["providers.list"]);
    assert_eq!(app.new_agent.task, "Agregar login");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::NewAgent);
    assert_eq!(app.new_agent.task, "Agregar login");
}

#[test]
fn an_unavailable_exact_model_is_never_substituted() {
    let mut app = home_app();
    press(&mut app, KeyCode::Char('n'));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Enter);
    ok(&mut app, Req::Models, models());
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    let calls = press(&mut app, KeyCode::Enter);
    assert!(calls.is_empty());
    assert_eq!(app.screen, Screen::ModelPicker);
    assert!(matches!(&app.notice, Some(Notice::Error(e)) if e.contains("codex/gpt-5")));
    assert_eq!(app.new_agent.model, None);
    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::NewAgent);
    assert_eq!(app.new_agent.model.as_deref(), Some("claude/sonnet"));
}

#[test]
fn spawn_task_starting_with_a_path_is_not_a_model() {
    let mut app = home_app();
    app.update(key(KeyCode::Char(':')));
    typed(&mut app, "spawn src/main.rs crashes on empty input");
    let calls = press(&mut app, KeyCode::Enter);
    assert_eq!(methods(&calls), ["agent.create"]);
    assert_eq!(
        calls[0].params["title"],
        "src/main.rs crashes on empty input"
    );
    assert!(calls[0].params["model"].is_null());
}

#[test]
fn created_agent_opens_its_view() {
    let mut app = home_app();
    let calls = app.update(key(KeyCode::Char(':')));
    assert!(calls.is_empty());
    typed(&mut app, "spawn claude/sonnet implement auth");
    let calls = press(&mut app, KeyCode::Enter);
    assert_eq!(methods(&calls), ["agent.create"]);
    assert_eq!(calls[0].params["model"], "claude/sonnet");
    assert_eq!(calls[0].params["title"], "implement auth");
    let calls = ok(
        &mut app,
        Req::Create,
        json!({ "agent_id": "01NEW", "number": 4 }),
    );
    assert_eq!(app.screen, Screen::Agent);
    assert_eq!(methods(&calls), ["project.status", "agent.inspect"]);
}

#[test]
fn stop_needs_confirmation_and_switch_uses_the_picker() {
    let mut app = agent_app(Tab::Overview);
    assert!(press(&mut app, KeyCode::Char('x')).is_empty());
    let calls = press(&mut app, KeyCode::Char('x'));
    assert_eq!(methods(&calls), ["agent.stop"]);

    press(&mut app, KeyCode::Char('x'));
    press(&mut app, KeyCode::Char('j')); // otra tecla cancela la confirmación
    assert!(press(&mut app, KeyCode::Char('x')).is_empty());

    press(&mut app, KeyCode::Char('s'));
    assert_eq!(app.screen, Screen::ModelPicker);
    ok(&mut app, Req::Models, models());
    let calls = press(&mut app, KeyCode::Enter);
    assert_eq!(methods(&calls), ["agent.switch"]);
    assert_eq!(calls[0].params["model"], "claude/opus");
    assert_eq!(app.screen, Screen::Agent);
}

#[test]
fn pause_toggles_with_the_state() {
    let mut app = agent_app(Tab::Overview);
    assert_eq!(
        methods(&press(&mut app, KeyCode::Char('p'))),
        ["agent.pause"]
    );
    let mut paused = inspect();
    paused["agent"]["state"] = json!("PAUSED");
    ok(&mut app, Req::Inspect, paused);
    assert_eq!(
        methods(&press(&mut app, KeyCode::Char('p'))),
        ["agent.resume"]
    );
}

#[test]
fn tabs_fetch_their_data() {
    let mut app = agent_app(Tab::Overview);
    assert_eq!(methods(&press(&mut app, KeyCode::Right)), ["agent.logs"]);
    assert_eq!(
        methods(&press(&mut app, KeyCode::Right)),
        ["agent.activity"]
    );
    assert_eq!(
        methods(&press(&mut app, KeyCode::Char('d'))),
        ["agent.diff"]
    );
    assert_eq!(
        methods(&press(&mut app, KeyCode::Char('5'))),
        ["agent.history"]
    );
    let calls = press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen, Screen::Home);
    assert!(methods(&calls).contains(&"agent.list"));
}

#[test]
fn bus_events_refresh_on_the_next_tick_without_piling_up() {
    let mut app = home_app();
    app.update(Msg::Event("agent.event".into()));
    let calls = app.update(Msg::Tick(NOW));
    assert_eq!(
        methods(&calls),
        ["agent.list", "providers.list", "recovery.list"]
    );
    // Con respuestas pendientes no se piden más.
    app.update(Msg::Event("agent.event".into()));
    assert!(app.update(Msg::Tick(NOW)).is_empty());
    ok(&mut app, Req::Agents, agents());
    ok(&mut app, Req::Providers, providers());
    ok(&mut app, Req::Recovery, json!({ "items": [] }));
    assert_eq!(app.update(Msg::Tick(NOW)).len(), 3);
}

#[test]
fn recovery_actions_and_disconnection() {
    let mut app = home_app();
    press(&mut app, KeyCode::Char('r'));
    ok(&mut app, Req::Recovery, recovery());
    let calls = press(&mut app, KeyCode::Char('r'));
    assert_eq!(
        calls[0].params,
        json!({ "id": "01REC1", "action": "restart" })
    );
    press(&mut app, KeyCode::Down);
    let calls = press(&mut app, KeyCode::Char('x'));
    assert_eq!(
        calls[0].params,
        json!({ "id": "01REC2", "action": "dismiss" })
    );

    app.update(Msg::Reply(
        Req::Recovery,
        Err(Failure {
            code: "disconnected".into(),
            message: "el daemon cerró la conexión".into(),
        }),
    ));
    assert!(!app.connected);
    assert!(app.update(Msg::Tick(NOW)).is_empty());
    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    app.update(Msg::Key(ctrl_c));
    assert!(app.quit);
}

#[test]
fn open_in_the_cli_reports_the_terminal_or_the_command() {
    let mut app = agent_app(Tab::Overview);
    let calls = press(&mut app, KeyCode::Char('o'));
    assert_eq!(methods(&calls), ["agent.attach"]);
    assert_eq!(calls[0].params, json!({ "agent": "01AGENT1" }));
    ok(
        &mut app,
        Req::Attach,
        json!({ "cli": "Claude Code", "command": "claude --resume s1", "opened": true, "error": null }),
    );
    assert!(matches!(&app.notice, Some(Notice::Info(i)) if i.contains("Claude Code")));

    press(&mut app, KeyCode::Char('o'));
    ok(
        &mut app,
        Req::Attach,
        json!({ "cli": "Claude Code", "command": "claude --resume s1", "opened": false,
                "error": "no se pudo abrir una terminal" }),
    );
    assert!(
        matches!(&app.notice, Some(Notice::Error(e)) if e.contains("claude --resume s1")),
        "{:?}",
        app.notice
    );
}

/// Chat de un proyecto configurado, recién abierto (sin conversación).
fn chat_app() -> App {
    let mut app = App::new("/work/arete-mobile");
    app.now_ms = NOW;
    app.start();
    ok(&mut app, Req::Providers, providers());
    ok(&mut app, Req::ProjectStatus, status(true, 0));
    ok(&mut app, Req::Models, models());
    ok(&mut app, Req::ChatGet, json!({ "agent_id": null }));
    app
}

#[test]
fn view_00_chat_empty() {
    insta::assert_snapshot!(draw(&chat_app()));
}

#[test]
fn chat_first_message_creates_the_chat_then_sends_and_switches() {
    let mut app = chat_app();
    for c in "hola".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    let calls = press(&mut app, KeyCode::Enter);
    assert_eq!(methods(&calls), ["agent.create"]);
    assert!(app.chat_input.is_empty());
    ok(&mut app, Req::ChatCreate, json!({ "agent_id": "01CHAT" }));
    assert!(app.chat.is_some());
    ok(&mut app, Req::Inspect, inspect());

    // Mismo modelo → agent.send; otro modelo (Tab) → agent.switch con el mensaje.
    let cur = app.chat.as_ref().unwrap().model().unwrap().to_string();
    app.chat_model = Some(cur);
    press(&mut app, KeyCode::Char('x'));
    assert_eq!(methods(&press(&mut app, KeyCode::Enter)), ["agent.send"]);
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Char('y'));
    assert_eq!(methods(&press(&mut app, KeyCode::Enter)), ["agent.switch"]);
}

#[test]
fn chat_scroll_never_goes_below_the_end() {
    let mut app = chat_app();
    app.update(Msg::Scroll(-3));
    assert_eq!(app.chat_scroll, 0);
    app.update(Msg::Scroll(3));
    assert_eq!(app.chat_scroll, 3);
}

#[test]
fn reopening_the_chat_keeps_its_model_so_a_plain_message_does_not_switch() {
    // `models.list` llega antes que el `inspect` del chat existente.
    let mut app = chat_app();
    assert_eq!(app.chat_model.as_deref(), Some("claude/opus"));
    ok(&mut app, Req::ChatGet, json!({ "agent_id": "01CHAT" }));
    assert_eq!(app.chat_model, None);
    ok(&mut app, Req::Inspect, inspect());
    assert_eq!(app.chat_model.as_deref(), Some("codex/gpt-5"));
    press(&mut app, KeyCode::Char('x'));
    assert_eq!(methods(&press(&mut app, KeyCode::Enter)), ["agent.send"]);
}

#[test]
fn chat_scroll_stops_at_the_top_the_render_reports() {
    let mut app = chat_app();
    app.chat_max.set(5);
    for _ in 0..3 {
        press(&mut app, KeyCode::PageUp);
    }
    assert_eq!(app.chat_scroll, 5);
    press(&mut app, KeyCode::Down);
    assert_eq!(app.chat_scroll, 4);
}

#[test]
fn chat_long_input_keeps_the_tail_visible() {
    let mut app = chat_app();
    app.chat_input = format!("{}FINAL", "palabra ".repeat(40));
    assert!(draw(&app).contains("FINAL▏"));
}

#[test]
fn agent_conversation_wraps_long_messages_instead_of_cutting_them() {
    let mut app = agent_app(Tab::Conversation);
    let long = format!("{}FIN-DEL-TEXTO", "palabra ".repeat(40));
    app.agent.as_mut().unwrap().messages = serde_json::from_value(json!([
        { "role": "ASSISTANT", "content": long },
    ]))
    .unwrap();
    assert!(draw(&app).contains("FIN-DEL-TEXTO"));
}

#[test]
fn chat_shows_the_agent_process_live_in_time_order() {
    let mut app = chat_app();
    ok(&mut app, Req::ChatGet, json!({ "agent_id": "01CHAT" }));
    let chat = app.chat.as_mut().unwrap();
    chat.inspect = json!({ "agent": { "state": "RUNNING" } });
    chat.messages = serde_json::from_value(json!([
        { "role": "USER", "content": "corre los tests", "at": NOW - 6_000 },
        { "role": "ASSISTANT", "content": "Voy a correr el chequeo.", "at": NOW - 5_000 },
    ]))
    .unwrap();
    // `agent.activity` trae lo más nuevo primero.
    chat.activity = serde_json::from_value(json!([
        { "kind": "tool", "tool": "Bash", "command": "cargo test", "status": "RUNNING", "at": NOW - 2_000 },
        { "kind": "tool", "tool": "Read", "command": "STATUS.md", "status": "DONE", "at": NOW - 4_000 },
        { "kind": "checkpoint", "seq": 1, "at": NOW - 3_000 },
    ]))
    .unwrap();
    assert!(app.animating());
    let screen = draw(&app);
    let at = |s: &str| screen.find(s).unwrap_or_else(|| panic!("falta `{s}`"));
    assert!(
        at("corre los tests") < at("Voy a correr") && at("Voy a correr") < at("Read STATUS.md")
    );
    assert!(at("Read STATUS.md") < at("Bash cargo test"));
    assert!(!screen.contains("checkpoint"));
    assert!(screen.contains("ejecutando Bash… 2 s"));
}

#[test]
fn chat_at_rest_does_not_animate() {
    let mut app = chat_app();
    assert!(!app.animating());
    ok(&mut app, Req::ChatGet, json!({ "agent_id": "01CHAT" }));
    app.chat.as_mut().unwrap().inspect = json!({ "agent": { "state": "READY" } });
    assert!(!app.animating());
}

#[test]
fn pasting_multiple_lines_fills_the_input_without_sending() {
    let mut app = chat_app();
    let calls = app.update(Msg::Paste("uno\r\ndos\ntres".into()));
    assert!(calls.is_empty());
    assert_eq!(app.chat_input, "uno\ndos\ntres");
}

fn ctrl(app: &mut App, c: char) -> Vec<Call> {
    app.update(Msg::Key(KeyEvent::new(
        KeyCode::Char(c),
        KeyModifiers::CONTROL,
    )))
}

#[test]
fn ctrl_y_copies_the_last_agent_reply_and_says_so_when_there_is_none() {
    let mut app = chat_app();
    ctrl(&mut app, 'y');
    assert!(app.copy.is_none());
    assert!(matches!(app.notice, Some(Notice::Error(_))));
    ok(&mut app, Req::ChatGet, json!({ "agent_id": "01CHAT" }));
    app.chat.as_mut().unwrap().messages = serde_json::from_value(json!([
        { "role": "ASSISTANT", "content": "primera" },
        { "role": "USER", "content": "y luego" },
        { "role": "ASSISTANT", "content": "la ultima" },
    ]))
    .unwrap();
    ctrl(&mut app, 'y');
    assert_eq!(app.copy.as_deref(), Some("la ultima"));
}

#[test]
fn f2_toggles_the_mouse_capture_for_selecting_text() {
    let mut app = chat_app();
    assert!(app.mouse);
    press(&mut app, KeyCode::F(2));
    assert!(!app.mouse);
    press(&mut app, KeyCode::F(2));
    assert!(app.mouse);
}

#[test]
fn a_pasted_newline_is_text_in_the_chat_and_enter_elsewhere() {
    let mut app = chat_app();
    for c in ['a', symphony_tui::NEWLINE, 'b'] {
        press(&mut app, KeyCode::Char(c));
    }
    assert_eq!(app.chat_input, "a\nb");
    // Fuera del chat no hay texto multilínea: cuenta como Enter (aquí, entrar a agentes vacío).
    let mut home = home_app();
    let calls = press(&mut home, KeyCode::Char(symphony_tui::NEWLINE));
    assert_eq!(calls, press(&mut home_app(), KeyCode::Enter));
}

#[test]
fn agent_conversation_scrolls_back_with_up_and_the_wheel_within_its_limits() {
    let mut app = agent_app(Tab::Conversation);
    let long: Vec<Value> = (0..40)
        .map(|i| json!({ "role": "ASSISTANT", "content": format!("mensaje {i}") }))
        .collect();
    app.agent.as_mut().unwrap().messages = long;
    let top_after = |app: &App| draw(app);
    assert!(top_after(&app).contains("mensaje 39"));
    // Up va hacia lo más viejo; la rueda hacia atrás también; ↓ vuelve a lo nuevo.
    press(&mut app, KeyCode::PageUp);
    assert!(!top_after(&app).contains("mensaje 39"));
    app.update(Msg::Scroll(3));
    let up = app.agent.as_ref().unwrap().scroll;
    assert!(up >= 13);
    // Con el tope que fijó el render, pasarse no acumula pulsaciones muertas.
    for _ in 0..50 {
        press(&mut app, KeyCode::PageUp);
    }
    let max = app.agent.as_ref().unwrap().scroll_max.get();
    assert_eq!(app.agent.as_ref().unwrap().scroll, max);
    press(&mut app, KeyCode::Down);
    assert_eq!(app.agent.as_ref().unwrap().scroll, max - 1);
    for _ in 0..200 {
        press(&mut app, KeyCode::Down);
    }
    assert!(draw(&app).contains("mensaje 39"));
}

#[test]
fn export_is_ctrl_e_in_the_chat_and_e_in_the_agent_view() {
    let mut app = chat_app();
    // Sin chat todavía no hay nada que exportar.
    assert!(ctrl(&mut app, 'e').is_empty());
    ok(&mut app, Req::ChatGet, json!({ "agent_id": "01CHAT" }));
    assert_eq!(methods(&ctrl(&mut app, 'e')), ["agent.export"]);
    ok(
        &mut app,
        Req::Export,
        json!({ "path": "/p/.symphony/exports/chat-1.md" }),
    );
    assert!(matches!(&app.notice, Some(Notice::Info(t)) if t.contains("chat-1.md")));

    let mut app = agent_app(Tab::Conversation);
    assert_eq!(
        methods(&press(&mut app, KeyCode::Char('e'))),
        ["agent.export"]
    );
}

/// Chat con catálogo de skills de Claude y de Codex ya cargado.
fn skills_app() -> App {
    let mut app = chat_app();
    app.skills.insert(
        "anthropic".into(),
        serde_json::from_value(json!([
            { "name": "deslop", "description": "Limpia código", "source": "usuario" },
            { "name": "flintstone", "description": "Respuestas breves", "source": "usuario" },
            { "name": "ponytail:review", "description": "Revisa sobre-ingeniería", "source": "plugin" },
        ]))
        .unwrap(),
    );
    app.skills.insert(
        "openai".into(),
        serde_json::from_value(
            json!([{ "name": "deslop", "description": "Limpia", "source": "usuario" }]),
        )
        .unwrap(),
    );
    app.chat_model = Some("claude/sonnet".into());
    app
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
}

fn names(app: &App) -> Vec<String> {
    app.skill_matches()
        .iter()
        .map(|s| s["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn slash_opens_the_claude_catalog_and_filters_while_typing() {
    let mut app = skills_app();
    type_text(&mut app, "hola");
    assert!(names(&app).is_empty());
    let mut app = skills_app();
    type_text(&mut app, "/");
    assert_eq!(names(&app), ["deslop", "flintstone", "ponytail:review"]);
    assert!(draw(&app).contains("/flintstone"));
    type_text(&mut app, "fl");
    assert_eq!(names(&app), ["flintstone"]);
    // Coincidencias por el inicio primero, luego las que lo contienen.
    let mut app = skills_app();
    type_text(&mut app, "/re");
    assert_eq!(names(&app), ["ponytail:review"]);
    // Un espacio significa que ya se eligió: se cierra.
    type_text(&mut app, "view x");
    assert!(names(&app).is_empty());
}

#[test]
fn right_arrow_completes_the_selected_skill_and_up_down_choose() {
    let mut app = skills_app();
    type_text(&mut app, "/");
    press(&mut app, KeyCode::Right);
    assert_eq!(app.chat_input, "/deslop ");
    let mut app = skills_app();
    type_text(&mut app, "/");
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down); // tope
    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Right);
    assert_eq!(app.chat_input, "/flintstone ");
}

#[test]
fn esc_closes_the_catalog_without_leaving_the_chat_and_typing_reopens_it() {
    let mut app = skills_app();
    type_text(&mut app, "/f");
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen, Screen::Chat);
    assert!(names(&app).is_empty());
    type_text(&mut app, "l");
    assert_eq!(names(&app), ["flintstone"]);
}

#[test]
fn codex_uses_dollar_and_the_other_prefix_gets_a_hint() {
    let mut app = skills_app();
    app.chat_model = Some("codex/gpt-5".into());
    type_text(&mut app, "$");
    assert_eq!(names(&app), ["deslop"]);
    assert!(draw(&app).contains("$deslop"));
    let mut app = skills_app();
    app.chat_model = Some("codex/gpt-5".into());
    type_text(&mut app, "/");
    assert!(names(&app).is_empty());
    assert!(draw(&app).contains("las skills empiezan con $"));
}
