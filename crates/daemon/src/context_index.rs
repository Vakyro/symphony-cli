//! Indexado de objetos de contexto (P09.S2/S3): trocea el texto, lo deja buscable con FTS5/BM25 y,
//! si es largo, guarda su versión comprimida. El original no se toca: sigue en su blob,
//! recuperable por su `ctx://`.

use rusqlite::Connection;
use symphony_context::compress::{Hint, compress};
use symphony_core::ContextObjectId;
use symphony_store::context::{self, NewChunk};
use symphony_store::repo::RepoError;

/// Desde cuántos bytes vale la pena guardar una versión comprimida.
const COMPRESS_FROM_BYTES: usize = 2_000;

/// Trocea `text`, reemplaza los chunks del objeto y guarda su versión comprimida si ahorra algo.
pub fn index_text(
    conn: &Connection,
    object: ContextObjectId,
    text: &str,
    hint: Hint,
) -> Result<(), RepoError> {
    let chunks: Vec<NewChunk> = symphony_context::chunk::chunk_text(text)
        .into_iter()
        .map(|c| NewChunk {
            seq: i64::try_from(c.seq).unwrap_or(i64::MAX),
            start_line: i64::try_from(c.start_line).unwrap_or(i64::MAX),
            end_line: i64::try_from(c.end_line).unwrap_or(i64::MAX),
            text: c.text,
        })
        .collect();
    context::replace_chunks(conn, object, &chunks)?;
    if text.len() >= COMPRESS_FROM_BYTES {
        let c = compress(text, hint);
        if c.compressor != symphony_context::compress::Compressor::None {
            context::set_compression(
                conn,
                object,
                &c.text,
                c.compressor.db_label(),
                c.tokens_original,
                c.tokens_compressed,
            )?;
        }
    }
    Ok(())
}
