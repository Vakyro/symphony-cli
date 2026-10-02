//! `pinned_bin`: la copia fija de un binario de test (ver `symphony_testkit::pinned_bin`).
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use symphony_testkit::pinned_bin;

#[test]
fn copies_once_and_leaves_the_original_free() {
    let dir = tempfile::tempdir().unwrap();
    let debug = dir.path().join("debug");
    std::fs::create_dir_all(&debug).unwrap();
    let bin = debug.join("tool.exe");
    std::fs::write(&bin, b"v1").unwrap();

    let a = pinned_bin(&bin);
    let b = pinned_bin(&bin);
    assert_eq!(a, b, "la misma versión comparte copia");
    assert_ne!(a, bin);
    assert_eq!(a.parent().unwrap(), dir.path().join("test-bins"));
    assert_eq!(std::fs::read(&a).unwrap(), b"v1");

    // El original se puede reemplazar aunque la copia siga en uso.
    std::fs::write(&bin, b"version 2").unwrap();
    let c = pinned_bin(&bin);
    assert_ne!(c, a, "otra versión, otra copia");
    assert_eq!(std::fs::read(&c).unwrap(), b"version 2");
    assert_eq!(std::fs::read(&a).unwrap(), b"v1");
}

#[test]
fn falls_back_to_the_original_when_it_cannot_copy() {
    let missing = std::env::temp_dir().join("no-existe").join("x.exe");
    assert_eq!(pinned_bin(&missing), missing);
}

#[test]
fn a_binary_not_rebuilt_for_days_is_still_pinned() {
    let dir = tempfile::tempdir().unwrap();
    let debug = dir.path().join("debug");
    std::fs::create_dir_all(&debug).unwrap();
    let bin = debug.join("tool.exe");
    std::fs::write(&bin, b"viejo").unwrap();
    let three_days = std::time::Duration::from_secs(3 * 24 * 3600);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&bin)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - three_days)
        .unwrap();

    let pinned = pinned_bin(&bin);
    assert_ne!(
        pinned, bin,
        "la copia no debe podarse por la fecha del original"
    );
    assert!(pinned.is_file());
    assert_eq!(pinned_bin(&bin), pinned);
}

#[test]
fn concurrent_callers_share_one_copy() {
    let dir = tempfile::tempdir().unwrap();
    let debug = dir.path().join("debug");
    std::fs::create_dir_all(&debug).unwrap();
    let bin = debug.join("tool.exe");
    std::fs::write(&bin, vec![7u8; 2_000_000]).unwrap();

    let paths: Vec<_> = (0..8)
        .map(|_| {
            let bin = bin.clone();
            std::thread::spawn(move || pinned_bin(&bin))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    assert!(paths.iter().all(|p| p == &paths[0]), "{paths:?}");
    assert_eq!(std::fs::read(&paths[0]).unwrap().len(), 2_000_000);
    let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("test-bins"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".lock") || n.ends_with(".tmp"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "sin restos de la copia: {leftovers:?}"
    );
}
