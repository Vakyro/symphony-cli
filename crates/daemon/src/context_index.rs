//! Indexado de objetos de contexto (P09.S2): trocea el texto y lo deja buscable con FTS5/BM25.
//! El original no se toca: sigue en su blob, recuperable por su `ctx://`.

use rusqlite::Connection;
use symphony_core::ContextObjectId;
use symphony_store::context::{self, NewChunk};
use symphony_store::repo::RepoError;

/// Trocea `text` y reemplaza los chunks del objeto.
pub fn index_text(conn: &Connection, object: ContextObjectId, text: &str) -> Result<(), RepoError> {
    let chunks: Vec<NewChunk> = symphony_context::chunk::chunk_text(text)
        .into_iter()
        .map(|c| NewChunk {
            seq: i64::try_from(c.seq).unwrap_or(i64::MAX),
            start_line: i64::try_from(c.start_line).unwrap_or(i64::MAX),
            end_line: i64::try_from(c.end_line).unwrap_or(i64::MAX),
            text: c.text,
        })
        .collect();
    context::replace_chunks(conn, object, &chunks)
}
