//! Transporte local entre `symphony` y `symphonyd` (STACK §6.1, §47).
//!
//! - Unix: socket en `<home>/run/symphonyd.sock`, archivo `0600` dentro de un
//!   directorio `0700`. No se usa el namespace abstracto de Linux: no tiene permisos.
//! - Windows: named pipe con un descriptor de seguridad que solo da acceso al
//!   dueño (el usuario que arrancó el daemon). Como los pipes son globales, el
//!   nombre incluye un hash del directorio de Symphony.

use std::io;
use std::path::{Path, PathBuf};

use interprocess::local_socket::tokio::{Listener, Stream};
use interprocess::local_socket::traits::tokio::Stream as _;
use interprocess::local_socket::{ListenerOptions, Name};

use crate::Connection;

pub type LocalStream = Stream;
pub type LocalListener = Listener;
/// Trait que da `accept()` a `LocalListener`.
pub use interprocess::local_socket::traits::tokio::Listener as ListenerExt;

pub fn run_dir(home: &Path) -> PathBuf {
    home.join("run")
}

#[cfg(unix)]
fn endpoint(home: &Path) -> io::Result<Name<'static>> {
    use interprocess::local_socket::{GenericFilePath, ToFsName};
    run_dir(home)
        .join("symphonyd.sock")
        .to_fs_name::<GenericFilePath>()
}

#[cfg(windows)]
fn endpoint(home: &Path) -> io::Result<Name<'static>> {
    use interprocess::local_socket::{GenericNamespaced, ToNsName};
    // Las rutas de Windows no distinguen mayúsculas.
    let key = home.to_string_lossy().to_lowercase();
    format!("symphonyd-{:016x}", fnv1a(key.as_bytes())).to_ns_name::<GenericNamespaced>()
}

/// FNV-1a de 64 bits: hash estable entre versiones de Rust (no como `DefaultHasher`).
#[cfg_attr(not(windows), allow(dead_code))]
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Conecta con el daemon de este directorio de Symphony.
pub async fn connect(home: &Path) -> io::Result<Connection<LocalStream>> {
    Stream::connect(endpoint(home)?).await.map(Connection::new)
}

/// Crea el listener del daemon, accesible solo por el usuario actual.
/// Llamar solo con el lock de instancia tomado: en Unix reemplaza un socket viejo.
pub fn listen(home: &Path) -> io::Result<LocalListener> {
    let options = ListenerOptions::new().name(endpoint(home)?);
    #[cfg(unix)]
    let options = {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let dir = run_dir(home);
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        options.mode(0o600).try_overwrite(true)
    };
    #[cfg(windows)]
    let options = {
        use interprocess::os::windows::local_socket::ListenerOptionsExt;
        use interprocess::os::windows::security_descriptor::SecurityDescriptor;
        // DACL protegida: acceso total solo para el dueño del objeto (OW = Owner Rights).
        let sd = SecurityDescriptor::deserialize(widestring::u16cstr!("D:P(A;;GA;;;OW)"))?;
        options.security_descriptor(sd)
    };
    options.create_tokio()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_is_stable() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn endpoints_differ_per_home() {
        let a = format!("{:?}", endpoint(Path::new("/tmp/home-a")).unwrap());
        let b = format!("{:?}", endpoint(Path::new("/tmp/home-b")).unwrap());
        assert_ne!(a, b);
    }
}
