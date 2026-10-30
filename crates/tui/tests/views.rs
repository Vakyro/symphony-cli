//! Snapshots de cada vista (FLOW §18) y ramas de FLOW §4, §6, §8, §16, sin daemon:
//! el estado se arma con las mismas respuestas IPC que manda el daemon.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use serde_json::{Value, json};
use symphony_tui::app::{App, Call, Exec, Failure, Msg, Notice, Req, Screen, Tab, diff_stats};
use symphony_tui::theme::Theme;
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
    terminal(app, 96, 28).backend().to_string()
}

fn terminal(app: &App, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut t = Terminal::new(TestBackend::new(width, height)).unwrap();
    t.draw(|f| ui::render(app, f)).unwrap();
    t
}

/// El buffer crudo, con estilos. `to_string()` solo da el texto: el color no se ve ahí.
fn buffer(app: &App) -> Buffer {
    terminal(app, 96, 28).backend().buffer().clone()
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
    ok(&mut app, Req::UiConfig, json!({ "animations": true }));
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
    assert_eq!(
        methods(&app.start()),
        ["project.status", "providers.list", "config.ui"]
    );
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
fn launch_art_fits_and_is_hidden_in_a_small_terminal() {
    let app = App::new("/work/arete-mobile");
    let out = draw(&app);
    assert!(out.contains(ui::LAUNCH_ART[0]));
    for l in ui::LAUNCH_ART {
        assert!(l.chars().count() <= 64);
    }
    let small = terminal(&app, 60, 16).backend().to_string();
    assert!(!small.contains(ui::LAUNCH_ART[0]));
    assert!(small.contains("SYMPHONY"));
}

#[test]
fn launch_new_project_lists_what_to_do_first() {
    let mut app = App::new("/work/arete-mobile");
    app.start();
    ok(&mut app, Req::Providers, providers());
    ok(&mut app, Req::ProjectStatus, status(false, 0));
    assert_eq!(app.screen, Screen::Launch);
    let out = draw(&app);
    assert!(out.contains("PARA EMPEZAR"));
    assert!(out.contains("configurar el proyecto"));
    assert!(out.contains("abrir sin configurar"));
    let home = home_app();
    let home_out = draw(&home);
    assert!(!home_out.contains("PARA EMPEZAR"));
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

// --- P09.S9: contexto, skills y MCP compartidos (FLOW 20, 21, 32, 33, 35) ------

fn command(app: &mut App, text: &str) -> Vec<Call> {
    press(app, KeyCode::Char(':'));
    typed(app, text);
    press(app, KeyCode::Enter)
}

fn context_inspect() -> Value {
    json!({
        "agent_id": "01AGENT1",
        "handoff": {
            "id": "01HANDOFF", "mode": "BALANCED", "tokens_raw_estimate": 21512, "tokens_sent": 1412,
            "build_ms": 14, "outcome": "COMPLETED", "created_at": NOW - 60_000
        },
        "items": [
            { "section": "GOAL", "path": null, "fidelity": 5, "tokens": 120 },
            { "section": "FILES", "path": "src/auth.ts", "fidelity": 1, "tokens": 310 },
            { "section": "DIFF", "path": "ctx://diff/01AGENT1/ab12cd", "fidelity": 0, "tokens": 40 },
            { "section": "FILES", "path": "src/session.ts", "fidelity": 3, "tokens": 540 }
        ]
    })
}

#[test]
fn view_20_context_overview() {
    let mut app = agent_app(Tab::Overview);
    let calls = press(&mut app, KeyCode::Char('c'));
    assert_eq!(app.screen, Screen::ContextOverview);
    assert_eq!(methods(&calls), ["context.inspect"]);
    assert_eq!(calls[0].params["agent_id"], "01AGENT1");
    ok(&mut app, Req::ContextInspect, context_inspect());
    insta::assert_snapshot!(draw(&app));
}

#[test]
fn view_20_without_handoffs_says_so() {
    let mut app = agent_app(Tab::Overview);
    press(&mut app, KeyCode::Char('c'));
    ok(
        &mut app,
        Req::ContextInspect,
        json!({ "agent_id": "01AGENT1", "handoff": null, "items": [] }),
    );
    assert!(draw(&app).contains("todavía no tiene handoffs"));
}

#[test]
fn context_without_an_agent_or_chat_is_an_error() {
    let mut app = home_app();
    let calls = command(&mut app, "context");
    assert!(calls.is_empty());
    assert!(matches!(app.notice, Some(Notice::Error(_))));
}

#[test]
fn view_21_context_object_opens_from_an_item() {
    let mut app = agent_app(Tab::Overview);
    press(&mut app, KeyCode::Char('c'));
    ok(&mut app, Req::ContextInspect, context_inspect());
    // Una parte sin objeto (el objetivo) no abre nada.
    let calls = press(&mut app, KeyCode::Enter);
    assert!(calls.is_empty());
    assert_eq!(app.screen, Screen::ContextOverview);
    // Un archivo se abre como ctx://file/<ruta>.
    press(&mut app, KeyCode::Down);
    let calls = press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::ContextObject);
    assert_eq!(methods(&calls), ["context.raw"]);
    assert_eq!(calls[0].params["uri"], "ctx://file/src/auth.ts");
    ok(
        &mut app,
        Req::ContextRaw,
        json!({
            "uri": "ctx://file/src/auth.ts", "kind": "FILE", "bytes": 96,
            "text": "export function login(user: string) {\n  return sign(user);\n}\n\nexport function logout() {}\n",
            "compressor": null, "tokens_original": 24, "tokens_compressed": null
        }),
    );
    insta::assert_snapshot!(draw(&app));
    // Esc vuelve al resumen; una dirección ctx:// se abre tal cual.
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen, Screen::ContextOverview);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    let calls = press(&mut app, KeyCode::Enter);
    assert_eq!(calls[0].params["uri"], "ctx://diff/01AGENT1/ab12cd");
}

fn shared_skills() -> Value {
    json!({ "skills": [
        { "name": "revisar", "scope": "PROJECT", "path": "/work/.symphony/skills/revisar", "enabled": true,
          "description": "Revisa un diff y lista los riesgos", "providers": [], "shadowed": false },
        { "name": "revisar", "scope": "USER", "path": "/home/.symphony/skills/revisar", "enabled": true,
          "description": "Revisión general", "providers": [], "shadowed": true },
        { "name": "release-notes", "scope": "USER", "path": "/home/.symphony/skills/release-notes", "enabled": false,
          "description": "Redacta las notas de versión", "providers": ["anthropic"], "shadowed": false }
    ]})
}

