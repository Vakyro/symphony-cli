//! Snapshots de cada vista (FLOW §18) y ramas de FLOW §4, §6, §8, §16, sin daemon:
//! el estado se arma con las mismas respuestas IPC que manda el daemon.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};
use symphony_tui::app::{App, Call, Failure, Msg, Notice, Req, Screen, Tab};
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
          "cli_path": "/usr/bin/claude", "enabled": true, "models": 4 },
        { "id": "openai", "display_name": "Codex", "setup_state": "LOGIN_REQUIRED", "cli_version": "0.40.0",
          "cli_path": "/usr/bin/codex", "enabled": true, "models": 0 },
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
    assert_eq!(app.screen, Screen::Home);
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
    assert_eq!(app.screen, Screen::Home);
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
    assert_eq!(methods(&calls), ["models.list"]);
    ok(&mut app, Req::Models, models());
    press(&mut app, KeyCode::Down);
    insta::assert_snapshot!(draw(&app));
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
    assert_eq!(app.screen, Screen::Home);
    // Sin proyecto en la base, `agent.list` traería agentes de otros proyectos.
    assert_eq!(methods(&calls), ["providers.list", "recovery.list"]);
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
