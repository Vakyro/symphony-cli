//! `plugin.toml` (ADR-0015): qué es un plugin y cómo se ejecuta. Validación estricta: lo que no
//! se entiende se rechaza, nunca se interpreta «a medias» (STACK §46).

use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

/// Versiones del protocolo que este host entiende.
pub const SUPPORTED_API: u32 = 1;
/// Tiempo máximo por llamada si el manifiesto no dice otro.
pub const DEFAULT_TIMEOUT_SECS: u64 = 10;
/// Tope del tiempo máximo que un manifiesto puede pedir.
pub const MAX_TIMEOUT_SECS: u64 = 300;

/// Qué hace un plugin (IDEA §5.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Validator,
    ContextTransformer,
    Tool,
    ProviderAdapter,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Validator => "validator",
            Self::ContextTransformer => "context-transformer",
            Self::Tool => "tool",
            Self::ProviderAdapter => "provider-adapter",
        }
    }

    /// ¿El host sabe ejecutarlo en esta versión? (`tool` y `provider-adapter`: v1.1.)
    pub fn supported(self) -> bool {
        matches!(self, Self::Validator | Self::ContextTransformer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    name: String,
    version: String,
    plugin_api: u32,
    kind: Kind,
    exec: Vec<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

/// Un manifiesto ya validado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub plugin_api: u32,
    pub kind: Kind,
    /// Programa y argumentos (sin shell).
    pub exec: Vec<String>,
    pub description: String,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    #[error("plugin.toml no es válido: {0}")]
    Parse(String),
    #[error("`name` tiene que ser [a-z0-9-] y tener entre 1 y 40 caracteres: `{0}`")]
    Name(String),
    #[error("`version` está vacía")]
    Version,
    #[error("`plugin_api = {0}` no está soportada (este Symphony entiende {SUPPORTED_API})")]
    Api(u32),
    #[error("`exec` está vacío")]
    ExecEmpty,
    #[error(
        "`exec[0]` = `{0}`: tiene que ser un archivo de la carpeta del plugin (`./x`) o un nombre \
         del PATH, sin ruta absoluta ni `..`"
    )]
    ExecPath(String),
    #[error("`timeout_secs = {0}` fuera de 1–{MAX_TIMEOUT_SECS}")]
    Timeout(u64),
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let raw: Raw =
            toml_edit::de::from_str(text).map_err(|e| ManifestError::Parse(e.to_string()))?;
        let name_ok = !raw.name.is_empty()
            && raw.name.len() <= 40
            && raw
                .name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !raw.name.starts_with('-');
        if !name_ok {
            return Err(ManifestError::Name(raw.name));
        }
        if raw.version.trim().is_empty() {
            return Err(ManifestError::Version);
        }
        if raw.plugin_api != SUPPORTED_API {
            return Err(ManifestError::Api(raw.plugin_api));
        }
        let Some(program) = raw.exec.first() else {
            return Err(ManifestError::ExecEmpty);
        };
        if program.trim().is_empty() {
            return Err(ManifestError::ExecEmpty);
        }
        if !exec_path_ok(program) {
            return Err(ManifestError::ExecPath(program.clone()));
        }
        let timeout = raw.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
        if !(1..=MAX_TIMEOUT_SECS).contains(&timeout) {
            return Err(ManifestError::Timeout(timeout));
        }
        Ok(Self {
            name: raw.name,
            version: raw.version,
            plugin_api: raw.plugin_api,
            kind: raw.kind,
            exec: raw.exec,
            description: raw.description,
            timeout_secs: timeout,
        })
    }

    /// El programa a lanzar: un archivo del plugin (con `.exe` en Windows si hace falta) o un
    /// nombre del `PATH`.
    pub fn program(&self, dir: &Path) -> PathBuf {
        let first = &self.exec[0];
        let direct = dir.join(first);
        let exe = dir.join(format!("{first}.exe"));
        // Un archivo de la carpeta del plugin gana: `validator` y `./validator` son el mismo.
        if direct.is_file() {
            return direct;
        }
        if cfg!(windows) && exe.is_file() {
            return exe;
        }
        // Con separador se esperaba un archivo del plugin; sin él, un programa del PATH.
        if is_relative_file(first) {
            direct
        } else {
            PathBuf::from(first)
        }
    }
}