#[test]
fn view_32_shared_skills_toggle() {
    let mut app = home_app();
    let calls = command(&mut app, "skills");
    assert_eq!(app.screen, Screen::SharedSkills);
    assert_eq!(methods(&calls), ["skills.shared"]);
    ok(&mut app, Req::SharedSkills, shared_skills());
    insta::assert_snapshot!(draw(&app));
    // Espacio apaga la elegida y vuelve a leer.
    let calls = press(&mut app, KeyCode::Char(' '));
    assert_eq!(methods(&calls), ["skills.set_enabled"]);
    assert_eq!(calls[0].params["name"], "revisar");
    assert_eq!(calls[0].params["enabled"], false);
    let calls = ok(&mut app, Req::SharedToggle, json!({ "updated": 1 }));
    assert_eq!(methods(&calls), ["skills.shared"]);
    // Una apagada se enciende.
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    let calls = press(&mut app, KeyCode::Enter);
    assert_eq!(calls[0].params["name"], "release-notes");
    assert_eq!(calls[0].params["enabled"], true);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen, Screen::Home);
}

#[test]
fn view_33_shared_mcp_with_warnings() {
    let mut app = home_app();
    let calls = command(&mut app, "mcp");
    assert_eq!(app.screen, Screen::SharedMcp);
    assert_eq!(methods(&calls), ["mcp.list"]);
    ok(
        &mut app,
        Req::SharedMcp,
        json!({
            "servers": [
                { "name": "fs", "scope": "PROJECT", "transport": "STDIO", "share_mode": "PER_CLIENT",
                  "command": "npx", "url": null, "enabled": true, "note": null },
                { "name": "docs", "scope": "USER", "transport": "HTTP", "share_mode": "PER_CLIENT",
                  "command": null, "url": "https://example.test/mcp", "enabled": false, "note": null },
                { "name": "indexador", "scope": "USER", "transport": "STDIO", "share_mode": "SHARED",
                  "command": "indexer", "url": null, "enabled": true,
                  "note": "instancia compartida sin soporte: no se registra" }
            ],
            "warnings": ["mcp.toml: `roto`: falta `url`"]
        }),
    );
    insta::assert_snapshot!(draw(&app));
    let calls = press(&mut app, KeyCode::Enter);
    assert_eq!(methods(&calls), ["mcp.set_enabled"]);
    assert_eq!(calls[0].params["name"], "fs");
    assert_eq!(calls[0].params["enabled"], false);
}

