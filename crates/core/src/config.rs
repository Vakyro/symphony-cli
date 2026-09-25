//! Configuración (STACK §11): `~/.symphony/config.toml` y
//! `<proyecto>/.symphony/project.toml`. Se lee con serde y se edita con
//! `toml_edit` para no perder los comentarios del usuario.
//!
//! Solo están las claves que el spec ya define; cada fase agrega las suyas.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml_edit::DocumentMut;

use crate::enums::{ContextMode, FailoverPolicy, PerformanceProfile};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("no se encontró el directorio del usuario; define SYMPHONY_HOME")]
    NoHome,
    #[error("no se pudo leer o escribir {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} no es TOML válido: {source}")]
    Parse {
        path: PathBuf,
        source: toml_edit::TomlError,
    },
    #[error("{path}: {source}")]
    Schema {
        path: PathBuf,
        source: toml_edit::de::Error,
    },
    #[error("{path}: {message}")]
    Invalid { path: PathBuf, message: String },
}

/// Directorio de estado de Symphony: `$SYMPHONY_HOME` o `~/.symphony`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymphonyHome(PathBuf);

impl SymphonyHome {
    pub fn resolve() -> Result<Self, ConfigError> {
        if let Some(dir) = std::env::var_os("SYMPHONY_HOME").filter(|v| !v.is_empty()) {
            // Absoluta: el nombre del pipe/socket del daemon se deriva de esta ruta.
            let dir = std::path::absolute(&dir).map_err(|source| ConfigError::Io {
                path: dir.into(),
                source,
            })?;
            return Ok(Self(dir));
        }
        std::env::home_dir()
            .map(|h| Self(h.join(".symphony")))
            .ok_or(ConfigError::NoHome)
    }

    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self(dir.into())
    }

    pub fn root(&self) -> &Path {
        &self.0
    }

    pub fn config_path(&self) -> PathBuf {
        self.0.join("config.toml")
    }

    /// Base de datos del daemon (DB §1). Solo `symphonyd` la abre.
    pub fn db_path(&self) -> PathBuf {
        self.0.join("symphony.db")
    }

    /// Object store (STACK §10).
    pub fn objects_dir(&self) -> PathBuf {
        self.0.join("objects")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.0.join("logs")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub performance: PerformanceConfig,
    pub routing: RoutingConfig,
    pub context: ContextConfig,
    pub providers: ProvidersConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PerformanceConfig {
    pub profile: PerformanceProfile,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RoutingConfig {
    /// Profile por defecto al crear un agente (FLOW §6: `@code`).
    pub default_profile: String,
    pub failover: FailoverPolicy,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContextConfig {
    pub mode: ContextMode,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProvidersConfig {
    /// Fracción de cuota que se reserva (DB §3.D: `reserve = 0.20`).
    pub quota_reserve: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    /// Filtro de `tracing` (`error`, `warn`, `info`, `debug`, `trace`).
    pub level: String,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            profile: PerformanceProfile::Balanced,
        }
    }
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            default_profile: "@code".into(),
            failover: FailoverPolicy::Any,
        }
    }
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            mode: ContextMode::Balanced,
        }
    }
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        Self {
            quota_reserve: 0.20,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
        }
    }
}

/// Plantilla que se escribe la primera vez. Los valores son los defaults de `Config`.
pub const DEFAULT_CONFIG: &str = r#"# Configuración de Symphony. Symphony conserva tus comentarios al editar este archivo.

[performance]
# ECO · BALANCED · PERFORMANCE · CUSTOM
profile = "BALANCED"

[routing]
# Profile o modelo por defecto al crear un agente.
default_profile = "@code"
# Qué hacer si el proveedor falla: NONE · SAME_PROVIDER · ANY
failover = "ANY"

[context]
# Cuánto se comprime el contexto en un handoff: RAW · SAFE · BALANCED · AGGRESSIVE
mode = "BALANCED"

[providers]
# Fracción de cuota que se guarda para trabajo urgente (0.0–1.0).
quota_reserve = 0.20

[logging]
# error · warn · info · debug · trace
level = "info"
"#;

impl Config {
    fn validate(&self, path: &Path) -> Result<(), ConfigError> {
        let invalid = |message: String| ConfigError::Invalid {
            path: path.to_path_buf(),
            message,
        };
        if !(0.0..=1.0).contains(&self.providers.quota_reserve) {
            return Err(invalid(format!(
                "providers.quota_reserve = {} debe estar entre 0.0 y 1.0",
                self.providers.quota_reserve
            )));
        }
        if self.routing.default_profile.trim().is_empty() {
            return Err(invalid(
                "routing.default_profile no puede estar vacío".into(),
            ));
        }
        Ok(())
    }
}

