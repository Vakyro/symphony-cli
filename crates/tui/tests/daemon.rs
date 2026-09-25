//! P07.S1: la TUI contra un daemon real, solo por IPC (requests + suscripción al bus).
//! El crate no depende de `symphony-store` ni de `rusqlite`: no puede abrir la base.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use serde_json::json;
use symphony_protocol::transport;
use symphony_tui::app::{App, Call, Msg, Screen, Tab};
use symphony_tui::{io, ui};
use tokio::sync::mpsc;

/// `target/debug` (este test vive en `target/debug/deps`).
fn target_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.parent().unwrap().parent().unwrap().to_path_buf()
}

/// Construye un binario de otro paquete del workspace y devuelve su ruta.
fn build(package: &str, bin: &str) -> PathBuf {
    let status = Command::new(env!("CARGO"))
        .args(["build", "-q", "-p", package, "--bin", bin])
        .status()
        .unwrap();
    assert!(status.success(), "no se pudo construir {bin}");
    target_dir().join(format!("{bin}{}", std::env::consts::EXE_SUFFIX))
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
}

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn draw(app: &App) -> String {
    let mut t = Terminal::new(TestBackend::new(100, 30)).unwrap();
    t.draw(|f| ui::render(app, f)).unwrap();
    t.backend().to_string()
}

struct Driver {
    app: App,
    calls: mpsc::Sender<Call>,
    rx: mpsc::Receiver<Msg>,
    events: usize,
}

impl Driver {
    async fn send(&mut self, calls: Vec<Call>) {
        for c in calls {
            self.calls.send(c).await.unwrap();
        }
    }

    async fn key(&mut self, code: KeyCode) {
        let calls = self.app.update(Msg::Key(KeyEvent::from(code)));
        self.send(calls).await;
    }

    async fn typed(&mut self, text: &str) {
        for c in text.chars() {
            self.key(KeyCode::Char(c)).await;
        }
    }

    /// Procesa mensajes (y ticks) hasta que se cumpla `done`.
    async fn until(&mut self, what: &str, done: impl Fn(&App) -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        while !done(&self.app) {
            let msg = tokio::time::timeout_at(deadline, self.rx.recv())
                .await
                .unwrap_or_else(|_| panic!("timeout esperando: {what}\n{}", draw(&self.app)))
                .expect("canal cerrado");
            if matches!(msg, Msg::Event(_)) {
                self.events += 1;
            }
            let calls = self.app.update(msg);
            self.send(calls).await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tui_drives_a_real_daemon_over_ipc_only() {
    let daemon_bin = build("symphony-daemon", "symphonyd");
    let fake = build("symphony-testkit", "fake-agent");

    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let repo = dir.path().join("mi repo");
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t.local"]);
    git(&repo, &["config", "user.name", "T"]);
    std::fs::write(repo.join("README.md"), "# demo\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    // fake-agent como `claude`, antes que cualquier CLI real del PATH.
    std::fs::copy(
        &fake,
        bin.join(format!("claude{}", std::env::consts::EXE_SUFFIX)),
    )
    .unwrap();
    let path = std::env::join_paths(std::iter::once(bin.clone()).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap();

    let _daemon = Daemon(
        Command::new(&daemon_bin)
            .env("SYMPHONY_HOME", &home)
            .env("PATH", path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let connect = || async {
        for _ in 0..200 {
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
        let mut every = tokio::time::interval(Duration::from_millis(200));
        let mut now = 1_000;
        loop {
            every.tick().await;
            now += 1_000;
            if tx.send(Msg::Tick(now)).await.is_err() {
                return;
            }
        }
    });
    let cwd = repo.display().to_string();
    let mut d = Driver {
        app: App::new(cwd),
        calls,
        rx,
        events: 0,
    };

    // 01 Launch: repo sin Symphony.
    let start = d.app.start();
    d.send(start).await;
    d.until("launch", |a| a.loaded() && !a.providers.is_empty())
        .await;
    assert_eq!(d.app.screen, Screen::Launch);
    assert!(!d.app.initialized());

    // 02 First-run → project.toml.
    d.key(KeyCode::Char('i')).await;
    for _ in 0..4 {
        d.key(KeyCode::Enter).await;
    }
    d.until("init", |a| {
        matches!(a.screen, Screen::Home | Screen::ProviderSetup) && a.initialized()
    })
    .await;
    assert!(repo.join(".symphony").join("project.toml").is_file());
    if d.app.screen == Screen::ProviderSetup {
        d.key(KeyCode::Enter).await;
    }
    assert_eq!(d.app.screen, Screen::Home);

    // 05 por la barra de comandos → 06 vista del agente.
    d.key(KeyCode::Char(':')).await;
    d.typed("spawn claude/sonnet agrega una seccion de uso al README")
        .await;
    d.key(KeyCode::Enter).await;
    d.until("agente creado", |a| {
        a.screen == Screen::Agent && a.agent.as_ref().is_some_and(|v| !v.inspect.is_null())
    })
    .await;
    let view = d.app.agent.as_ref().unwrap();
    assert_eq!(view.inspect["agent"]["number"], 1);
    assert_eq!(
        view.inspect["task"]["title"],
        "agrega una seccion de uso al README"
    );

    // El bus llega por la suscripción (el prompt inicial es un evento).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while d.events == 0 {
        let msg = tokio::time::timeout_at(deadline, d.rx.recv())
            .await
            .expect("no llegó ningún evento del bus")
            .unwrap();
        if matches!(msg, Msg::Event(_)) {
            d.events += 1;
        }
        let calls = d.app.update(msg);
        d.send(calls).await;
    }

    // Conversación: el prompt inicial quedó en `messages`.
    d.key(KeyCode::Char('2')).await;
    d.until("conversación", |a| {
        a.agent
            .as_ref()
            .is_some_and(|v| v.tab == Tab::Conversation && !v.messages.is_empty())
    })
    .await;
    assert!(draw(&d.app).contains("agrega una seccion de uso al README"));

    // 04 Home con el agente listado.
    d.key(KeyCode::Esc).await;
    d.until("home con agentes", |a| !a.agents.is_empty()).await;
    let screen = draw(&d.app);
    assert!(screen.contains("#1"), "{screen}");
    assert!(
        screen.contains("agrega una seccion de uso al README"),
        "{screen}"
    );

    // Suscripción a un tópico desconocido: error explícito.
    let mut raw = connect().await;
    raw.send(&symphony_protocol::Message::Subscribe(
        symphony_protocol::Subscribe::new("s", vec!["nada".into()]),
    ))
    .await
    .unwrap();
    match raw.recv().await.unwrap() {
        Some(symphony_protocol::Message::Response(r)) => assert!(
            matches!(r.outcome, symphony_protocol::Outcome::Error(ref e) if e.code == "unknown_topic")
        ),
        other => panic!("respuesta inesperada: {other:?}"),
    }

    // Un enum inválido se rechaza, nunca cae al default en silencio.
    let mut req = connect().await;
    let reply = io::perform(
        &mut req,
        "x",
        &Call {
            req: symphony_tui::app::Req::Create,
            method: "agent.create",
            params: json!({ "title": "t", "project_root": repo, "failover": "sometimes" }),
        },
    )
    .await;
    assert_eq!(reply.unwrap_err().code, "invalid_params");

    let _ = io::perform(
        &mut req,
        "bye",
        &Call {
            req: symphony_tui::app::Req::Control,
            method: "shutdown",
            params: json!({}),
        },
    )
    .await;
}