#[test]
fn view_35_usage_and_context_stats() {
    let mut app = home_app();
    let calls = command(&mut app, "stats");
    assert_eq!(app.screen, Screen::Stats);
    assert_eq!(methods(&calls), ["context.stats", "usage.get"]);
    ok(
        &mut app,
        Req::ContextStats,
        json!({
            "objects": 142, "chunks": 903,
            "compression": [
                { "compressor": "DIFF", "objects": 12, "tokens_original": 80_000, "tokens_compressed": 1_900 },
                { "compressor": "LOG", "objects": 5, "tokens_original": 14_200, "tokens_compressed": 1_100 }
            ],
            "handoffs": [
                { "mode": "BALANCED", "handoffs": 6, "tokens_raw": 126_000, "tokens_sent": 8_400,
                  "needed_retrieval": 1, "failed_to_continue": 0 },
                { "mode": "RAW", "handoffs": 1, "tokens_raw": 21_500, "tokens_sent": 21_500,
                  "needed_retrieval": 0, "failed_to_continue": 0 }
            ],
            "retrievals": { "total": 9, "misses": 2, "tokens_returned": 5_300 }
        }),
    );
    ok(
        &mut app,
        Req::Usage,
        json!({ "days": 7, "usage": [
            { "provider_id": "anthropic", "model_id": "claude/sonnet", "source": "REPORTED",
              "runs": 14, "tokens_in": 9_661_214, "tokens_out": 0 },
            { "provider_id": "github", "model_id": "github/auto", "source": "ESTIMATED",
              "runs": 2, "tokens_in": 48_000, "tokens_out": 0 }
        ]}),
    );
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
    app.update(Msg::Event("agent.event".into(), serde_json::Value::Null));
    let calls = app.update(Msg::Tick(NOW));
    assert_eq!(
        methods(&calls),
        [
            "agent.list",
            "providers.list",
            "recovery.list",
            "system.get"
        ]
    );
    // Con respuestas pendientes no se piden más.
    app.update(Msg::Event("agent.event".into(), serde_json::Value::Null));
    assert!(app.update(Msg::Tick(NOW)).is_empty());
    ok(&mut app, Req::Agents, agents());
    ok(&mut app, Req::Providers, providers());
    ok(&mut app, Req::Recovery, json!({ "items": [] }));
    ok(&mut app, Req::System, json!({}));
    assert_eq!(app.update(Msg::Tick(NOW)).len(), 4);
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

/// P17.S4: chat con el agente trabajando (la mascota se ve) y las animaciones prendidas.
fn chat_running_app() -> App {
    let mut app = chat_app();
    ok(&mut app, Req::UiConfig, json!({ "animations": true }));
    ok(&mut app, Req::ChatGet, json!({ "agent_id": "01CHAT" }));
    let chat = app.chat.as_mut().unwrap();
    chat.inspect = json!({ "agent": { "state": "RUNNING" } });
    chat.messages = serde_json::from_value(json!([
        { "role": "USER", "content": "corre los tests", "at": NOW - 6_000 },
    ]))
    .unwrap();
    // Los snapshots no deben depender del entorno de quien corre los tests.
    app.ui_animations = true;
    app
}

/// La mascota (un mini piano de una octava con frases) cabe entera a 80 y a 160 columnas.
#[test]
fn view_00_chat_mascot_80_columns() {
    insta::assert_snapshot!(terminal(&chat_running_app(), 80, 28).backend().to_string());
}

#[test]
fn view_00_chat_mascot_160_columns() {
    insta::assert_snapshot!(terminal(&chat_running_app(), 160, 28).backend().to_string());
}

/// P17.S4: con `SYMPHONY_NO_ANIM` / `[ui] animations = false` / `NO_COLOR` no hay piano.
#[test]
fn chat_mascot_respects_the_kill_switches() {
    use symphony_tui::mascot;

    // Config apagada (`config.ui` → animations=false): nunca dibuja, sea cual sea el entorno.
    let mut app = chat_running_app();
    ok(&mut app, Req::UiConfig, json!({ "animations": false }));
    assert!(!app.ui_animations);
    let screen = draw(&app);
    assert!(!screen.contains('█'), "{screen}");
    assert!(!screen.contains('♪'), "{screen}");

    // Config prendida: manda el entorno (es lo que leen SYMPHONY_NO_ANIM / NO_COLOR / TERM).
    let env = mascot::env_ok();
    ok(&mut app, Req::UiConfig, json!({ "animations": true }));
    assert_eq!(app.ui_animations, env);

    // Animaciones apagadas a mano (resultado de SYMPHONY_NO_ANIM): mismo chat, sin mascota.
    app.ui_animations = false;
    let screen = draw(&app);
    assert!(!screen.contains('█'), "{screen}");
    assert!(!screen.contains('♪'), "{screen}");

    // Los tres interruptores de entorno apagan por separado.
    assert!(mascot::env_allows(false, false, false));
    assert!(!mascot::env_allows(true, false, false));
    assert!(!mascot::env_allows(false, true, false));
    assert!(!mascot::env_allows(false, false, true));
    // La decisión pura: hacen falta config y entorno a la vez.
    assert!(mascot::allowed(true, true));
    assert!(!mascot::allowed(false, true));
    assert!(!mascot::allowed(true, false));
}

/// La mascota solo vive en el chat principal: la vista de agente no la dibuja.
#[test]
fn mascot_only_shows_in_the_main_chat() {
    let mut app = chat_running_app();
    // Mismo agente «RUNNING», pero en la vista de agente (Esc → Home → Enter).
    press(&mut app, KeyCode::Esc);
    ok(&mut app, Req::Agents, agents());
    let calls = press(&mut app, KeyCode::Enter);
    assert_eq!(methods(&calls), ["agent.inspect"]);
    ok(&mut app, Req::Inspect, inspect());
    let screen = draw(&app);
    assert!(!screen.contains('█'), "{screen}");
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

/// P11.5.S3: el texto que el agente va escribiendo se ve en el chat (con cursor) y reemplaza
/// al «pensando…»; al llegar el mensaje completo desaparece.
#[test]
fn chat_shows_the_partial_text_while_the_agent_writes() {
    let mut app = chat_app();
    ok(&mut app, Req::ChatGet, json!({ "agent_id": "01CHAT" }));
    app.chat.as_mut().unwrap().inspect = json!({ "agent": { "state": "RUNNING" } });
    ok(
        &mut app,
        Req::Logs,
        json!({
            "messages": [{ "role": "USER", "content": "explícame el bus", "at": NOW - 3_000 }],
            "partial": "El bus persiste primero y desp",
        }),
    );
    let screen = draw(&app);
    assert!(
        screen.contains("El bus persiste primero y desp▍"),
        "{screen}"
    );
    assert!(!screen.contains("pensando"), "{screen}");

    // Mensaje completo: el parcial llega vacío y queda solo el mensaje.
    ok(
        &mut app,
        Req::Logs,
        json!({
            "messages": [
                { "role": "USER", "content": "explícame el bus", "at": NOW - 3_000 },
                { "role": "ASSISTANT", "content": "El bus persiste primero y después difunde.", "at": NOW - 1_000 },
            ],
            "partial": "",
        }),
    );
    let screen = draw(&app);
    assert!(screen.contains("y después difunde."), "{screen}");
    assert!(!screen.contains('▍'), "{screen}");
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

// --- P08.S8: vistas de Fase 2 (15–19 y 26–29) ----------------------------------------

fn tasks_json() -> Value {
    json!({ "tasks": [
        { "task_id": "01TASK1", "code": "T-1", "title": "Supabase authentication", "kind": "WORK",
          "status": "DONE", "status_reason": null, "priority": 0, "milestone_id": "01MS1",
          "milestone": "Authentication complete", "agent_id": "01AGENT1", "agent_number": 1,
          "agent_state": "COMPLETED", "merge_status": "MERGED", "waiting_for": [] },
        { "task_id": "01TASK2", "code": "T-2", "title": "Responsive modals", "kind": "WORK",
          "status": "RUNNING", "status_reason": null, "priority": 0, "milestone_id": "01MS1",
          "milestone": "Authentication complete", "agent_id": "01AGENT2", "agent_number": 2,
          "agent_state": "RUNNING", "merge_status": null, "waiting_for": [] },
        { "task_id": "01TASK3", "code": "T-3", "title": "Integration tests", "kind": "WORK",
          "status": "WAITING", "status_reason": "espera a T-2", "priority": 0, "milestone_id": null,
          "milestone": null, "agent_id": "01AGENT3", "agent_number": 3,
          "agent_state": "WAITING_DEPENDENCY", "merge_status": null, "waiting_for": ["T-2"] },
        { "task_id": "01TASK4", "code": "T-4", "title": "Merge auth changes", "kind": "WORK",
          "status": "BLOCKED", "status_reason": "conflicto de merge", "priority": 0, "milestone_id": null,
          "milestone": null, "agent_id": "01AGENT4", "agent_number": 4,
          "agent_state": "COMPLETED", "merge_status": "BLOCKED", "waiting_for": [] },
    ]})
}

#[test]
fn view_15_task_list_and_view_16_task_detail() {
    let mut app = home_app();
    let calls = press(&mut app, KeyCode::Char('t'));
    assert_eq!(app.screen, Screen::Tasks);
    assert_eq!(methods(&calls), ["task.list"]);
    ok(&mut app, Req::Tasks, tasks_json());
    insta::assert_snapshot!("view_15_task_list", draw(&app));

    // Enter abre el detalle y pide sus dependencias.
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    let calls = press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::TaskDetail);
    assert_eq!(methods(&calls), ["task.list", "task.deps"]);
    assert_eq!(calls[1].params, json!({ "task": "01TASK3" }));
    ok(
        &mut app,
        Req::TaskDeps,
        json!({ "task_id": "01TASK3", "dependencies": [
            { "task_id": "01TASK2", "code": "T-2", "title": "Responsive modals", "status": "RUNNING" },
            { "task_id": "01TASK1", "code": "T-1", "title": "Supabase authentication", "status": "DONE" },
        ], "unmet": [] }),
    );
    insta::assert_snapshot!("view_16_task_detail", draw(&app));

    // `a` abre el agente de la tarea; Esc vuelve a la lista.
    let calls = press(&mut app, KeyCode::Char('a'));
    assert_eq!(app.screen, Screen::Agent);
    assert!(methods(&calls).contains(&"agent.inspect"));
}

#[test]
fn view_17_milestone_review_decides_only_what_the_user_asks() {
    let mut app = home_app();
    let calls = press(&mut app, KeyCode::Char('m'));
    assert_eq!(app.screen, Screen::Milestones);
    assert_eq!(methods(&calls), ["milestone.list"]);
    ok(
        &mut app,
        Req::Milestones,
        json!({ "milestones": [
            { "id": "01MS1", "name": "Authentication complete", "requires_approval": true,
              "status": "IN_REVIEW", "decided_at": null, "tasks_total": 2, "tasks_done": 2 },
            { "id": "01MS2", "name": "Responsive UI", "requires_approval": false,
              "status": "OPEN", "decided_at": null, "tasks_total": 3, "tasks_done": 1 },
        ]}),
    );
    insta::assert_snapshot!("view_17_milestone_review", draw(&app));

    let calls = press(&mut app, KeyCode::Char('a'));
    assert_eq!(methods(&calls), ["milestone.decide"]);
    assert_eq!(
        calls[0].params,
        json!({ "milestone": "01MS1", "decision": "approve" })
    );
    let calls = press(&mut app, KeyCode::Char('c'));
    assert_eq!(calls[0].params["decision"], "request_changes");
    let calls = press(&mut app, KeyCode::Char('h'));
    assert_eq!(calls[0].params["decision"], "hold");
}

fn system_json() -> Value {
    json!({
        "profile": "BALANCED", "cpu_pct": 61.0, "ram_pct": 64.0, "process_count": 7,
        "sampled_at": NOW - 2_000, "heavy_slots_used": 1, "heavy_slots_total": 1,
        "cpu_ceiling_pct": 70.0, "ram_ceiling_pct": 70.0,
        "running": [ { "agent": "Agent #2", "command": "pnpm test --filter api", "class": 3 } ],
        "waiting": [ { "agent": "Agent #4", "command": "pnpm build", "class": 4,
                       "why": "en cola: el slot pesado está ocupado (Agent #2 corre `pnpm test --filter api`)" } ],
    })
}

#[test]
fn view_18_system_and_view_19_performance_profile() {
    let mut app = home_app();
    let calls = press(&mut app, KeyCode::Char('s'));
    assert_eq!(app.screen, Screen::System);
    assert_eq!(methods(&calls), ["system.get"]);
    ok(&mut app, Req::System, system_json());
    insta::assert_snapshot!("view_18_system", draw(&app));

    // `p` abre el selector con el perfil vigente resaltado; Enter lo aplica.
    press(&mut app, KeyCode::Char('p'));
    assert_eq!(app.screen, Screen::PerformanceProfile);
    ok(&mut app, Req::System, system_json());
    press(&mut app, KeyCode::Right);
    insta::assert_snapshot!("view_19_performance_profile", draw(&app));
    let calls = press(&mut app, KeyCode::Enter);
    assert_eq!(methods(&calls), ["performance.set"]);
    assert_eq!(calls[0].params, json!({ "profile": "PERFORMANCE" }));
}

#[test]
fn home_shows_the_real_system_strip() {
    let mut app = home_app();
    ok(&mut app, Req::System, system_json());
    let screen = draw(&app);
    assert!(
        screen.contains("CPU 61%") && screen.contains("slots pesados 1/1"),
        "{screen}"
    );
    assert!(
        screen.contains("1 operación(es) esperando turno"),
        "{screen}"
    );
}

fn open_flow(key: char) -> App {
    let mut app = agent_app(Tab::Overview);
    let calls = press(&mut app, KeyCode::Char(key));
    assert!(
        matches!(app.screen, Screen::Validation | Screen::MergePreflight),
        "{:?}",
        app.screen
    );
    assert!(!calls.is_empty());
    app
}

#[test]
fn view_28_validation_shows_each_tier_and_the_failure() {
    let mut app = open_flow('v');
    ok(
        &mut app,
        Req::ValidationGet,
        json!({ "agent_id": "01AGENT1", "runs": [
            { "id": "v2", "tier": 2, "trigger": "AUTO", "status": "FAIL", "started_at": NOW - 5_000,
              "finished_at": NOW - 2_000, "checks": [
                { "name": "typecheck", "status": "PASS", "passed": null, "failed": null, "summary": null },
                { "name": "targeted_tests", "status": "FAIL", "passed": 3, "failed": 1,
                  "summary": "FAIL auth.refresh.test.ts expected 200, received 401" } ] },
            { "id": "v1", "tier": 1, "trigger": "AUTO", "status": "PASS", "started_at": NOW - 9_000,
              "finished_at": NOW - 8_000, "checks": [
                { "name": "format", "status": "PASS", "passed": null, "failed": null, "summary": null },
                { "name": "lint", "status": "PASS", "passed": null, "failed": null, "summary": null } ] },
        ]}),
    );
    insta::assert_snapshot!("view_28_validation", draw(&app));
    let calls = press(&mut app, KeyCode::Char('r'));
    assert_eq!(methods(&calls), ["validation.run"]);
    assert_eq!(calls[0].params, json!({ "agent": "01AGENT1", "tier": 2 }));
}

fn merge_json(conflicts: bool) -> Value {
    json!({
        "agent_id": "01AGENT1",
        "merge_request": {
            "id": "01MR1", "status": if conflicts { "BLOCKED" } else { "READY" },
            "target_branch": "symphony/chat", "requires_approval": true, "approved_at": null,
            "merge_commit": null, "merged_at": null,
            "preflight": { "checks": [
                { "name": "target", "status": "PASS", "detail": "se integra a `symphony/chat`" },
                { "name": "task", "status": "PASS", "detail": "T-1 completa" },
                { "name": "dependencies", "status": "PASS", "detail": "todas cumplidas" },
                { "name": "validation", "status": "PASS", "detail": "2 tier(s) pasaron" },
                { "name": "conflicts", "status": if conflicts { "FAIL" } else { "PASS" },
                  "detail": if conflicts { "choca en src/auth.ts" } else { "ninguno" } },
            ] },
        },
        "conflicts": if conflicts { json!([
            { "path": "src/auth.ts", "other_agent_id": "01AGENT2", "resolution": "PENDING" }
        ]) } else { json!([]) },
        "reviews": [
            { "id": "01RV1", "reviewer_agent_id": "01AGENT9", "status": "DONE",
              "recommendation": "REQUEST_CHANGES",
              "findings": [ { "severity": "high", "file": "src/auth.ts", "text": "falta el caso de error 401" },
                            { "severity": "low", "file": "src/auth.ts", "text": "nombre poco claro" } ] }
        ],
    })
}

#[test]
fn view_26_merge_preflight_and_its_actions() {
    let mut app = open_flow('g');
    ok(&mut app, Req::MergeGet, merge_json(false));
    insta::assert_snapshot!("view_26_merge_preflight", draw(&app));

    let calls = press(&mut app, KeyCode::Char('a'));
    assert_eq!(methods(&calls), ["merge.approve"]);
    assert_eq!(calls[0].params, json!({ "merge_request": "01MR1" }));
    let calls = press(&mut app, KeyCode::Char('i'));
    assert_eq!(methods(&calls), ["merge.integrate"]);
    let calls = press(&mut app, KeyCode::Char('p'));
    assert_eq!(
        calls[0].params,
        json!({ "agent": "01AGENT1", "validate": true })
    );
    let calls = press(&mut app, KeyCode::Char('w'));
    assert_eq!(methods(&calls), ["review.start"]);
    // Sin conflictos, `c` no abre nada.
    press(&mut app, KeyCode::Char('c'));
    assert_eq!(app.screen, Screen::MergePreflight);
}

#[test]
fn view_27_conflict_review_and_view_29_review_result() {
    let mut app = open_flow('g');
    ok(&mut app, Req::MergeGet, merge_json(true));
    press(&mut app, KeyCode::Char('c'));
    assert_eq!(app.screen, Screen::ConflictReview);
    insta::assert_snapshot!("view_27_conflict_review", draw(&app));
    let calls = press(&mut app, KeyCode::Char('m'));
    assert_eq!(methods(&calls), ["merge.resolve"]);
    assert_eq!(
        calls[0].params,
        json!({ "merge_request": "01MR1", "resolution": "manual" })
    );
    let calls = press(&mut app, KeyCode::Char('x'));
    assert_eq!(methods(&calls), ["merge.abort"]);

    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen, Screen::MergePreflight);
    ok(&mut app, Req::MergeGet, merge_json(true));
    press(&mut app, KeyCode::Char('y'));
    assert_eq!(app.screen, Screen::ReviewResult);
    insta::assert_snapshot!("view_29_review_result", draw(&app));
}

#[test]
fn the_new_views_open_from_the_command_bar() {
    for (cmd, screen, method) in [
        ("tasks", Screen::Tasks, "task.list"),
        ("milestones", Screen::Milestones, "milestone.list"),
        ("system", Screen::System, "system.get"),
        ("profile", Screen::PerformanceProfile, "system.get"),
    ] {
        let mut app = home_app();
        let calls = command(&mut app, cmd);
        assert_eq!(app.screen, screen, "{cmd}");
        assert_eq!(methods(&calls), [method], "{cmd}");
    }
}

/// P11.5.S6: integrar el chat en `main` desde la vista Chat pide `Ctrl+G` dos veces seguidas y
/// solo si el preflight quedó listo con la revisión aprobada; otra tecla lo desarma.
#[test]
fn chat_merge_needs_two_ctrl_g_after_an_approving_preflight() {
    let mut app = chat_app();
    let prepared = |status: &str| {
        json!({
            "ahead": 2, "ready": true,
            "checks": [{ "name": "review", "status": status, "detail": "el revisor recomienda integrar" }],
        })
    };

    // Primer Ctrl+G: solo el preflight, nunca el merge.
    let calls = ctrl(&mut app, 'g');
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "chat.merge.prepare");
    // Sin revisión aprobada, no queda armado: otro Ctrl+G vuelve a ser preflight.
    ok(&mut app, Req::ChatMerge, prepared("WARN"));
    assert!(!app.chat_merge_armed);
    assert_eq!(ctrl(&mut app, 'g')[0].method, "chat.merge.prepare");

    // Revisión aprobada: queda armado y el siguiente Ctrl+G integra con confirmación.
    ok(&mut app, Req::ChatMerge, prepared("PASS"));
    assert!(app.chat_merge_armed);
    let calls = ctrl(&mut app, 'g');
    assert_eq!(calls[0].method, "chat.merge.integrate");
    assert_eq!(calls[0].params["confirm"], true);

    // Una tecla en medio lo desarma.
    ok(&mut app, Req::ChatMerge, prepared("PASS"));
    app.update(Msg::Key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::NONE,
    )));
    assert_eq!(ctrl(&mut app, 'g')[0].method, "chat.merge.prepare");

    // Un FAIL se muestra como error y no arma nada.
    ok(
        &mut app,
        Req::ChatMerge,
        json!({ "ahead": 1, "checks": [{ "name": "conflicts", "status": "FAIL", "detail": "choca en a.rs" }] }),
    );
    assert!(!app.chat_merge_armed);
    assert!(matches!(&app.notice, Some(Notice::Error(m)) if m.contains("choca en a.rs")));

    // Ctrl+R pide la revisión.
    assert_eq!(ctrl(&mut app, 'r')[0].method, "chat.merge.review");
}

