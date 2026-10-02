//! Utilidades de prueba compartidas (STACK §42, capa L2).
//!
//! `fake-agent` imita un CLI de proveedor guiado por un [`Script`]: stream JSONL
//! en stdout, hooks reales, transcript y los errores que Symphony tiene que
//! manejar (429, cuota, auth, crash, cuelgue). CI nunca gasta suscripciones.

pub mod fake_adapter;
pub mod script;

pub use fake_adapter::FakeAdapter;
pub use script::{Script, Step};

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Copia `bin` a `target/test-bins/` (con su tamaño y fecha en el nombre) y devuelve la copia.
///
/// Windows no deja reemplazar un `.exe` en ejecución: si un test corre `target/debug/symphonyd.exe`
/// y otro lo recompila, el build falla con «Acceso denegado». Cada test corre una copia fija y
/// cargo queda libre de reemplazar el original. Si no se puede copiar, devuelve `bin`.
pub fn pinned_bin(bin: &Path) -> PathBuf {
    pin(bin).unwrap_or_else(|_| bin.to_path_buf())
}

fn pin(bin: &Path) -> std::io::Result<PathBuf> {
    let not_found = || std::io::Error::from(std::io::ErrorKind::NotFound);
    let meta = std::fs::metadata(bin)?;
    let stamp = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    // `bin` vive en `target/<perfil>/`: las copias, en `target/test-bins/`.
    let dir = bin
        .parent()
        .and_then(Path::parent)
        .ok_or_else(not_found)?
        .join("test-bins");
    std::fs::create_dir_all(&dir)?;
    let stem = bin
        .file_stem()
        .ok_or_else(not_found)?
        .to_string_lossy()
        .into_owned();
    let dest = dir.join(format!(
        "{stem}-{}-{stamp}{}",
        meta.len(),
        std::env::consts::EXE_SUFFIX
    ));
    if !dest.is_file() {
        let tmp = dir.join(format!("{stem}-{}.tmp", std::process::id()));
        std::fs::copy(bin, &tmp)?;
        // Si otro proceso de test ganó la carrera, su copia sirve igual.
        if std::fs::rename(&tmp, &dest).is_err() {
            let _ = std::fs::remove_file(&tmp);
        } else if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&dest) {
            // `copy` conserva la fecha del binario: sin esto, uno viejo se podaría al instante.
            let _ = f.set_modified(SystemTime::now());
        }
    }
    prune_old(&dir, &dest);
    dest.is_file().then_some(dest).ok_or_else(not_found)
}

/// Borra las copias de más de un día, salvo `keep` (nunca las recientes: otro test puede estar
/// por lanzarlas).
fn prune_old(dir: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age > Duration::from_secs(24 * 3600));
        if old && e.path() != keep {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Ruta del binario `fake-agent` junto al ejecutable del test que lo llama
/// (`target/<perfil>/`). Lo construye cargo al compilar los tests del workspace.
pub fn fake_agent_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = format!("fake-agent{}", std::env::consts::EXE_SUFFIX);
    // Los tests viven en target/<perfil>/deps/; los binarios, un nivel arriba.
    exe.ancestors()
        .skip(1)
        .take(2)
        .map(|d| d.join(&name))
        .find(|p| p.is_file())
        .map(|p| pinned_bin(&p))
}
