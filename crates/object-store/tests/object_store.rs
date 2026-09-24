//! P03.S4: dedupe, atomicidad ante crash, roundtrip, integridad y GC.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::time::Duration;

use symphony_object_store::{ObjectError, ObjectStore};

fn setup() -> (tempfile::TempDir, ObjectStore, rusqlite::Connection) {
    let dir = tempfile::tempdir().unwrap();
    let store = ObjectStore::new(dir.path().join("objects"));
    let conn = symphony_store::open_in_memory().unwrap();
    (dir, store, conn)
}

fn files_under(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for shard in std::fs::read_dir(root).unwrap().flatten() {
        for f in std::fs::read_dir(shard.path()).unwrap().flatten() {
            out.push(f.path());
        }
    }
    out
}

#[test]
fn roundtrip_and_layout() {
    let (dir, store, conn) = setup();
    let log = "896 passed, 1 failed\n".repeat(2_000);
    let s = store
        .put(&conn, log.as_bytes(), Some("text/plain"))
        .unwrap();
    assert!(s.created);
    assert_eq!(s.hash, blake3::hash(log.as_bytes()).to_hex().to_string());
    assert!(
        s.stored_bytes < s.size_bytes / 10,
        "zstd debería comprimir un log repetitivo: {s:?}"
    );
    let path = dir.path().join("objects").join(&s.hash[..2]).join(&s.hash);
    assert!(path.is_file());
    assert_eq!(store.get(&s.hash).unwrap(), log.as_bytes());
    let (size, ref_count): (i64, i64) = conn
        .query_row(
            "SELECT size_bytes, ref_count FROM blobs WHERE hash=?1",
            [&s.hash],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((size as usize, ref_count), (log.len(), 0));
    // Vacío también es un blob válido.
    let empty = store.put(&conn, b"", None).unwrap();
    assert_eq!(store.get(&empty.hash).unwrap(), b"");
}

#[test]
fn dedupe_two_equal_puts_one_file() {
    let (dir, store, conn) = setup();
    let a = store.put(&conn, b"same bytes", None).unwrap();
    let b = store.put(&conn, b"same bytes", None).unwrap();
    assert_eq!(a.hash, b.hash);
    assert!(a.created && !b.created);
    assert_eq!(files_under(&dir.path().join("objects")).len(), 1);
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM blobs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 1);
}

#[test]
fn crash_mid_write_leaves_no_truncated_blob() {
    let (dir, store, conn) = setup();
    let data = b"contenido completo del blob".repeat(100);
    let hash = blake3::hash(&data).to_hex().to_string();
    let shard = dir.path().join("objects").join(&hash[..2]);
    std::fs::create_dir_all(&shard).unwrap();

    // Simula el proceso muerto a media escritura: un temporal con la mitad de los bytes, nunca renombrado.
    let compressed = zstd::bulk::compress(&data, 3).unwrap();
    let mut partial = tempfile::Builder::new()
        .prefix(".tmp-")
        .tempfile_in(&shard)
        .unwrap();
    partial
        .write_all(&compressed[..compressed.len() / 2])
        .unwrap();
    let (_file, leftover) = partial.keep().unwrap();

    // El blob no existe (no hay un archivo truncado con su nombre)...
    assert!(matches!(store.get(&hash), Err(ObjectError::NotFound(_))));
    // ...y un put posterior lo escribe completo.
    store.put(&conn, &data, None).unwrap();
    assert_eq!(store.get(&hash).unwrap(), data);
    store.add_ref(&conn, &hash).unwrap();
    // El GC se lleva el temporal abandonado.
    let report = store.gc(&conn, Duration::ZERO).unwrap();
    assert_eq!(report.temp_files_removed, 1);
    assert!(!leftover.exists());
    assert_eq!(
        store.get(&hash).unwrap(),
        data,
        "el GC no toca un blob referenciado"
    );
}

#[test]
fn corruption_is_detected() {
    let (dir, store, conn) = setup();
    let s = store.put(&conn, b"original", None).unwrap();
    let path = dir.path().join("objects").join(&s.hash[..2]).join(&s.hash);
    std::fs::write(&path, zstd::bulk::compress(b"otra cosa", 3).unwrap()).unwrap();
    assert!(matches!(
        store.get(&s.hash),
        Err(ObjectError::Corrupt { .. })
    ));
    std::fs::write(&path, b"no es zstd").unwrap();
    assert!(matches!(
        store.get(&s.hash),
        Err(ObjectError::Corrupt { .. })
    ));
    assert!(matches!(
        store.get("../../etc/passwd"),
        Err(ObjectError::InvalidHash(_))
    ));
}

#[test]
fn refcount_and_gc() {
    let (dir, store, conn) = setup();
    let kept = store.put(&conn, b"referenciado", None).unwrap();
    let dropped = store.put(&conn, b"sin referencias", None).unwrap();
    store.add_ref(&conn, &kept.hash).unwrap();
    store.add_ref(&conn, &kept.hash).unwrap();
    store.release(&conn, &kept.hash).unwrap();
    // Nunca baja de 0.
    store.release(&conn, &dropped.hash).unwrap();
    assert!(matches!(
        store.add_ref(&conn, &"0".repeat(64)),
        Err(ObjectError::NotFound(_))
    ));

    // Con margen de gracia, nada recién creado se borra.
    assert_eq!(
        store
            .gc(&conn, Duration::from_secs(3600))
            .unwrap()
            .blobs_removed,
        0
    );

    // Un archivo huérfano (sin fila), como tras un crash entre el archivo y el INSERT.
    let orphan = store.write_object(b"huerfano").unwrap();

    let report = store.gc(&conn, Duration::ZERO).unwrap();
    assert_eq!(report.blobs_removed, 1);
    assert_eq!(report.orphan_files_removed, 1);
    assert!(store.get(&kept.hash).is_ok());
    assert!(matches!(
        store.get(&dropped.hash),
        Err(ObjectError::NotFound(_))
    ));
    assert!(matches!(
        store.get(&orphan.hash),
        Err(ObjectError::NotFound(_))
    ));
    assert_eq!(files_under(&dir.path().join("objects")).len(), 1);
}
