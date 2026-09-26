//! P07.S7 (L3, gasta cuota): Journey A con Claude Code real, manejado con la
//! misma lógica de la TUI (`App` + IPC) contra un daemon real.
//!
//!   SYMPHONY_LIVE=1 cargo nextest run -p symphony-tui --no-capture live_
//!
//! Solo con permiso de Leo. Modelo barato (`claude/haiku`) y una tarea mínima.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::{Daemon, Driver, build, draw, git};

use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ratatui::crossterm::event::KeyCode;
use symphony_protocol::transport;
use symphony_tui::app::{App, Msg, Screen, Tab};
use symphony_tui::io;
use tokio::sync::mpsc;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}

fn agent_state(app: &App) -> String {
    app.agent
        .as_ref()
        .map(|a| a.state().to_string())
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_journey_a_with_claude_code() {
    if std::env::var("SYMPHONY_LIVE").as_deref() != Ok("1") {
        eprintln!("omitido: define SYMPHONY_LIVE=1 (gasta cuota; solo con permiso de Leo)");
        return;
    }
    let daemon_bin = build("symphony-daemon", "symphonyd");
    // `symphony hook emit` junto a symphonyd: los hooks de Claude lo usan.
    build("symphony-cli", "symphony");

    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let repo = dir.path().join("demo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t.local"]);
    git(&repo, &["config", "user.name", "T"]);
    std::fs::write(repo.join("README.md"), "# demo\n\nA tiny demo project.\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);

    let _daemon = Daemon(
        Command::new(&daemon_bin)
            .env("SYMPHONY_HOME", &home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let connect = || async {
        for _ in 0..400 {
            if let Ok(c) = transport::connect(&home).await {
                return c;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("el daemon no arrancó");
    };
    let requests = connect().await;
    let events = connect().await;
    let (tx, rx) = mpsc::channel(256);
    let calls = io::spawn(requests, events, tx.clone());
    tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_secs(1));
        loop {
            every.tick().await;
            if tx.send(Msg::Tick(now_ms())).await.is_err() {
                return;
            }
        }
    });
    let mut d = Driver {
        app: App::new(repo.display().to_string()),
        calls,
        rx,
        events: 0,
    };
    let started = std::time::Instant::now();

    // Abrir → inicializar → Home.
    let start = d.app.start();
    d.send(start).await;
    d.until("launch", |a| a.loaded() && !a.providers.is_empty())
        .await;
    d.key(KeyCode::Char('i')).await;
    for _ in 0..4 {
        d.key(KeyCode::Enter).await;
    }
    d.until("init", |a| {
        matches!(a.screen, Screen::Home | Screen::ProviderSetup) && a.initialized()
    })
    .await;
    println!("── Proveedores detectados\n{}", draw(&d.app));
    if d.app.screen == Screen::ProviderSetup {
        d.key(KeyCode::Enter).await;
    }

    // Crear el agente con modelo exacto por el formulario (vistas 05 y 13).
    d.key(KeyCode::Char('n')).await;
    d.typed(
        "Add a section titled '## Usage' at the end of README.md with one line: \
         'Run `symphony` inside the repo.' Do not run any command. Then reply: done",
    )
    .await;
    d.key(KeyCode::Down).await;
    d.key(KeyCode::Right).await;
    d.key(KeyCode::Enter).await;
    d.until("modelos", |a| !a.models.is_empty()).await;
    let haiku = d
        .app
        .models
        .iter()
        .position(|m| m["id"] == "claude/haiku")
        .expect("claude/haiku en el Model Picker");
    assert_eq!(
        d.app.models[haiku]["available"], true,
        "Claude no está listo"
    );
    for _ in 0..haiku {
        d.key(KeyCode::Down).await;
    }
    d.key(KeyCode::Enter).await;
    assert_eq!(d.app.new_agent.model.as_deref(), Some("claude/haiku"));
    d.key(KeyCode::Down).await;
    d.key(KeyCode::Down).await;
    d.key(KeyCode::Enter).await;
    d.until("vista del agente", |a| {
        a.screen == Screen::Agent && a.agent.as_ref().is_some_and(|v| !v.inspect.is_null())
    })
    .await;
    println!(
        "── Agente creado ({} ms)\n{}",
        started.elapsed().as_millis(),
        draw(&d.app)
    );

    // Verlo trabajar en la Conversación hasta que termine.
    d.key(KeyCode::Char('2')).await;
    d.until_within("fin de la tarea", Duration::from_secs(300), |a| {
        matches!(
            agent_state(a).as_str(),
            "COMPLETED" | "FAILED" | "CANCELLED" | "BLOCKED" | "WAITING_PROVIDER"
        )
    })
    .await;
    let took = started.elapsed();
    // Una vuelta más para que la pestaña tenga la conversación final.
    d.key(KeyCode::Char('2')).await;
    d.until("conversación final", |a| {
        a.agent
            .as_ref()
            .is_some_and(|v| v.messages.iter().any(|m| m["role"] == "ASSISTANT"))
    })
    .await;
    println!("── Conversación ({} s)\n{}", took.as_secs(), draw(&d.app));
    assert_eq!(agent_state(&d.app), "COMPLETED", "{}", draw(&d.app));

    // Actividad, cambios (diff) e historial.
    d.key(KeyCode::Char('3')).await;
    d.until("actividad", |a| {
        a.agent.as_ref().is_some_and(|v| v.tab == Tab::Activity)
    })
    .await;
    d.until("actividad cargada", |a| {
        a.agent.as_ref().is_some_and(|v| !v.activity.is_empty())
    })
    .await;
    println!("── Actividad\n{}", draw(&d.app));
    d.key(KeyCode::Char('d')).await;
    d.until("diff", |a| {
        a.agent.as_ref().is_some_and(|v| v.diff.contains("Usage"))
    })
    .await;
    println!("── Cambios\n{}", draw(&d.app));
    d.key(KeyCode::Char('5')).await;
    d.until("historial", |a| {
        a.agent.as_ref().is_some_and(|v| !v.history.is_null())
    })
    .await;
    println!("── Historial\n{}", draw(&d.app));

    d.key(KeyCode::Esc).await;
    d.until("home", |a| a.screen == Screen::Home && !a.agents.is_empty())
        .await;
    println!("── Inicio\n{}", draw(&d.app));
    assert!(d.events > 0, "no llegó ningún evento del bus");
    println!(
        "Journey A live: {} eventos del bus, {} s en total",
        d.events,
        started.elapsed().as_secs()
    );
}
