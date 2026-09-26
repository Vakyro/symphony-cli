//! Helpers de los tests que corren la TUI contra un daemon real.
// Cada archivo de tests usa solo parte de estos helpers.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use symphony_tui::app::{App, Call, Msg};
use symphony_tui::ui;
use tokio::sync::mpsc;

/// `target/debug` (este test vive en `target/debug/deps`).
pub fn target_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.parent().unwrap().parent().unwrap().to_path_buf()
}

/// Construye un binario de otro paquete del workspace y devuelve su ruta.
pub fn build(package: &str, bin: &str) -> PathBuf {
    let status = Command::new(env!("CARGO"))
        .args(["build", "-q", "-p", package, "--bin", bin])
        .status()
        .unwrap();
    assert!(status.success(), "no se pudo construir {bin}");
    target_dir().join(format!("{bin}{}", std::env::consts::EXE_SUFFIX))
}

pub fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
}

pub struct Daemon(pub Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn draw(app: &App) -> String {
    let mut t = Terminal::new(TestBackend::new(100, 30)).unwrap();
    t.draw(|f| ui::render(app, f)).unwrap();
    t.backend().to_string()
}

pub struct Driver {
    pub app: App,
    pub calls: mpsc::Sender<Call>,
    pub rx: mpsc::Receiver<Msg>,
    pub events: usize,
}

impl Driver {
    pub async fn send(&mut self, calls: Vec<Call>) {
        for c in calls {
            self.calls.send(c).await.unwrap();
        }
    }

    pub async fn key(&mut self, code: KeyCode) {
        let calls = self.app.update(Msg::Key(KeyEvent::from(code)));
        self.send(calls).await;
    }

    pub async fn typed(&mut self, text: &str) {
        for c in text.chars() {
            self.key(KeyCode::Char(c)).await;
        }
    }

    /// Procesa mensajes (y ticks) hasta que se cumpla `done`.
    pub async fn until(&mut self, what: &str, done: impl Fn(&App) -> bool) {
        self.until_within(what, Duration::from_secs(30), done).await;
    }

    pub async fn until_within(&mut self, what: &str, limit: Duration, done: impl Fn(&App) -> bool) {
        let deadline = tokio::time::Instant::now() + limit;
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