/// P11.5.S7: la vista 17 crea un milestone desde la TUI (nombre + con/sin aprobación).
#[test]
fn view_17_creates_a_milestone_from_the_tui() {
    let mut app = home_app();
    press(&mut app, KeyCode::Char('m'));
    ok(&mut app, Req::Milestones, json!({ "milestones": [] }));

    press(&mut app, KeyCode::Char('n'));
    assert_eq!(app.milestone_input, Some((String::new(), true)));
    // Sin nombre no hay llamada.
    assert!(press(&mut app, KeyCode::Enter).is_empty());
    assert!(matches!(&app.notice, Some(Notice::Error(_))));
    for c in "Auth listo".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Tab);
    assert!(draw(&app).contains("Auth listo▏"));
    assert!(draw(&app).contains("sin aprobación"));
    let calls = press(&mut app, KeyCode::Enter);
    assert_eq!(methods(&calls), ["milestone.create"]);
    assert_eq!(calls[0].params["name"], "Auth listo");
    assert_eq!(calls[0].params["requires_approval"], false);
    assert!(app.milestone_input.is_none());

    // Esc cancela sin llamar.
    press(&mut app, KeyCode::Char('n'));
    press(&mut app, KeyCode::Char('x'));
    assert!(press(&mut app, KeyCode::Esc).is_empty());
    assert!(app.milestone_input.is_none());
    assert_eq!(app.screen, Screen::Milestones);
}