/// `[project]` en `<raíz>/.symphony/project.toml`. Las secciones opcionales
/// sobrescriben la config global para este proyecto.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub project: ProjectSection,
    #[serde(default)]
    pub routing: Option<RoutingConfig>,
    #[serde(default)]
    pub context: Option<ContextConfig>,
    #[serde(default)]
    pub performance: Option<PerformanceConfig>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSection {
    pub name: String,
    pub default_branch: String,
}

pub fn project_config_path(project_root: &Path) -> PathBuf {
    project_root.join(".symphony").join("project.toml")
}

fn read(path: &Path) -> Result<Option<String>, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ConfigError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Escritura atómica: archivo temporal en el mismo directorio y `rename`.
fn write_atomic(path: &Path, contents: &str) -> Result<(), ConfigError> {
    let io = |source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(io)?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, contents).map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}

fn parse<T: serde::de::DeserializeOwned>(path: &Path, text: &str) -> Result<T, ConfigError> {
    toml_edit::de::from_str(text).map_err(|source| ConfigError::Schema {
        path: path.to_path_buf(),
        source,
    })
}

/// Lee la config global; si no existe, la crea con la plantilla.
pub fn load_or_create(home: &SymphonyHome) -> Result<Config, ConfigError> {
    let path = home.config_path();
    let text = match read(&path)? {
        Some(t) => t,
        None => {
            write_atomic(&path, DEFAULT_CONFIG)?;
            DEFAULT_CONFIG.to_string()
        }
    };
    let config: Config = parse(&path, &text)?;
    config.validate(&path)?;
    Ok(config)
}

pub fn load_project(project_root: &Path) -> Result<Option<ProjectConfig>, ConfigError> {
    let path = project_config_path(project_root);
    read(&path)?.map(|t| parse(&path, &t)).transpose()
}

/// Decisiones del First-run (FLOW §4.2) que se guardan en `project.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectInit<'a> {
    pub name: &'a str,
    pub default_branch: &'a str,
    pub performance: PerformanceProfile,
    pub failover: FailoverPolicy,
}

/// Crea `project.toml` si no existe y devuelve la config del proyecto.
/// Si ya existe, no la pisa.
pub fn init_project(project_root: &Path, init: &ProjectInit) -> Result<ProjectConfig, ConfigError> {
    let path = project_config_path(project_root);
    if let Some(existing) = load_project(project_root)? {
        return Ok(existing);
    }
    let mut doc = DocumentMut::new();
    doc["project"]["name"] = toml_edit::value(init.name);
    doc["project"]["default_branch"] = toml_edit::value(init.default_branch);
    doc["performance"]["profile"] = toml_edit::value(init.performance.as_str());
    doc["routing"]["failover"] = toml_edit::value(init.failover.as_str());
    let text = format!("# Configuración de Symphony para este proyecto.\n{doc}");
    write_atomic(&path, &text)?;
    parse(&path, &text)
}

