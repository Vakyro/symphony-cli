//! Framing: cada mensaje va precedido por su longitud en un `u32` big-endian
//! (STACK §6.2). El tamaño se valida antes de reservar memoria (STACK §47).

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::ProtocolError;

/// Tamaño máximo de un frame. Un mensaje del protocolo nunca se acerca a esto;
/// los blobs grandes viajan por el object store, no por IPC.
pub const MAX_FRAME_LEN: u32 = 8 * 1024 * 1024;

const HEADER_LEN: usize = 4;

/// Devuelve el frame completo (cabecera + payload) listo para escribir.
pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    let len = u32::try_from(payload.len())
        .ok()
        .filter(|&n| n <= MAX_FRAME_LEN)
        .ok_or(ProtocolError::FrameTooLarge {
            len: payload.len() as u64,
        })?;
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// Decodificador incremental: se le pasan bytes en cualquier partición y
/// devuelve frames completos en orden.
#[derive(Debug, Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// El siguiente frame completo, `None` si todavía faltan bytes, o error si
    /// la cabecera anuncia un frame demasiado grande.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, ProtocolError> {
        let Some(header) = self.buf.first_chunk::<HEADER_LEN>() else {
            return Ok(None);
        };
        let len = u32::from_be_bytes(*header);
        if len > MAX_FRAME_LEN {
            return Err(ProtocolError::FrameTooLarge {
                len: u64::from(len),
            });
        }
        let end = HEADER_LEN + len as usize;
        if self.buf.len() < end {
            return Ok(None);
        }
        let frame = self.buf[HEADER_LEN..end].to_vec();
        self.buf.drain(..end);
        Ok(Some(frame))
    }

    /// Bytes recibidos que todavía no forman un frame completo.
    pub fn pending(&self) -> usize {
        self.buf.len()
    }
}

/// Lee un frame. `Ok(None)` si el otro extremo cerró limpio entre frames;
/// error si cerró a mitad de un frame.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Option<Vec<u8>>, ProtocolError> {
    let mut header = [0u8; HEADER_LEN];
    let mut got = 0;
    while got < HEADER_LEN {
        match r.read(&mut header[got..]).await? {
            0 if got == 0 => return Ok(None),
            0 => return Err(ProtocolError::Truncated),
            n => got += n,
        }
    }
    let len = u32::from_be_bytes(header);
    if len > MAX_FRAME_LEN {
        return Err(ProtocolError::FrameTooLarge {
            len: u64::from(len),
        });
    }
    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload)
        .await
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::UnexpectedEof => ProtocolError::Truncated,
            _ => ProtocolError::Io(e),
        })?;
    Ok(Some(payload))
}

pub async fn write_frame<W: AsyncWrite + Unpin>(
    w: &mut W,
    payload: &[u8],
) -> Result<(), ProtocolError> {
    w.write_all(&encode_frame(payload)?).await?;
    w.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn payloads() -> impl Strategy<Value = Vec<Vec<u8>>> {
        prop::collection::vec(prop::collection::vec(any::<u8>(), 0..512), 0..8)
    }

    proptest! {
        /// Frames concatenados y partidos en cualquier punto salen idénticos y en orden.
        #[test]
        fn roundtrip_any_split(payloads in payloads(), cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..16)) {
            let stream: Vec<u8> = payloads.iter().flat_map(|p| encode_frame(p).unwrap()).collect();
            let mut points: Vec<usize> = cuts.iter().map(|i| i.index(stream.len() + 1)).collect();
            points.push(0);
            points.push(stream.len());
            points.sort_unstable();
            let mut dec = FrameDecoder::new();
            let mut out = Vec::new();
            for w in points.windows(2) {
                dec.push(&stream[w[0]..w[1]]);
                while let Some(f) = dec.next_frame().unwrap() {
                    out.push(f);
                }
            }
            prop_assert_eq!(out, payloads);
            prop_assert_eq!(dec.pending(), 0);
        }

        /// Un stream truncado nunca produce un frame falso: lo completo sale, el resto queda pendiente.
        #[test]
        fn truncated_stream_never_yields_partial_frame(payloads in payloads(), cut in any::<prop::sample::Index>()) {
            let stream: Vec<u8> = payloads.iter().flat_map(|p| encode_frame(p).unwrap()).collect();
            let cut = cut.index(stream.len() + 1);
            let mut dec = FrameDecoder::new();
            dec.push(&stream[..cut]);
            let mut out = Vec::new();
            while let Some(f) = dec.next_frame().unwrap() {
                out.push(f);
            }
            prop_assert!(out.len() <= payloads.len());
            prop_assert_eq!(&out[..], &payloads[..out.len()]);
            let consumed: usize = out.iter().map(|f| HEADER_LEN + f.len()).sum();
            prop_assert_eq!(dec.pending(), cut - consumed);
        }

        /// Cualquier cabecera que anuncie más de MAX_FRAME_LEN es error, sin reservar memoria.
        #[test]
        fn oversized_header_is_rejected(len in (MAX_FRAME_LEN + 1)..=u32::MAX) {
            let mut dec = FrameDecoder::new();
            dec.push(&len.to_be_bytes());
            prop_assert!(
                matches!(dec.next_frame(), Err(ProtocolError::FrameTooLarge { .. })),
                "una cabecera sobredimensionada debe dar FrameTooLarge"
            );
        }
    }

    #[test]
    fn encode_rejects_oversized_payload() {
        let big = vec![0u8; MAX_FRAME_LEN as usize + 1];
        assert!(matches!(
            encode_frame(&big),
            Err(ProtocolError::FrameTooLarge { .. })
        ));
    }

    #[tokio::test]
    async fn async_read_handles_clean_eof_and_truncation() {
        let mut stream: &[u8] = &encode_frame(b"hola").unwrap();
        assert_eq!(
            read_frame(&mut stream).await.unwrap().as_deref(),
            Some(&b"hola"[..])
        );
        assert!(read_frame(&mut stream).await.unwrap().is_none());

        let full = encode_frame(b"hola").unwrap();
        let mut truncated: &[u8] = &full[..full.len() - 1];
        assert!(matches!(
            read_frame(&mut truncated).await,
            Err(ProtocolError::Truncated)
        ));
        let mut half_header: &[u8] = &full[..2];
        assert!(matches!(
            read_frame(&mut half_header).await,
            Err(ProtocolError::Truncated)
        ));
    }

    #[tokio::test]
    async fn async_read_rejects_oversized_header() {
        let mut stream: &[u8] = &(MAX_FRAME_LEN + 1).to_be_bytes();
        assert!(matches!(
            read_frame(&mut stream).await,
            Err(ProtocolError::FrameTooLarge { .. })
        ));
    }
}
