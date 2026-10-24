//! Lado Rust de la GUI (P15.S1). Solo es un puente: `ipc_call` reenvía un request al daemon y
//! el bus se reemite como el evento `symphony://event`. Cero lógica de negocio.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};
use symphony_gui_bridge::{Bridge, BridgeError};
use tauri::{AppHandle, Emitter, Manager, State};

struct Daemon(Bridge);

fn failure(e: &BridgeError) -> Value {
    match e {
        BridgeError::Remote { code, message } => json!({ "code": code, "message": message }),
        BridgeError::Timeout(_) => json!({ "code": "timeout", "message": e.to_string() }),
        BridgeError::Unavailable(_) => json!({ "code": "disconnected", "message": e.to_string() }),
    }
}

/// `gui.set_project`: la UI eligió otra carpeta de proyecto. Cada proyecto tiene su daemon
/// (ADR-0018); esto solo apunta el puente al de esa carpeta, no habla con ningún daemon.
fn set_project(bridge: &Bridge, params: &Value) -> Result<Value, Value> {
    let root = params
        .get("root")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if root.is_empty() {
        bridge.set_home(None);
        return Ok(json!({ "home": null }));
    }
    match symphony_core::SymphonyHome::for_project(std::path::Path::new(root)) {
        Ok(home) => {
            bridge.set_home(Some(home.root().to_path_buf()));
            Ok(json!({ "home": home.root().display().to_string() }))
        }
        Err(e) => Err(json!({ "code": "invalid_project", "message": e.to_string() })),
    }
}

/// Un request al daemon. El error es `{code, message}` para que la UI decida qué mostrar.
#[tauri::command]
async fn ipc_call(
    daemon: State<'_, Daemon>,
    method: String,
    params: Value,
) -> Result<Value, Value> {
    if method == "gui.set_project" {
        return set_project(&daemon.0, &params);
    }
    daemon
        .0
        .call(&method, params)
        .await
        .map_err(|e| failure(&e))
}

/// Arranca el daemon si no responde (como hace `symphony`).
#[tauri::command]
async fn daemon_ensure(daemon: State<'_, Daemon>) -> Result<(), Value> {
    daemon.0.ensure_daemon().await.map_err(|e| failure(&e))
}

/// Reemite el bus al WebView; si se corta, avisa y reintenta (el daemon puede reiniciarse).
fn forward_events(app: AppHandle, bridge: Bridge) {
    tauri::async_runtime::spawn(async move {
        // Se avisa una vez por caída, no en cada reintento.
        let mut announced = false;
        loop {
            let emit = app.clone();
            let ready = app.clone();
            let was_up = Arc::new(AtomicBool::new(false));
            let up = was_up.clone();
            let listening = bridge.subscribe_with(
                move || {
                    up.store(true, Ordering::Relaxed);
                    // Evento sintético: las vistas se refrescan y la UI vuelve a «conectado».
                    let _ = ready.emit(
                        "symphony://event",
                        json!({ "topic": "gui.connected", "payload": null }),
                    );
                },
                move |ev| {
                    let _ = emit.emit(
                        "symphony://event",
                        json!({ "topic": ev.topic, "payload": ev.payload }),
                    );
                },
            );
            let why = tokio::select! {
                why = listening => why,
                // Otro proyecto: se deja la suscripción al daemon anterior y se hace la nueva.
                () = bridge.project_changed() => {
                    announced = false;
                    continue;
                }
            };
            if was_up.load(Ordering::Relaxed) {
                announced = false;
            }
            if !announced {
                let _ = app.emit("symphony://disconnected", why.to_string());
                announced = true;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });
}

/// Sin consola (`windows_subsystem`), un error de arranque solo se ve en el código de salida.
fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("symphony-gui: {msg}");
    std::process::exit(1)
}

fn main() {
    // El proyecto del directorio desde donde se abrió la GUI, si lo es; si no, la UI pide uno.
    let bridge = match symphony_core::SymphonyHome::resolve() {
        Ok(h) => Bridge::new(h.root()),
        Err(_) => Bridge::without_project(),
    };
    let bridge = match symphony_core::SymphonyHome::user() {
        Ok(u) => bridge.with_user_home(u.root()),
        Err(e) => die(format!("no pude ubicar ~/.symphony: {e}")),
    };
    let events = bridge.clone();
    let built = tauri::Builder::default()
        .manage(Daemon(bridge))
        .setup(move |app| {
            forward_events(app.handle().clone(), events.clone());
            let _ = app.get_webview_window("main");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![ipc_call, daemon_ensure])
        .run(tauri::generate_context!());
    if let Err(e) = built {
        die(format!("error al iniciar la GUI: {e}"));
    }
}