/// P11.5.S7: «Enviar al agente» de la vista 28 manda el fallo de validación a la sesión del
/// agente, con lo que falló y su resumen; sin fallos no manda nada.
#[test]
fn view_28_sends_the_validation_failure_to_the_agent() {
    let mut app = open_flow('v');
    ok(
        &mut app,
        Req::ValidationGet,
        json!({ "agent_id": "01AGENT1", "runs": [
            { "id": "v2", "tier": 2, "trigger": "AUTO", "status": "FAIL", "checks": [
                { "name": "typecheck", "status": "PASS", "summary": null },
                { "name": "targeted_tests", "status": "FAIL",
                  "summary": "FAIL auth.refresh.test.ts expected 200, received 401" } ] },
        ]}),
    );
    let calls = press(&mut app, KeyCode::Char('e'));
    assert_eq!(methods(&calls), ["agent.send"]);
    assert_eq!(calls[0].params["agent"], "01AGENT1");
    let text = calls[0].params["text"].as_str().unwrap();
    assert!(text.contains("tier 2 · targeted_tests: FAIL"), "{text}");
    assert!(text.contains("expected 200, received 401"), "{text}");
    assert!(!text.contains("typecheck"), "solo lo que falló: {text}");

    // Todo en verde: no hay nada que mandar.
    ok(
        &mut app,
        Req::ValidationGet,
        json!({ "agent_id": "01AGENT1", "runs": [
            { "id": "v3", "tier": 2, "trigger": "AUTO", "status": "PASS", "checks": [
                { "name": "typecheck", "status": "PASS", "summary": null } ] },
        ]}),
    );
    assert!(press(&mut app, KeyCode::Char('e')).is_empty());
    assert!(matches!(&app.notice, Some(Notice::Info(m)) if m.contains("No hay fallos")));
}

