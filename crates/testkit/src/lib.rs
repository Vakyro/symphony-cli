//! Utilidades de prueba compartidas (STACK §42, capa L2).
//!
//! `fake-agent` imita un CLI de proveedor guiado por un [`Script`]: stream JSONL
//! en stdout, hooks reales, transcript y los errores que Symphony tiene que
//! manejar (429, cuota, auth, crash, cuelgue). CI nunca gasta suscripciones.

pub mod script;

pub use script::{Script, Step};

use std::path::PathBuf;

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
}
