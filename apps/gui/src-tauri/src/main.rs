//! Lado Rust de la GUI (P15.S1). Solo es un puente: `ipc_call` reenvía un request al daemon y
//! el bus se reemite como el evento `symphony://event`. Cero lógica de negocio.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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

/// Un request al daemon. El error es `{code, message}` para que la UI decida qué mostrar.
#[tauri::command]
async fn ipc_call(
    daemon: State<'_, Daemon>,
    method: String,
    params: Value,
) -> Result<Value, Value> {
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
        loop {
            let emit = app.clone();
            let why = bridge
                .subscribe(move |ev| {
                    let _ = emit.emit(
                        "symphony://event",
                        json!({ "topic": ev.topic, "payload": ev.payload }),
                    );
                })
                .await;
            let _ = app.emit("symphony://disconnected", why.to_string());
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });
}

fn main() {
    let home = symphony_core::SymphonyHome::resolve()
        .map(|h| h.root().to_path_buf())
        .unwrap_or_else(|_| std::path::PathBuf::from(".symphony"));
    let bridge = Bridge::new(home);
    let events = bridge.clone();
    tauri::Builder::default()
        .manage(Daemon(bridge))
        .setup(move |app| {
            forward_events(app.handle().clone(), events.clone());
            let _ = app.get_webview_window("main");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![ipc_call, daemon_ensure])
        .run(tauri::generate_context!())
        .expect("error al iniciar la GUI de Symphony");
}
