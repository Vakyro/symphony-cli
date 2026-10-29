//! Indexado de objetos de contexto (P09.S2/S3): trocea el texto, lo deja buscable con FTS5/BM25 y,
//! si es largo, guarda su versión comprimida. El original no se toca: sigue en su blob,
//! recuperable por su `ctx://`.
//!
//! El trabajo pesado (trocear y comprimir) se hace en [`prepare`], **fuera** del hilo único del
//! writer de SQLite; [`apply`] solo escribe lo ya calculado dentro de la transacción (P13.S5).

use rusqlite::Connection;
use symphony_context::compress::{Compressed, Compressor, Hint, compress};
use symphony_core::ContextObjectId;
use symphony_store::context::{self, NewChunk};
use symphony_store::repo::RepoError;

/// Desde cuántos bytes vale la pena guardar una versión comprimida.
const COMPRESS_FROM_BYTES: usize = 2_000;

/// Lo calculado antes de entrar al writer: chunks y, si ahorra algo, la versión comprimida.
#[derive(Debug, Clone, Default)]
pub struct Prepared {
    chunks: Vec<NewChunk>,
    compressed: Option<Compressed>,
}

/// Trocea `text` y calcula su versión comprimida si hace falta. No toca la base.
pub fn prepare(text: &str, hint: Hint) -> Prepared {
    let chunks = chunks(text);
    let compressed = if text.len() >= COMPRESS_FROM_BYTES {
        let c = compress(text, hint);
        (c.compressor != Compressor::None).then_some(c)
    } else {
        None
    };
    Prepared { chunks, compressed }
}

/// Escribe en `object` lo que devolvió [`prepare`], dentro de la transacción del writer.
pub fn apply(
    conn: &Connection,
    object: ContextObjectId,
    prepared: Prepared,
) -> Result<(), RepoError> {
    context::replace_chunks(conn, object, &prepared.chunks)?;
    if let Some(c) = prepared.compressed {
        context::set_compression(
            conn,
            object,
            &c.text,
            c.compressor.db_label(),
            c.tokens_original,
            c.tokens_compressed,
        )?;
    }
    Ok(())
}

/// Los chunks de `text` (los archivos del proyecto no se «comprimen»).
pub fn chunks(text: &str) -> Vec<NewChunk> {
    symphony_context::chunk::chunk_text(text)
        .into_iter()
        .map(|c| NewChunk {
            seq: i64::try_from(c.seq).unwrap_or(i64::MAX),
            start_line: i64::try_from(c.start_line).unwrap_or(i64::MAX),
            end_line: i64::try_from(c.end_line).unwrap_or(i64::MAX),
            text: c.text,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_core::ContextObjectId;

    #[test]
    fn prepare_no_toca_la_base_y_apply_escribe_chunks_y_compresión() {
        // `prepare` calcula chunks y compresión sin tocar la base; `apply` los escribe.
        let conn = symphony_store::open_in_memory().unwrap();
        let object = ContextObjectId::new();
        conn.execute(
            "INSERT INTO projects (id, name, root_path, default_branch, created_at)
             VALUES ('p1', 'demo', '/repo', 'main', 0)",
            rusqlite::params![],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO blobs (hash, size_bytes, stored_bytes, created_at)
             VALUES ('h1', 1, 1, 0)",
            rusqlite::params![],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO context_objects (id, uri, project_id, kind, blob_hash, created_at)
             VALUES (?1, 'ctx://test/prepare.md', 'p1', 'FILE', 'h1', 0)",
            rusqlite::params![object.to_string()],
        )
        .unwrap();

        let texto = "línea uno de prueba\n".repeat(3000);
        let preparada = prepare(&texto, Hint::Auto);

        let antes: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM context_chunks",
                rusqlite::params![],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(antes, 0, "prepare no debe escribir chunks");

        apply(&conn, object, preparada).unwrap();

        let despues: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM context_chunks",
                rusqlite::params![],
                |r| r.get(0),
            )
            .unwrap();
        assert!(despues > 0, "apply debe escribir chunks");

        let comprimida: Option<String> = conn
            .query_row(
                "SELECT compressed_text FROM context_objects WHERE id = ?1",
                rusqlite::params![object.to_string()],
                |r| r.get(0),
            )
            .ok()
            .flatten();
        assert!(
            comprimida.is_some(),
            "texto largo debe almacenarse comprimido"
        );
    }
}