/// P11.5.S7: un aviso del pie caduca solo (no se queda hasta la siguiente tecla).
#[test]
fn a_footer_notice_expires_on_its_own() {
    let mut app = chat_app();
    app.error("algo falló");
    assert!(app.notice.is_some());
    app.update(Msg::Tick(NOW + 1_000));
    assert!(app.notice.is_some(), "todavía reciente");
    app.update(Msg::Tick(NOW + 20_000));
    assert!(app.notice.is_none(), "caducó");
}

/// P11.5.S6 (revisión): la confirmación de «Ctrl+G otra vez» no sobrevive a salir del chat.
#[test]
fn chat_merge_confirmation_does_not_survive_leaving_the_chat() {
    let mut app = chat_app();
    ok(
        &mut app,
        Req::ChatMerge,
        json!({ "ahead": 1, "checks": [{ "name": "review", "status": "PASS", "detail": "ok" }] }),
    );
    assert!(app.chat_merge_armed);
    // A otra pantalla y de vuelta.
    press(&mut app, KeyCode::Esc);
    assert_ne!(app.screen, Screen::Chat);
    assert!(!app.chat_merge_armed || press(&mut app, KeyCode::Char('x')).is_empty());
    assert!(!app.chat_merge_armed, "salir del chat lo desarma");
}

/// P11.5.S6 (revisión): la URI de un archivo que arma la TUI es la misma que el parser del
/// daemon acepta y devuelve igual.
#[test]
fn the_tui_builds_file_uris_the_context_parser_accepts() {
    use symphony_context::uri::CtxUri;
    for p in [
        "src/a.rs",
        "packages/@scope/x.ts",
        "a?b#c%d.txt",
        "dir/sub dir/a b.txt",
    ] {
        let uri = symphony_tui::app::ctx_file_uri(p);
        let parsed = CtxUri::parse(&uri).unwrap_or_else(|e| panic!("{uri}: {e}"));
        assert_eq!(
            parsed,
            CtxUri::File {
                path: p.into(),
                rev: None
            }
        );
        assert_eq!(parsed.to_string(), uri);
    }
}

/// P14.S2 (FLOW §8.5): una sugerencia de modelo pendiente se ve en el chat y se resuelve con
/// tres teclas; sin sugerencia, esas teclas no hacen nada.
#[test]
fn chat_shows_the_model_suggestion_and_resolves_it_with_three_keys() {
    let mut app = chat_app();
    ok(&mut app, Req::ChatGet, json!({ "agent_id": "01CHAT" }));
    // Sin sugerencia: Ctrl+T/K/N no llaman a nada.
    for c in ['t', 'k', 'n'] {
        assert!(ctrl(&mut app, c).is_empty());
        // No se calla: dice que no hay nada que resolver.
        assert!(
            matches!(&app.notice, Some(Notice::Info(m)) if m.contains("No hay una sugerencia"))
        );
    }
    app.chat.as_mut().unwrap().inspect = json!({
        "agent": { "state": "READY" },
        "suggestion": {
            "id": "SUG1", "current_model_id": "google/flash",
            "suggested_model_id": "claude/sonnet",
            "reason": "la tarea pasó de documentación a un refactor de varios archivos",
        },
    });
    let screen = draw(&app);
    assert!(
        screen.contains("Sugerencia: google/flash → claude/sonnet"),
        "{screen}"
    );
    assert!(screen.contains("^T cambiar"), "{screen}");

    for (c, action) in [('t', "switch"), ('k', "keep"), ('n', "mute")] {
        let calls = ctrl(&mut app, c);
        assert_eq!(methods(&calls), ["suggestion.resolve"], "{c}");
        assert_eq!(calls[0].params, json!({ "id": "SUG1", "action": action }));
    }
    // La respuesta se avisa y no cambia nada por su cuenta.
    ok(
        &mut app,
        Req::SuggestionResolve,
        json!({ "status": "ACCEPTED" }),
    );
    assert!(matches!(&app.notice, Some(Notice::Info(m)) if m.contains("modelo sugerido")));
}

// --- tema (P17.S1) -----------------------------------------------------------

/// El color tiene que llegar al buffer: los snapshots de texto no lo probarían.
#[test]
fn the_brand_paints_the_header() {
    let mut app = home_app();
    app.theme = Theme::default();
    let buf = buffer(&app);
    let title = buf.cell((1, 0)).unwrap();
    assert_eq!(title.symbol(), "S", "la cabecera empieza por « SYMPHONY »");
    assert_eq!(title.fg, Color::Rgb(140, 120, 180), "morado de marca");
}