/// `./x` y `bin/x` son siempre archivos del plugin; `node` es un nombre del PATH salvo que la
/// carpeta del plugin tenga un archivo con ese nombre.
fn is_relative_file(p: &str) -> bool {
    p.contains('/') || p.contains('\\')
}

fn exec_path_ok(p: &str) -> bool {
    let path = Path::new(p);
    if path.is_absolute() || p.starts_with('/') || p.starts_with('\\') || p.contains(':') {
        return false;
    }
    !path.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = r#"
name = "validator-todo-check"
version = "0.1.0"
plugin_api = 1
kind = "validator"
exec = ["./validator-todo-check"]
description = "falla si el diff agrega TODO sin issue"
timeout_secs = 20
"#;

    #[test]
    fn a_valid_manifest_parses() {
        let m = Manifest::parse(OK).unwrap();
        assert_eq!(m.name, "validator-todo-check");
        assert_eq!(m.kind, Kind::Validator);
        assert_eq!(m.timeout_secs, 20);
        assert!(m.kind.supported());
        assert!(!Kind::Tool.supported() && !Kind::ProviderAdapter.supported());
    }

    #[test]
    fn defaults_and_bare_program_names() {
        let m = Manifest::parse(
            "name='x'\nversion='1'\nplugin_api=1\nkind='context-transformer'\nexec=['node','t.js']",
        )
        .unwrap();
        assert_eq!(m.timeout_secs, DEFAULT_TIMEOUT_SECS);
        assert_eq!(m.program(Path::new("/p")), PathBuf::from("node"));
    }

    #[test]
    fn everything_unknown_or_unsafe_is_rejected() {
        let bad = |s: &str| Manifest::parse(s).unwrap_err();
        let base = |extra: &str| {
            format!("name='x'\nversion='1'\nplugin_api=1\nkind='validator'\nexec=['./x']\n{extra}")
        };
        assert!(matches!(
            bad(&OK.replace("plugin_api = 1", "plugin_api = 2")),
            ManifestError::Api(2)
        ));
        assert!(matches!(
            bad(&OK.replace("validator-todo-check\"\nversion", "Mal Nombre\"\nversion")),
            ManifestError::Name(_)
        ));
        assert!(matches!(bad(&base("extra = 1")), ManifestError::Parse(_)));
        assert!(matches!(
            bad(&OK.replace("kind = \"validator\"", "kind = \"magia\"")),
            ManifestError::Parse(_)
        ));
        for exec in [
            "['/usr/bin/x']",
            "['../x']",
            "['a/../../x']",
            "['C:/x.exe']",
            "['\\\\x']",
            "[]",
            "['']",
        ] {
            let err = bad(&format!(
                "name='x'\nversion='1'\nplugin_api=1\nkind='validator'\nexec={exec}"
            ));
            assert!(
                matches!(err, ManifestError::ExecPath(_) | ManifestError::ExecEmpty),
                "{exec}: {err}"
            );
        }
        assert!(matches!(
            bad(&base("timeout_secs = 0")),
            ManifestError::Timeout(0)
        ));
        assert!(matches!(
            bad(&base("timeout_secs = 301")),
            ManifestError::Timeout(301)
        ));
    }

    #[test]
    fn a_bare_name_that_exists_in_the_plugin_dir_is_the_plugin_file() {
        let dir = std::env::temp_dir().join(format!("sym-plugin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("check.py"), "x").unwrap();
        let m = Manifest::parse(
            "name='x'\nversion='1'\nplugin_api=1\nkind='validator'\nexec=['check.py']",
        )
        .unwrap();
        assert_eq!(m.program(&dir), dir.join("check.py"));
        let other =
            Manifest::parse("name='x'\nversion='1'\nplugin_api=1\nkind='validator'\nexec=['node']")
                .unwrap();
        assert_eq!(other.program(&dir), PathBuf::from("node"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_relative_file_resolves_inside_the_plugin_dir() {
        let m = Manifest::parse(OK).unwrap();
        let dir = Path::new("/plugins/p");
        assert_eq!(m.program(dir), dir.join("./validator-todo-check"));
    }
}
