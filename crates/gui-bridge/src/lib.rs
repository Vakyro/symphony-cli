//! Puente entre la GUI y el daemon (P15.S1). La GUI (Tauri) llama aquí y no sabe nada del
//! protocolo: `call` hace un request y `subscribe` reenvía los eventos del bus. **Sin lógica de
//! negocio**: lo que hace la GUI lo hace el daemon (FLOW regla 9).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use symphony_protocol::transport::{self, LocalStream};
use symphony_protocol::{Connection, Message, Outcome, Request, Subscribe};

const CALL_TIMEOUT: Duration = Duration::from_secs(15);
const START_TIMEOUT: Duration = Duration::from_secs(35);
const CONNECT_RETRIES: u32 = 20;
const CONNECT_RETRY_EVERY: Duration = Duration::from_millis(250);

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("el daemon no está disponible: {0}")]
    Unavailable(String),
    #[error("el daemon no respondió a `{0}` a tiempo")]
    Timeout(String),
    /// Error devuelto por el daemon (`code` es estable; `message` es para mostrar).
    #[error("{message}")]
    Remote { code: String, message: String },
}

/// Un evento del bus: tema y carga.
#[derive(Debug, Clone, PartialEq)]
pub struct BusEvent {
    pub topic: String,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct Bridge {
    home: PathBuf,
    /// Un solo `ensure_daemon` a la vez: el arranque de la GUI y el botón «Arrancarlo» coinciden.
    starting: Arc<tokio::sync::Mutex<()>>,
}

impl Bridge {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            starting: Arc::default(),
        }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    async fn connect(&self) -> Result<Connection<LocalStream>, BridgeError> {
        transport::connect(&self.home)
            .await
            .map_err(|e| BridgeError::Unavailable(e.to_string()))
    }

    /// Hace un request y devuelve su resultado. Una conexión por llamada: son locales y baratas.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, BridgeError> {
        let mut conn = self.connect().await?;
        let id = "gui";
        let exchange = async {
            conn.send(&Message::Request(Request::new(id, method, params)))
                .await
                .map_err(|e| BridgeError::Unavailable(e.to_string()))?;
            loop {
                match conn.recv().await {
                    Ok(Some(Message::Response(r))) if r.id == id => return Ok(r.outcome),
                    Ok(Some(_)) => continue,
                    Ok(None) => {
                        return Err(BridgeError::Unavailable(
                            "el daemon cerró la conexión".into(),
                        ));
                    }
                    Err(e) => return Err(BridgeError::Unavailable(e.to_string())),
                }
            }
        };
        match tokio::time::timeout(CALL_TIMEOUT, exchange).await {
            Err(_) => Err(BridgeError::Timeout(method.into())),
            Ok(Err(e)) => Err(e),
            Ok(Ok(Outcome::Ok(v))) => Ok(v),
            Ok(Ok(Outcome::Error(e))) => Err(BridgeError::Remote {
                code: e.code,
                message: e.message,
            }),
        }
    }

    /// Se suscribe al bus (`agent.event`) y llama a `on_event` por cada evento hasta que la
    /// conexión se corte (devuelve entonces el motivo).
    pub async fn subscribe(&self, on_event: impl FnMut(BusEvent)) -> BridgeError {
        self.subscribe_with(|| {}, on_event).await
    }

    /// Como [`Bridge::subscribe`], y avisa con `on_ready` en cuanto la suscripción queda hecha
    /// (la GUI lo usa para saber que el daemon volvió).
    pub async fn subscribe_with(
        &self,
        on_ready: impl FnOnce(),
        mut on_event: impl FnMut(BusEvent),
    ) -> BridgeError {
        let mut conn = match self.connect().await {
            Ok(c) => c,
            Err(e) => return e,
        };
        let sub = Subscribe::new("gui-events", vec!["agent.event".into()]);
        if let Err(e) = conn.send(&Message::Subscribe(sub)).await {
            return BridgeError::Unavailable(e.to_string());
        }
        on_ready();
        loop {
            match conn.recv().await {
                Ok(Some(Message::Event(e))) => on_event(BusEvent {
                    topic: e.topic,
                    payload: e.payload,
                }),
                Ok(Some(_)) => {}
                Ok(None) => {
                    return BridgeError::Unavailable("el daemon cerró la suscripción".into());
                }
                Err(e) => return BridgeError::Unavailable(e.to_string()),
            }
        }
    }

    /// Si el daemon no responde, lo arranca con `symphony daemon start` (que sabe desacoplarlo
    /// de este proceso) y espera a que conteste. `symphony` se busca junto al ejecutable actual
    /// y en el PATH (o `$SYMPHONY_BIN`).
    pub async fn ensure_daemon(&self) -> Result<(), BridgeError> {
        let _one_at_a_time = self.starting.lock().await;
        if self.connect().await.is_ok() {
            return Ok(());
        }
        let bin = cli_binary().ok_or_else(|| {
            BridgeError::Unavailable("no encontré el ejecutable `symphony` (instálalo)".into())
        })?;
        let status = tokio::process::Command::new(&bin)
            .args(["daemon", "start"])
            .env("SYMPHONY_HOME", &self.home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match tokio::time::timeout(START_TIMEOUT, status).await {
            Err(_) => Err(BridgeError::Unavailable(
                "el daemon tardó demasiado en arrancar".into(),
            )),
            Ok(Err(e)) => Err(BridgeError::Unavailable(format!(
                "no se pudo ejecutar `{}`: {e}",
                bin.display()
            ))),
            Ok(Ok(s)) if !s.success() => Err(BridgeError::Unavailable(format!(
                "`symphony daemon start` terminó con {s}"
            ))),
            // `daemon start` puede volver antes de que el socket escuche: se reintenta un rato.
            Ok(Ok(_)) => {
                let mut last = BridgeError::Unavailable("el daemon no contestó".into());
                for _ in 0..CONNECT_RETRIES {
                    match self.connect().await {
                        Ok(_) => return Ok(()),
                        Err(e) => last = e,
                    }
                    tokio::time::sleep(CONNECT_RETRY_EVERY).await;
                }
                Err(last)
            }
        }
    }
}

fn cli_binary() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SYMPHONY_BIN").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let name = format!("symphony{}", std::env::consts::EXE_SUFFIX);
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(&name)))
        .filter(|p| p.is_file());
    sibling.or_else(|| {
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|d| d.join(&name))
            .find(|p| p.is_file())
    })
}