/// Ninguna celda puede salirse de la paleta: nada de cian ni colores chillones.
#[test]
fn no_cell_escapes_the_brand_palette() {
    let mut app = home_app();
    app.theme = Theme::default();
    let t = Theme::default();
    let palette: Vec<Color> = [t.brand, t.accent, t.ok, t.warn, t.error, t.muted, t.border]
        .into_iter()
        .flatten()
        .collect();
    let outside: Vec<(String, Color)> = buffer(&app)
        .content
        .iter()
        .filter(|c| c.fg != Color::Reset && !palette.contains(&c.fg))
        .map(|c| (c.symbol().to_string(), c.fg))
        .collect();
    assert!(
        outside.is_empty(),
        "colores fuera de la paleta: {outside:?}"
    );
}

/// `NO_COLOR`: la misma información, sin pintar nada.
#[test]
fn no_color_keeps_the_view_without_paint() {
    let mut app = home_app();
    app.theme = Theme::no_color();
    let buf = buffer(&app);
    assert!(
        buf.content.iter().all(|c| c.fg == Color::Reset),
        "con NO_COLOR no se pinta ninguna celda"
    );
    assert!(buf.content.iter().any(|c| c.symbol() == "S"));
}

/// La barra se ve entera a 80 columnas, y a 160 no se estira ni se rompe.
#[test]
fn view_01_home_80_columns() {
    insta::assert_snapshot!(terminal(&home_app(), 80, 28).backend().to_string());
}

#[test]
fn view_01_home_160_columns() {
    insta::assert_snapshot!(terminal(&home_app(), 160, 28).backend().to_string());
}

// --- barra de estado (P17.S1) -------------------------------------------------

/// La home con el catálogo cargado: modelo real y cuota tomada de `providers.list`.
fn status_app() -> App {
    let mut app = home_app();
    ok(&mut app, Req::Models, models());
    ok(&mut app, Req::Providers, providers());
    app
}

/// La barra es la penúltima fila: debajo solo queda la línea de pistas.
fn status_bar(app: &App, width: u16) -> String {
    let screen = terminal(app, width, 28).backend().to_string();
    let lines: Vec<&str> = screen.lines().collect();
    // `TestBackend::to_string()` devuelve cada fila entre comillas y rellena a lo ancho.
    lines[lines.len() - 2].trim_matches('"').trim().to_string()
}

#[test]
fn the_status_bar_shows_the_six_fields_when_it_fits() {
    let app = status_app();
    let bar = status_bar(&app, 160);
    assert!(bar.starts_with("modelo claude/opus"), "{bar:?}");
    // 0.15 de cuota restante y reinicio dentro de 2 h, del proveedor real del modelo: el
    // `provider_id` del catálogo (`anthropic`), no el prefijo del id (`claude`).
    assert!(bar.contains("cuota 15 % (reinicia en 2 h)"), "{bar}");
    assert!(bar.contains("arete-mobile"), "{bar}");
    assert!(bar.contains("rama main"), "{bar}");
    assert!(bar.contains("tokens —"), "{bar}");
    assert!(bar.contains("modo —"), "{bar}");
}

#[test]
fn the_status_bar_drops_the_last_fields_before_wrapping() {
    let app = status_app();
    let bar = status_bar(&app, 90);
    assert!(bar.starts_with("modelo claude/opus"), "{bar}");
    assert!(bar.contains("cuota 15 %"), "{bar}");
    assert!(bar.contains("rama main"), "{bar}");
    for gone in ["arete-mobile", "tokens", "modo"] {
        assert!(!bar.contains(gone), "a 90 columnas no cabe {gone}: {bar}");
    }
}

#[test]
fn the_status_bar_keeps_model_and_quota_in_a_narrow_terminal() {
    let app = status_app();
    let bar = status_bar(&app, 60);
    assert!(bar.starts_with("modelo claude/opus"), "{bar}");
    assert!(bar.contains("cuota 15 %"), "{bar}");
    assert!(!bar.contains("rama"), "{bar}");
    assert!(bar.chars().count() <= 60, "se pasa de ancho: {bar}");
}

/// Si ni el modelo cabe, se recorta con «…»: la línea nunca se parte.
#[test]
fn the_status_bar_truncates_the_model_instead_of_wrapping() {
    let app = status_app();
    let bar = status_bar(&app, 30);
    assert!(bar.starts_with("modelo "), "{bar}");
    assert!(bar.contains('…'), "el modelo se recorta: {bar}");
    assert!(!bar.contains("claude/opus"), "{bar}");
    assert!(bar.contains("cuota 15 %"), "la cuota nunca se cae: {bar}");
    assert!(bar.chars().count() <= 30, "se pasa de ancho: {bar}");
}

#[test]
fn the_status_bar_is_on_every_screen() {
    // Launch: la app recién creada, sin ninguna respuesta del daemon.
    let launch = App::new("/work/arete-mobile");
    assert_eq!(launch.screen, Screen::Launch);
    assert!(status_bar(&launch, 96).starts_with("modelo "));

    // Chat: el inicio de la app, cuando ya llegó el estado del proyecto.
    let mut chat = App::new("/work/arete-mobile");
    chat.start();
    ok(&mut chat, Req::ProjectStatus, status(true, 0));
    ok(&mut chat, Req::Providers, providers());
    assert_eq!(chat.screen, Screen::Chat);
    assert!(status_bar(&chat, 96).starts_with("modelo "));

    // Home y Stats, con el catálogo cargado.
    let home = status_app();
    assert_eq!(home.screen, Screen::Home);
    assert!(status_bar(&home, 96).starts_with("modelo claude/opus"));

    let mut stats = status_app();
    let calls = command(&mut stats, "stats");
    assert_eq!(methods(&calls), ["context.stats", "usage.get"]);
    assert_eq!(stats.screen, Screen::Stats);
    assert!(status_bar(&stats, 96).starts_with("modelo "));
}

