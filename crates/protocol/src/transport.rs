//! Transporte local entre `symphony` y `symphonyd` (STACK §6.1, §47).
//!
//! - Unix: socket de archivo `0600` en `<home>/run/` (`0700`). Si esa ruta no
//!   entra en `sun_path` (104 bytes en macOS), se usa `/tmp/symphony-<hash>/`,
//!   verificando que sea un directorio propio y privado. No se usa el namespace
//!   abstracto de Linux: no tiene permisos.
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

/// FNV-1a de 64 bits: hash estable entre versiones de Rust (no como `DefaultHasher`).
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Largo máximo de la ruta del socket, con margen sobre `sun_path` (104 en macOS, 108 en Linux).
#[cfg(unix)]
const MAX_SOCKET_PATH: usize = 100;

/// Ruta del socket del daemon para este home.
#[cfg(unix)]
pub fn socket_path(home: &Path) -> PathBuf {
    let preferred = run_dir(home).join("symphonyd.sock");
    if preferred.as_os_str().len() <= MAX_SOCKET_PATH {
        return preferred;
    }
    let key = fnv1a(home.as_os_str().as_encoded_bytes());
    PathBuf::from(format!("/tmp/symphony-{key:016x}")).join("symphonyd.sock")
}

#[cfg(unix)]
fn endpoint(home: &Path) -> io::Result<Name<'static>> {
    use interprocess::local_socket::{GenericFilePath, ToFsName};
    socket_path(home).to_fs_name::<GenericFilePath>()
}

#[cfg(windows)]
fn endpoint(home: &Path) -> io::Result<Name<'static>> {
    use interprocess::local_socket::{GenericNamespaced, ToNsName};
    // Las rutas de Windows no distinguen mayúsculas.
    let key = home.to_string_lossy().to_lowercase();
    format!("symphonyd-{:016x}", fnv1a(key.as_bytes())).to_ns_name::<GenericNamespaced>()
}

/// Conecta con el daemon de este directorio de Symphony.
pub async fn connect(home: &Path) -> io::Result<Connection<LocalStream>> {
    Stream::connect(endpoint(home)?).await.map(Connection::new)
}

/// Crea (o valida) un directorio privado `0700` que pertenezca al mismo usuario
/// que `owner_ref`. En `/tmp` alguien podría haberlo creado antes que nosotros.
#[cfg(unix)]
fn private_dir(dir: &Path, owner_ref: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let meta = std::fs::symlink_metadata(dir)?;
    let expected_uid = std::fs::metadata(owner_ref)?.uid();
    if !meta.is_dir() || meta.uid() != expected_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} no es un directorio propio; no se usa para el socket",
                dir.display()
            ),
        ));
    }
    if meta.mode() & 0o077 != 0 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Crea el listener del daemon, accesible solo por el usuario actual.
/// Llamar solo con el lock de instancia tomado: en Unix reemplaza un socket viejo.
pub fn listen(home: &Path) -> io::Result<LocalListener> {
    let options = ListenerOptions::new().name(endpoint(home)?);
    #[cfg(unix)]
    let options = {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;
        let run = run_dir(home);
        std::fs::create_dir_all(&run)?;
        private_dir(&run, &run)?;
        if let Some(dir) = socket_path(home).parent().filter(|d| *d != run) {
            private_dir(dir, &run)?;
        }
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

/// Borra el socket al apagar (en Windows el pipe desaparece solo).
pub fn cleanup(home: &Path) {
    #[cfg(unix)]
    let _ = std::fs::remove_file(socket_path(home));
    #[cfg(windows)]
    let _ = home;
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

    #[cfg(unix)]
    #[test]
    fn long_homes_fall_back_to_short_socket_path() {
        let short = socket_path(Path::new("/home/leo/.symphony"));
        assert_eq!(short, Path::new("/home/leo/.symphony/run/symphonyd.sock"));
        let long_home = PathBuf::from("/var/folders").join("x".repeat(120));
        let fallback = socket_path(&long_home);
        assert!(fallback.starts_with("/tmp/symphony-"));
        assert!(fallback.as_os_str().len() <= MAX_SOCKET_PATH);
    }
}
