//! E/S de la TUI con el daemon: una conexión para requests (en orden) y otra
//! suscrita al bus. Nunca abre la base: solo IPC (PLAN P07.S1).

use std::time::Duration;

use serde_json::Value;
use symphony_protocol::{Connection, Message, Outcome, Request, Subscribe};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use crate::app::{Call, Failure, Msg};

const CALL_TIMEOUT: Duration = Duration::from_secs(15);

fn lost(message: impl Into<String>) -> Failure {
    Failure {
        code: "disconnected".into(),
        message: message.into(),
    }
}

/// Hace una llamada y espera su respuesta.
pub async fn perform<S>(conn: &mut Connection<S>, id: &str, call: &Call) -> Result<Value, Failure>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let exchange = async {
        let req = Request::new(id, call.method, call.params.clone());
        conn.send(&Message::Request(req))
            .await
            .map_err(|e| lost(e.to_string()))?;
        loop {
            match conn.recv().await {
                Ok(Some(Message::Response(r))) if r.id == id => return Ok(r.outcome),
                Ok(Some(_)) => continue,
                Ok(None) => return Err(lost("el daemon cerró la conexión")),
                Err(e) => return Err(lost(e.to_string())),
            }
        }
    };
    match tokio::time::timeout(CALL_TIMEOUT, exchange).await {
        Err(_) => Err(Failure {
            code: "timeout".into(),
            message: format!("el daemon no respondió a `{}` a tiempo", call.method),
        }),
        Ok(Err(f)) => Err(f),
        Ok(Ok(Outcome::Ok(v))) => Ok(v),
        Ok(Ok(Outcome::Error(e))) => Err(Failure {
            code: e.code,
            message: e.message,
        }),
    }
}

/// Se suscribe al bus y reenvía cada evento como `Msg::Event` hasta que se corte.
pub async fn subscribe<S>(mut conn: Connection<S>, tx: mpsc::Sender<Msg>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let sub = Subscribe::new("tui-events", vec!["agent.event".into()]);
    if let Err(e) = conn.send(&Message::Subscribe(sub)).await {
        let _ = tx.send(Msg::Disconnected(e.to_string())).await;
        return;
    }
    loop {
        let msg = match conn.recv().await {
            Ok(Some(Message::Event(e))) => Msg::Event(e.topic),
            Ok(Some(Message::Response(r))) => match r.outcome {
                Outcome::Ok(_) => continue,
                Outcome::Error(e) => Msg::Disconnected(e.message),
            },
            Ok(Some(_)) => continue,
            Ok(None) => Msg::Disconnected("el daemon cerró la suscripción".into()),
            Err(e) => Msg::Disconnected(e.to_string()),
        };
        let stop = matches!(msg, Msg::Disconnected(_));
        if tx.send(msg).await.is_err() || stop {
            return;
        }
    }
}

/// Lanza las tareas de E/S. Devuelve por dónde mandar las llamadas; las
/// respuestas y los eventos llegan por `tx`.
pub fn spawn<S>(
    mut requests: Connection<S>,
    events: Connection<S>,
    tx: mpsc::Sender<Msg>,
) -> mpsc::Sender<Call>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (calls, mut rx) = mpsc::channel::<Call>(64);
    let replies = tx.clone();
    tokio::spawn(async move {
        let mut n = 0u64;
        while let Some(call) = rx.recv().await {
            n += 1;
            let reply = perform(&mut requests, &format!("tui-{n}"), &call).await;
            let gone = matches!(&reply, Err(f) if f.code == "disconnected");
            if replies.send(Msg::Reply(call.req, reply)).await.is_err() || gone {
                return;
            }
        }
    });
    tokio::spawn(subscribe(events, tx));
    calls
}