/// Sin datos, la barra no inventa: enseña «—» y no se rompe en una terminal estrecha.
#[test]
fn the_status_bar_shows_a_dash_when_it_has_no_data() {
    let app = App::new("/work/arete-mobile");
    let wide = status_bar(&app, 160);
    assert!(wide.starts_with("modelo —"), "{wide}");
    assert!(wide.contains("cuota —"), "{wide}");
    // La carpeta sale del cwd, que existe aunque el proyecto no esté configurado.
    assert!(wide.contains("arete-mobile"), "{wide}");
    assert!(wide.contains("rama —"), "{wide}");
    assert!(wide.contains("tokens —"), "{wide}");
    assert!(wide.contains("modo —"), "{wide}");

    // En 40 columnas la carpeta ya no cabe, pero la línea no se parte ni se pasa de ancho.
    let narrow = status_bar(&app, 40);
    assert!(narrow.starts_with("modelo —"), "{narrow}");
    assert!(narrow.contains("cuota —"), "{narrow}");
    assert!(narrow.chars().count() <= 40, "{narrow}");
}

// --- P17.S2: resumen de ediciones y streaming --------------------------------

#[test]
fn diff_stats_counts_two_files_and_ignores_headers() {
    let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,3 @@\n context\n-old line\n+new line\n+added line\n\\ No newline at end of file\n\ndiff --git a/b.txt b/b.txt\n--- a/b.txt\n+++ b/b.txt\n@@ -1 +1 @@\n-removed line\n+changed line\n";
    let stats = diff_stats(diff);
    assert_eq!(stats.len(), 2, "{stats:?}");
    assert_eq!(stats[0].path, "a.txt");
    assert_eq!(stats[0].added, 2);
    assert_eq!(stats[0].removed, 1);
    assert_eq!(stats[1].path, "b.txt");
    assert_eq!(stats[1].added, 1);
    assert_eq!(stats[1].removed, 1);
}

/// Dentro de un hunk, una línea que empieza por `--- `/`+++ ` es contenido: quitar un comentario
/// `-- x` de un `.sql` se ve `--- x` en el diff y tiene que contar.
#[test]
fn diff_stats_counts_content_that_looks_like_a_file_header() {
    let diff = "diff --git a/x.sql b/x.sql\n--- a/x.sql\n+++ b/x.sql\n@@ -1,3 +1,3 @@\n--- comentario viejo\n+++ comentario nuevo\n SELECT 1;\n";
    let stats = diff_stats(diff);
    assert_eq!(stats.len(), 1, "{stats:?}");
    assert_eq!(stats[0].path, "x.sql");
    assert_eq!(stats[0].added, 1, "`+++ comentario nuevo` es contenido");
    assert_eq!(stats[0].removed, 1, "`--- comentario viejo` es contenido");
}

/// Trozo suelto sin `diff --git`: el nombre sale del `+++` o, en un borrado, del `---`.
#[test]
fn diff_stats_reads_a_fragment_without_a_git_header() {
    let fragment = "--- a/src/x.rs\n+++ b/src/x.rs\n@@ -1 +1,2 @@\n-old\n+new\n+extra\n";
    let stats = diff_stats(fragment);
    assert_eq!(stats.len(), 1, "{stats:?}");
    assert_eq!(stats[0].path, "src/x.rs");
    assert_eq!((stats[0].added, stats[0].removed), (2, 1));

    let deletion = "--- a/viejo.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-borrado\n";
    let stats = diff_stats(deletion);
    assert_eq!(stats.len(), 1, "{stats:?}");
    assert_eq!(stats[0].path, "viejo.txt");
    assert_eq!((stats[0].added, stats[0].removed), (0, 1));
}

/// Git entrecomilla y escapa las rutas raras; y el mismo archivo puede venir en dos bloques.
#[test]
fn diff_stats_unquotes_paths_and_sums_repeated_files() {
    let quoted =
        "diff --git \"a/caf\\303\\251.txt\" \"b/caf\\303\\251.txt\"\n@@ -1 +1 @@\n-uno\n+dos\n";
    let stats = diff_stats(quoted);
    assert_eq!(stats.len(), 1, "{stats:?}");
    assert_eq!(stats[0].path, "caf\u{e9}.txt");
    assert_eq!((stats[0].added, stats[0].removed), (1, 1));

    let repeated = "diff --git a/x.txt b/x.txt\n@@ -1 +1 @@\n-uno\n+dos\ndiff --git a/x.txt b/x.txt\n@@ -9 +9 @@\n-tres\n+cuatro\n+cinco\n";
    let stats = diff_stats(repeated);
    assert_eq!(stats.len(), 1, "{stats:?}");
    assert_eq!(stats[0].path, "x.txt");
    assert_eq!((stats[0].added, stats[0].removed), (3, 2));

    let binary =
        "diff --git a/logo.png b/logo.png\nBinary files a/logo.png and b/logo.png differ\n";
    let stats = diff_stats(binary);
    assert_eq!(stats.len(), 1, "{stats:?}");
    assert_eq!(stats[0].path, "logo.png");
    assert_eq!((stats[0].added, stats[0].removed), (0, 0));
}

#[test]
fn view_09_agent_changes_shows_edit_summary() {
    let app = agent_app(Tab::Changes);
    let screen = draw(&app);
    assert!(screen.contains("Cambios · 1 archivo +2 −1"), "{screen}");
    assert!(
        screen
            .lines()
            .any(|l| { l.contains("src/auth.ts") && l.contains("+2") && l.contains("−1") }),
        "{screen}"
    );
}

#[test]
fn view_09_agent_changes_without_diff_shows_empty_message() {
    let mut app = agent_app(Tab::Changes);
    app.agent.as_mut().unwrap().diff = String::new();
    let screen = draw(&app);
    assert!(screen.contains("Sin cambios contra la base."), "{screen}");
    assert!(!screen.contains("archivo"), "{screen}");
}

#[test]
fn chat_streams_partial_response_without_duplicates() {
    let mut app = chat_app();
    ok(&mut app, Req::ChatGet, json!({ "agent_id": "01CHAT" }));
    ok(&mut app, Req::Inspect, inspect());
    ok(
        &mut app,
        Req::Logs,
        json!({ "messages": [ { "role": "USER", "content": "hola" } ], "partial": "Voy a" }),
    );
    let screen = draw(&app);
    assert!(screen.contains("Voy a▍"), "{screen}");
    ok(
        &mut app,
        Req::Logs,
        json!({
            "messages": [
                { "role": "USER", "content": "hola" },
                { "role": "ASSISTANT", "content": "Voy a mirar el bug" }
            ],
            "partial": ""
        }),
    );
    let screen = draw(&app);
    assert!(screen.contains("Voy a mirar el bug"), "{screen}");
    assert!(!screen.contains("▍"), "{screen}");
    assert_eq!(screen.matches("Voy a mirar el bug").count(), 1, "{screen}");
}