/// Cambia una clave (`"routing.failover"`) conservando comentarios y formato.
/// El resultado se valida con el esquema antes de escribir: una edición inválida no toca el archivo.
pub fn set_value(
    home: &SymphonyHome,
    dotted_key: &str,
    value: toml_edit::Value,
) -> Result<Config, ConfigError> {
    let path = home.config_path();
    let text = read(&path)?.unwrap_or_else(|| DEFAULT_CONFIG.to_string());
    let mut doc: DocumentMut = text.parse().map_err(|source| ConfigError::Parse {
        path: path.clone(),
        source,
    })?;
    let parts: Vec<&str> = dotted_key.split('.').collect();
    let Some((key, tables)) = parts.split_last().filter(|(k, _)| !k.is_empty()) else {
        return Err(ConfigError::Invalid {
            path,
            message: format!("clave vacía: `{dotted_key}`"),
        });
    };
    let mut item = doc.as_item_mut();
    for t in tables {
        item = &mut item[*t];
    }
    item[*key] = toml_edit::Item::Value(value);
    let new_text = doc.to_string();
    let config: Config = parse(&path, &new_text)?;
    config.validate(&path)?;
    write_atomic(&path, &new_text)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, SymphonyHome) {
        let dir = tempfile::tempdir().unwrap();
        let home = SymphonyHome::at(dir.path().join(".symphony"));
        (dir, home)
    }

    #[test]
    fn creates_default_config_on_first_load() {
        let (_d, home) = home();
        let config = load_or_create(&home).unwrap();
        assert_eq!(config, Config::default());
        assert!(home.config_path().exists());
        // La plantilla y los defaults del código no pueden divergir.
        assert_eq!(
            parse::<Config>(Path::new("x"), DEFAULT_CONFIG).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn missing_keys_take_defaults() {
        let (_d, home) = home();
        std::fs::create_dir_all(home.root()).unwrap();
        std::fs::write(home.config_path(), "[context]\nmode = \"SAFE\"\n").unwrap();
        let config = load_or_create(&home).unwrap();
        assert_eq!(config.context.mode, ContextMode::Safe);
        assert_eq!(config.routing, RoutingConfig::default());
    }

    #[test]
    fn edit_preserves_comments_and_other_keys() {
        let (_d, home) = home();
        load_or_create(&home).unwrap();
        let before = std::fs::read_to_string(home.config_path()).unwrap();
        let mut with_user_comment =
            before.replace("[routing]", "[routing]\n# mi nota: prefiero Sonnet");
        with_user_comment.push_str("\n# comentario final del usuario\n");
        std::fs::write(home.config_path(), &with_user_comment).unwrap();

        let config = set_value(&home, "routing.failover", "SAME_PROVIDER".into()).unwrap();
        assert_eq!(config.routing.failover, FailoverPolicy::SameProvider);

        let after = std::fs::read_to_string(home.config_path()).unwrap();
        assert!(after.contains("# mi nota: prefiero Sonnet"));
        assert!(after.contains("# comentario final del usuario"));
        assert!(after.contains("# Qué hacer si el proveedor falla"));
        assert!(after.contains("failover = \"SAME_PROVIDER\""));
        assert_eq!(after.lines().count(), with_user_comment.lines().count());
    }

    #[test]
    fn invalid_edit_leaves_file_untouched() {
        let (_d, home) = home();
        load_or_create(&home).unwrap();
        let before = std::fs::read_to_string(home.config_path()).unwrap();
        assert!(set_value(&home, "routing.failover", "SOMETIMES".into()).is_err());
        assert!(set_value(&home, "providers.quota_reserve", 1.5.into()).is_err());
        assert!(set_value(&home, "routing.typo_key", "x".into()).is_err());
        assert_eq!(std::fs::read_to_string(home.config_path()).unwrap(), before);
    }

    #[test]
    fn rejects_unknown_keys_and_bad_values_with_path() {
        let (_d, home) = home();
        std::fs::create_dir_all(home.root()).unwrap();
        std::fs::write(home.config_path(), "[routing]\nfailvoer = \"ANY\"\n").unwrap();
        let err = load_or_create(&home).unwrap_err().to_string();
        assert!(
            err.contains("config.toml") && err.contains("failvoer"),
            "{err}"
        );
        std::fs::write(home.config_path(), "[providers]\nquota_reserve = -0.1\n").unwrap();
        assert!(matches!(
            load_or_create(&home),
            Err(ConfigError::Invalid { .. })
        ));
    }

    #[test]
    fn project_config_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_project(dir.path()).unwrap().is_none());
        let init = ProjectInit {
            name: "arete-mobile",
            default_branch: "main",
            performance: PerformanceProfile::Eco,
            failover: FailoverPolicy::SameProvider,
        };
        let created = init_project(dir.path(), &init).unwrap();
        assert_eq!(created.project.name, "arete-mobile");
        assert_eq!(created.project.default_branch, "main");
        assert_eq!(
            created.performance.map(|p| p.profile),
            Some(PerformanceProfile::Eco)
        );
        assert_eq!(
            created.routing.map(|r| r.failover),
            Some(FailoverPolicy::SameProvider)
        );
        // Crear de nuevo no pisa lo existente.
        std::fs::write(
            project_config_path(dir.path()),
            "[project]\nname = \"otro\"\ndefault_branch = \"develop\"\n\n[context]\nmode = \"RAW\"\n",
        )
        .unwrap();
        let again = init_project(dir.path(), &init).unwrap();
        assert_eq!(again.project.name, "otro");
        assert_eq!(
            again.context,
            Some(ContextConfig {
                mode: ContextMode::Raw
            })
        );
    }

    #[test]
    fn home_paths() {
        let home = SymphonyHome::at("/tmp/x");
        assert_eq!(home.config_path(), Path::new("/tmp/x").join("config.toml"));
        assert_eq!(home.logs_dir(), Path::new("/tmp/x").join("logs"));
    }
}
