//! TUI de Symphony (FLOW §1–§7, §12, §13, §16). Habla **solo** con el daemon
//! por IPC: una conexión de requests y otra suscrita al bus. Redibuja cuando
//! llega un mensaje (tecla, respuesta, evento o tick), nunca en un loop ocupado.

pub mod app;
pub mod io;
pub mod ui;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind, MouseEventKind,
};
use symphony_protocol::Connection;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use app::{App, Msg};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Lee el teclado en un hilo propio (crossterm bloquea) hasta que la TUI se cierre.
fn spawn_input(tx: mpsc::Sender<Msg>) {
    std::thread::spawn(move || {
        while !tx.is_closed() {
            match event::poll(Duration::from_millis(200)) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(_) => return,
            }
            let msg = match event::read() {
                // Windows también manda Release: solo cuenta la pulsación.
                Ok(Event::Key(k)) if k.kind != KeyEventKind::Release => Msg::Key(k),
                Ok(Event::Resize(..)) => Msg::Resize,
                Ok(Event::Mouse(m)) => match m.kind {
                    MouseEventKind::ScrollUp => Msg::Scroll(3),
                    MouseEventKind::ScrollDown => Msg::Scroll(-3),
                    _ => continue,
                },
                Ok(_) => continue,
                Err(_) => return,
            };
            if tx.blocking_send(msg).is_err() {
                return;
            }
        }
    });
}

/// Corre la TUI en la terminal hasta que el usuario salga.
/// `cwd`: carpeta desde la que se abrió Symphony.
pub async fn run<S>(
    requests: Connection<S>,
    events: Connection<S>,
    cwd: String,
) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (tx, mut rx) = mpsc::channel::<Msg>(256);
    let calls = io::spawn(requests, events, tx.clone());
    spawn_input(tx.clone());
    let ticks = tx.clone();
    tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_secs(1));
        loop {
            every.tick().await;
            if ticks.send(Msg::Tick(now_ms())).await.is_err() {
                return;
            }
        }
    });
    drop(tx);

    let mut app = App::new(cwd);
    app.now_ms = now_ms();
    let mut terminal = ratatui::init();
    // Para la rueda; con la captura activa, seleccionar texto pide Shift.
    let _ = ratatui::crossterm::execute!(std::io::stdout(), EnableMouseCapture);
    let result = async {
        let mut pending = app.start();
        loop {
            for call in pending.drain(..) {
                if calls.send(call).await.is_err() {
                    app.update(Msg::Disconnected("la conexión de requests se cerró".into()));
                }
            }
            if app.quit {
                return Ok(());
            }
            terminal.draw(|f| ui::render(&app, f))?;
            // Mientras el agente trabaja se redibuja ~7 veces por segundo (el indicador gira).
            let received = if app.animating() {
                match tokio::time::timeout(Duration::from_millis(150), rx.recv()).await {
                    Ok(m) => m,
                    Err(_) => {
                        app.frame = app.frame.wrapping_add(1);
                        continue;
                    }
                }
            } else {
                rx.recv().await
            };
            let Some(msg) = received else {
                return Ok(());
            };
            pending = app.update(msg);
            // Una ráfaga de mensajes se dibuja una sola vez.
            while let Ok(msg) = rx.try_recv() {
                pending.extend(app.update(msg));
            }
        }
    }
    .await;
    let _ = ratatui::crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}
