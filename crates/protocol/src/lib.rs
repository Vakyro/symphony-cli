//! Protocolo IPC de Symphony: frames con longitud + JSON tipado y versionado
//! (STACK §6.2, §46, §47). El transporte (named pipe / unix socket vía
//! `interprocess`) lo elige quien abre la conexión; aquí solo hay bytes.

mod frame;
mod message;
pub mod transport;

pub use frame::{FrameDecoder, MAX_FRAME_LEN, encode_frame, read_frame, write_frame};
pub use message::{
    ErrorBody, Event, Message, Outcome, PROTOCOL_VERSION, Request, Response, Subscribe,
};

use tokio::io::{AsyncRead, AsyncWrite};

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("frame de {len} bytes supera el máximo de {MAX_FRAME_LEN}")]
    FrameTooLarge { len: u64 },
    #[error("la conexión se cerró a mitad de un frame")]
    Truncated,
    #[error("versión de protocolo {got} no soportada (esta versión habla {PROTOCOL_VERSION})")]
    UnsupportedVersion { got: u64 },
    #[error("mensaje sin campo `protocol`")]
    MissingVersion,
    #[error("mensaje inválido: {0}")]
    Json(#[from] serde_json::Error),
    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),
}

/// Conexión que envía y recibe `Message` enmarcados sobre cualquier stream async.
#[derive(Debug)]
pub struct Connection<S> {
    stream: S,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Connection<S> {
    pub fn new(stream: S) -> Self {
        Self { stream }
    }

    pub async fn send(&mut self, msg: &Message) -> Result<(), ProtocolError> {
        write_frame(&mut self.stream, &msg.to_bytes()?).await
    }

    /// El siguiente mensaje, o `None` si el otro extremo cerró limpio.
    pub async fn recv(&mut self) -> Result<Option<Message>, ProtocolError> {
        match read_frame(&mut self.stream).await? {
            Some(bytes) => Message::from_bytes(&bytes).map(Some),
            None => Ok(None),
        }
    }

    pub fn into_inner(self) -> S {
        self.stream
    }
}
