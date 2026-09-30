//! TUI de Symphony (FLOW §1–§7, §12, §13, §16). Habla **solo** con el daemon
//! por IPC: una conexión de requests y otra suscrita al bus. Redibuja cuando
//! llega un mensaje (tecla, respuesta, evento o tick), nunca en un loop ocupado.

pub mod app;
pub mod io;
pub mod ui;

use std::collections::VecDeque;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
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
/// Cuánto se espera tras un Enter para ver si lo que sigue es un pegado.
const PASTE_WINDOW: Duration = Duration::from_millis(15);

/// ¿Este Enter forma parte de un pegado? La consola de Windows no manda pegado entre corchetes:
/// las líneas llegan como Enter seguidos de más teclas, y una persona no teclea tan rápido.
/// `next(espera)` da el siguiente evento si llega a tiempo. El «soltar» del propio Enter no
/// cuenta (Windows lo manda siempre) y lo que no es tecla (ratón, foco) tampoco delata un pegado;
/// lo que se leyó de más pasa a `later` para procesarse después.
fn enter_is_pasted(
    mut next: impl FnMut(Duration) -> Option<Event>,
    later: &mut VecDeque<Event>,
) -> bool {
    while let Some(e) = next(PASTE_WINDOW) {
        match e {
            Event::Key(k) if k.kind == KeyEventKind::Release => {}
            Event::Key(_) => {
                later.push_back(e);
                return true;
            }
            other => later.push_back(other),
        }
    }
    false
}

fn spawn_input(tx: mpsc::Sender<Msg>) {
    std::thread::spawn(move || {
        let mut later: VecDeque<Event> = VecDeque::new();
        while !tx.is_closed() {
            let ev = match later.pop_front() {
                Some(e) => e,
                None => {
                    match event::poll(Duration::from_millis(200)) {
                        Ok(true) => {}
                        Ok(false) => continue,
                        Err(_) => return,
                    }
                    match event::read() {
                        Ok(e) => e,
                        Err(_) => return,
                    }
                }
            };
            let msg = match ev {
                // Windows también manda Release: solo cuenta la pulsación.
                Event::Key(k) if k.kind != KeyEventKind::Release => {
                    let pasted = k.code == KeyCode::Enter
                        && enter_is_pasted(
                            |wait| {
                                event::poll(wait)
                                    .ok()
                                    .filter(|ready| *ready)
                                    .and_then(|_| event::read().ok())
                            },
                            &mut later,
                        );
                    if pasted {
                        Msg::Key(KeyEvent::new(KeyCode::Char(NEWLINE), KeyModifiers::NONE))
                    } else {
                        Msg::Key(k)
                    }
                }
                Event::Resize(..) => Msg::Resize,
                Event::Paste(text) => Msg::Paste(text),
                Event::Mouse(m) => match m.kind {
                    MouseEventKind::ScrollUp => Msg::Scroll(3),
                    MouseEventKind::ScrollDown => Msg::Scroll(-3),
                    _ => continue,
                },
                _ => continue,
            };
            if tx.blocking_send(msg).is_err() {
                return;
            }
        }
    });
}

/// Tecla sintética de un salto de línea pegado (ver `spawn_input`).
pub const NEWLINE: char = '\n';

/// Copia `text` al portapapeles con la herramienta del sistema (sin dependencias nuevas).
fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let (mut cmd, bytes): (Command, Vec<u8>) = if cfg!(windows) {
        // `clip` deja un BOM al inicio del texto; PowerShell con stdin en UTF-8 lo copia limpio.
        let mut c = Command::new("powershell");
        c.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::InputEncoding = [Text.Encoding]::UTF8; Set-Clipboard -Value ([Console]::In.ReadToEnd())",
        ]);
        (c, text.as_bytes().to_vec())
    } else if cfg!(target_os = "macos") {
        (Command::new("pbcopy"), text.as_bytes().to_vec())
    } else {
        let mut c = Command::new("xclip");
        c.args(["-selection", "clipboard"]);
        (c, text.as_bytes().to_vec())
    };
    let mut child = cmd.stdin(Stdio::piped()).spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&bytes)?;
    }
    // No se espera: PowerShell tarda ~0,5 s en arrancar y la TUI no debe congelarse.
    std::thread::spawn(move || child.wait());
    Ok(())
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
    let _ =
        ratatui::crossterm::execute!(std::io::stdout(), EnableMouseCapture, EnableBracketedPaste);
    let result = async {
        let mut mouse_on = true;
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
            if let Some(text) = app.copy.take() {
                match copy_to_clipboard(&text) {
                    Ok(()) => app.info(format!("Copiado ({} caracteres).", text.chars().count())),
                    Err(e) => app.error(format!("No se pudo copiar: {e}")),
                }
            }
            if app.mouse != mouse_on {
                mouse_on = app.mouse;
                let _ = if mouse_on {
                    ratatui::crossterm::execute!(std::io::stdout(), EnableMouseCapture)
                } else {
                    ratatui::crossterm::execute!(std::io::stdout(), DisableMouseCapture)
                };
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
    let _ = ratatui::crossterm::execute!(
        std::io::stdout(),
        DisableMouseCapture,
        DisableBracketedPaste
    );
    ratatui::restore();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEventState, MouseEvent};

    fn key(code: KeyCode, kind: KeyEventKind) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind,
            state: KeyEventState::NONE,
        })
    }

    fn pasted(events: Vec<Event>) -> (bool, usize) {
        let mut it = events.into_iter();
        let mut later = VecDeque::new();
        let r = enter_is_pasted(|_| it.next(), &mut later);
        (r, later.len())
    }

    #[test]
    fn a_lone_enter_is_not_a_paste_even_with_its_own_release() {
        assert_eq!(pasted(vec![]), (false, 0));
        // Windows manda el «soltar» tras cada tecla: no delata un pegado.
        assert_eq!(
            pasted(vec![key(KeyCode::Enter, KeyEventKind::Release)]),
            (false, 0)
        );
    }

    #[test]
    fn enter_followed_by_more_keys_is_a_paste() {
        let next = key(KeyCode::Char('a'), KeyEventKind::Press);
        assert_eq!(pasted(vec![next.clone()]), (true, 1));
        let release = key(KeyCode::Enter, KeyEventKind::Release);
        assert_eq!(pasted(vec![release, next]), (true, 1));
    }

    #[test]
    fn mouse_events_after_enter_do_not_make_it_a_paste_but_are_kept() {
        let moved = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(pasted(vec![moved]), (false, 1));
    }
}
