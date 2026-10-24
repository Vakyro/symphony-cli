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
    #[error(
        "{0} es tu carpeta personal, no un proyecto: abre Symphony dentro de la carpeta del          proyecto (o define SYMPHONY_HOME)"
    )]
    PersonalFolder(PathBuf),
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

/// Directorio de estado de Symphony (ADR-0018). Hay dos:
/// - el **del proyecto**, `<proyecto>/.symphony`: base de datos, objetos, worktrees, logs y el
///   socket de su daemon (un daemon por proyecto). Es lo que devuelve [`SymphonyHome::resolve`];
/// - el **del usuario**, `~/.symphony`: `config.toml`, skills, plugins y cachés
///   ([`SymphonyHome::user`]).
///
/// `$SYMPHONY_HOME` fuerza el del proyecto (y, si no hay `$SYMPHONY_USER_HOME`, también el del
/// usuario: un solo directorio para todo, como antes de P15.5; es lo que usan los tests y los
/// hooks que lanza el daemon).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymphonyHome(PathBuf);

fn absolute_env(var: &str) -> Result<Option<PathBuf>, ConfigError> {
    let Some(dir) = std::env::var_os(var).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    // Absoluta: el nombre del pipe/socket del daemon se deriva de esta ruta.
    std::path::absolute(&dir)
        .map(Some)
        .map_err(|source| ConfigError::Io {
            path: dir.into(),
            source,
        })
}

/// La raíz del proyecto que contiene `dir`: dentro de un worktree de Symphony
/// (`<raíz>/.symphony/worktrees/…`), esa raíz; si no, el ancestro más cercano con
/// `.symphony/project.toml` o con `.git`; si no hay ninguno, `dir` mismo.
pub fn project_root_of(dir: &Path) -> PathBuf {
    let in_worktree = dir.ancestors().find(|a| {
        a.file_name().is_some_and(|n| n == "worktrees")
            && a.parent()
                .is_some_and(|p| p.file_name().is_some_and(|n| n == ".symphony"))
    });
    if let Some(root) = in_worktree
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|r| !r.as_os_str().is_empty())
    {
        return root.to_path_buf();
    }
    let by = |marker: &dyn Fn(&Path) -> bool| dir.ancestors().find(|a| marker(a));
    by(&|a| project_config_path(a).is_file())
        .or_else(|| by(&|a| a.join(".git").exists()))
        .unwrap_or(dir)
        .to_path_buf()
}

impl SymphonyHome {
    /// El home del proyecto donde se trabaja: `$SYMPHONY_HOME` o el del directorio actual.
    pub fn resolve() -> Result<Self, ConfigError> {
        if let Some(dir) = absolute_env("SYMPHONY_HOME")? {
            return Ok(Self(dir));
        }
        let cwd = std::env::current_dir().map_err(|source| ConfigError::Io {
            path: PathBuf::from("."),
            source,
        })?;
        Self::for_project(&cwd)
    }

    /// El home del proyecto que contiene `dir` (`<raíz>/.symphony`).
    pub fn for_project(dir: &Path) -> Result<Self, ConfigError> {
        let root = project_root_of(dir);
        let home = Self(root.join(".symphony"));
        // La carpeta personal no es un proyecto: su `.symphony` es el home del usuario.
        if Self::user().is_ok_and(|u| u == home) {
            return Err(ConfigError::PersonalFolder(root));
        }
        Ok(home)
    }

    /// El home del usuario: `$SYMPHONY_USER_HOME`, o `$SYMPHONY_HOME` si está definido (un solo
    /// directorio), o `~/.symphony`.
    pub fn user() -> Result<Self, ConfigError> {
        if let Some(dir) = absolute_env("SYMPHONY_USER_HOME")?.or(absolute_env("SYMPHONY_HOME")?) {
            return Ok(Self(dir));
        }
        std::env::home_dir()
            .map(|h| Self(h.join(".symphony")))
            .ok_or(ConfigError::NoHome)
    }

    /// El home del usuario visto desde el daemon de `project_home`: `$SYMPHONY_USER_HOME` (que le
    /// pasa quien lo arranca) o, sin él, el mismo directorio.
    pub fn user_for(project_home: &Path) -> Self {
        match absolute_env("SYMPHONY_USER_HOME") {
            Ok(Some(dir)) => Self(dir),
            _ => Self(project_home.to_path_buf()),
        }
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

    /// Plugins instalados (`<home>/plugins/<nombre>/plugin.toml`, ADR-0015): instalación global.
    pub fn plugins_dir(&self) -> PathBuf {
        self.0.join("plugins")
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
    pub chat: ChatConfig,
    pub plan: PlanConfig,
    pub harness: HarnessConfig,
}

/// `[harness]` (ADR-0019): con qué herramientas trabajan los modelos.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HarnessConfig {
    pub mode: HarnessMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HarnessMode {
    /// Las herramientas propias de cada CLI (comportamiento de siempre).
    #[default]
    Native,
    /// Las de Symphony, iguales para todos los modelos; se apagan las nativas de los CLIs
    /// que lo permiten.
    Symphony,
}

/// `[plan]`: plan por fases y tareas de cada agente (ADR-0011, ADR-0013).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlanConfig {
    /// El agente escribe un plan por fases y Symphony abre una sesión nueva por fase.
    pub enabled: bool,
    /// `auto`: la fase siguiente arranca sola; `ask`: Symphony espera la confirmación.
    pub phase_start: PhaseStart,
    /// Una fase con menos tareas se junta con la siguiente.
    pub min_tasks_per_phase: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PhaseStart {
    Auto,
    Ask,
}

impl Default for PlanConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            phase_start: PhaseStart::Auto,
            min_tasks_per_phase: 2,
        }
    }
}

/// `[chat]`: política opcional de cambio de proveedor por uso de contexto (P07.5.S6).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChatConfig {
    /// Si el último turno del chat llegó a este número de tokens, el siguiente mensaje
    /// pasa al siguiente proveedor. `None` (por defecto) = apagado.
    pub switch_at_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PerformanceConfig {
    pub profile: PerformanceProfile,
    /// Procesos pesados (clases 3–4) simultáneos; pisa el valor del perfil.
    pub max_heavy: Option<u32>,
    /// Techo de CPU (%) por encima del cual los pesados esperan; pisa el del perfil.
    pub cpu_ceiling_pct: Option<f64>,
    /// Techo de RAM (%); pisa el del perfil.
    pub ram_ceiling_pct: Option<f64>,
    /// Máximo de segundos que un hook retiene una operación pesada en cola antes de
    /// rechazarla con su razón. Por defecto, el del proveedor (ADR-0003).
    pub hold_max_secs: Option<u64>,
    /// Núcleos que puede usar el árbol de procesos de cada agente (límite del SO). Por
    /// defecto lo da el perfil: `ECO` 1, `BALANCED` 2, `PERFORMANCE` sin límite.
    pub os_cpu_cores: Option<f64>,
    /// Memoria máxima del árbol de cada agente, en MB. Sin límite por defecto.
    pub os_max_memory_mb: Option<u64>,
    /// Procesos máximos por agente. Sin límite por defecto.
    pub os_max_processes: Option<u32>,
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
    /// Ajustes por proveedor (`[providers.limits.anthropic]`): reserva propia y, opcional, un
    /// presupuesto de tokens por ventana para estimar la cuota de CLIs que no la informan.
    pub limits: std::collections::BTreeMap<String, ProviderLimits>,
    /// `[providers.permissions]`: qué puede hacer cada CLI sin preguntar.
    pub permissions: ProviderPermissions,
}

/// `[providers.permissions]` (P11.5.S2). Desde P15.5.S2 (decisión de Leo, 2026-10-23) el valor por
/// defecto es **acceso local total**, como una terminal normal: el sandbox de Codex bloqueaba
/// pnpm, la red y la lectura de los worktrees. Para restringir, cambiar los valores aquí.
/// Kimi y OpenCode no tienen modo restringido en `--print`/`run`: no hay opción.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderPermissions {
    /// `--permission-mode` de Claude Code (`acceptEdits`, `default`, `plan`, `bypassPermissions`…).
    pub claude_permission_mode: String,
    /// `-s` de Codex: `read-only` · `workspace-write` · `danger-full-access`.
    pub codex_sandbox: String,
    /// `--dangerously-skip-permissions` de Antigravity: puede ejecutar cualquier comando.
    pub antigravity_skip_permissions: bool,
    /// `--allow-all-tools` de Copilot: puede ejecutar cualquier comando.
    pub copilot_allow_all_tools: bool,
}

impl Default for ProviderPermissions {
    fn default() -> Self {
        Self {
            claude_permission_mode: "bypassPermissions".into(),
            codex_sandbox: "danger-full-access".into(),
            antigravity_skip_permissions: true,
            copilot_allow_all_tools: true,
        }
    }
}

/// `[providers.limits.<proveedor>]`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderLimits {
    /// Reserva de este proveedor (0.0–1.0); sin valor, la global.
    pub reserve: Option<f64>,
    /// Con `window_tokens`: duración de la ventana de cuota, en horas.
    pub window_hours: Option<u64>,
    /// Tokens que el plan da por ventana (estimación; nunca se muestra como porcentaje exacto).
    pub window_tokens: Option<u64>,
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
            max_heavy: None,
            cpu_ceiling_pct: None,
            ram_ceiling_pct: None,
            hold_max_secs: None,
            os_cpu_cores: None,
            os_max_memory_mb: None,
            os_max_processes: None,
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
            limits: std::collections::BTreeMap::new(),
            permissions: ProviderPermissions::default(),
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
# Opcionales: pisan los límites del perfil.
# max_heavy = 1
# cpu_ceiling_pct = 70.0
# ram_ceiling_pct = 70.0
# hold_max_secs = 60
# Límites del SO por agente (Job Object en Windows, cgroup v2 en Linux). Sin límite de
# memoria ni de procesos por defecto; la CPU la da el perfil (ECO 1, BALANCED 2).
# os_cpu_cores = 2.0
# os_max_memory_mb = 4096
# os_max_processes = 512

[routing]
# Profile o modelo por defecto al crear un agente.
default_profile = "@code"
# Qué hacer si el proveedor falla: NONE · SAME_PROVIDER · ANY
failover = "ANY"

[plan]
# El agente planifica por fases y tareas (`.symphony/plan/<tarea>.md`, temporal) y cada fase
# corre en una sesión nueva. phase_start: "auto" (sola) o "ask" (te pregunta antes de cada fase).
enabled = true
phase_start = "auto"
min_tasks_per_phase = 2

[harness]
# Con qué herramientas trabajan los modelos (ADR-0019): "native" (las de cada CLI) o
# "symphony" (las de Symphony: leer, escribir, editar, buscar y shell, iguales para todos;
# se apagan las nativas de los CLIs que lo permiten).
mode = "native"

[context]
# Cuánto se comprime el contexto en un handoff: RAW · SAFE · BALANCED · AGGRESSIVE
mode = "BALANCED"

[providers]
# Fracción de cuota que se guarda para trabajo urgente (0.0–1.0).
quota_reserve = 0.20
# Ajustes por proveedor (opcional). La estimación de cuota solo se usa si das un presupuesto.
# [providers.limits.github]
# reserve = 0.30
# window_hours = 720
# window_tokens = 5000000

[providers.permissions]
# Qué puede hacer cada CLI sin preguntarte. Los valores de abajo son los predeterminados:
# acceso local total, como una terminal normal (red, instalar paquetes, leer cualquier carpeta).
# Para restringir, descomenta y cambia. Kimi y OpenCode no tienen modo restringido.
# claude_permission_mode = "bypassPermissions" # default · acceptEdits · plan · bypassPermissions
# codex_sandbox = "danger-full-access"         # read-only · workspace-write · danger-full-access
# antigravity_skip_permissions = true          # false: pide permiso para los comandos
# copilot_allow_all_tools = true               # false: pide permiso para los comandos

[logging]
# error · warn · info · debug · trace
level = "info"

[chat]
# Apagado por defecto. Con un valor, si el último turno del chat llegó a esa cantidad de
# tokens de contexto, el siguiente mensaje pasa al siguiente proveedor con el historial.
# switch_at_tokens = 150000
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
        for (id, l) in &self.providers.limits {
            if let Some(r) = l.reserve
                && !(0.0..=1.0).contains(&r)
            {
                return Err(invalid(format!(
                    "providers.limits.{id}.reserve = {r} debe estar entre 0.0 y 1.0"
                )));
            }
            if l.window_hours == Some(0) || l.window_tokens == Some(0) {
                return Err(invalid(format!(
                    "providers.limits.{id}: window_hours y window_tokens deben ser mayores que 0"
                )));
            }
        }
        let perm = &self.providers.permissions;
        // Forma, no lista: un modo nuevo de Claude Code no debe rechazarse; pero nada que pueda
        // leerse como otra opción de la línea de comandos.
        let mode = &perm.claude_permission_mode;
        if mode.is_empty() || !mode.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(invalid(format!(
                "providers.permissions.claude_permission_mode = {mode:?} debe ser una palabra \
                 (acceptEdits, default, plan, bypassPermissions…)"
            )));
        }
        if !["read-only", "workspace-write", "danger-full-access"]
            .contains(&perm.codex_sandbox.as_str())
        {
            return Err(invalid(format!(
                "providers.permissions.codex_sandbox = {:?} debe ser read-only, workspace-write \
                 o danger-full-access",
                perm.codex_sandbox
            )));
        }
        let perf = &self.performance;
        if perf.max_heavy == Some(0) {
            return Err(invalid("performance.max_heavy debe ser mayor que 0".into()));
        }
        if perf.os_cpu_cores.is_some_and(|c| c.is_nan() || c <= 0.0) {
            return Err(invalid(
                "performance.os_cpu_cores debe ser mayor que 0".into(),
            ));
        }
        if perf.os_max_memory_mb == Some(0) || perf.os_max_processes == Some(0) {
            return Err(invalid(
                "performance.os_max_memory_mb y os_max_processes deben ser mayores que 0".into(),
            ));
        }
        if perf.hold_max_secs == Some(0) {
            return Err(invalid(
                "performance.hold_max_secs debe ser mayor que 0".into(),
            ));
        }
        for (name, v) in [
            ("cpu_ceiling_pct", perf.cpu_ceiling_pct),
            ("ram_ceiling_pct", perf.ram_ceiling_pct),
        ] {
            if let Some(v) = v
                && !(1.0..=100.0).contains(&v)
            {
                return Err(invalid(format!(
                    "performance.{name} = {v} debe estar entre 1 y 100"
                )));
            }
        }
        if self.plan.min_tasks_per_phase == 0 {
            return Err(invalid(
                "plan.min_tasks_per_phase debe ser mayor que 0".into(),
            ));
        }
        if self.chat.switch_at_tokens == Some(0) {
            return Err(invalid("chat.switch_at_tokens debe ser mayor que 0".into()));
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
    /// Comandos de validación por tier (STACK §23) y gate de la tarea (P08.S6).
    #[serde(default)]
    pub validation: Option<ValidationConfig>,
    /// Preparación del worktree de cada agente (P08.S6, F8).
    #[serde(default)]
    pub worktree: Option<WorktreeConfig>,
    /// Política de integración (P08.S7).
    #[serde(default)]
    pub merge: Option<MergeConfig>,
    /// Plugins instalados que este proyecto activa (P12, ADR-0015).
    #[serde(default)]
    pub plugins: Option<PluginsConfig>,
}

/// `[plugins]` de `project.toml`: nombres de plugins ya instalados en `~/.symphony/plugins`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginsConfig {
    /// Validators: corren en el tier 2 de la validación.
    pub validators: Vec<String>,
    /// Context transformers: se aplican al handoff, en este orden.
    pub context_transformers: Vec<String>,
}

impl ProjectConfig {
    /// La validación efectiva: la del proyecto o, si solo activa validators de plugin, una
    /// por defecto (gate en el tier 2). `None` si no hay nada que validar.
    pub fn effective_validation(&self) -> Option<ValidationConfig> {
        let own = self.validation.clone().filter(|v| !v.is_empty());
        let has_plugins = self
            .plugins
            .as_ref()
            .is_some_and(|p| !p.validators.is_empty());
        match (own, has_plugins) {
            (Some(v), _) => Some(v),
            (None, true) => Some(self.validation.clone().unwrap_or_default()),
            (None, false) => None,
        }
    }
}

/// `[merge]` de `project.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MergeConfig {
    /// Integrar a la rama del chat exige aprobación del usuario (por defecto sí:
    /// «nunca merge sin política explícita», PLAN P08.S7).
    pub require_approval: bool,
    /// Al completarse, el agente deja su trabajo en un commit de su rama (F7): sin commit no
    /// hay nada que integrar. Por defecto sí.
    pub commit_on_complete: bool,
}

impl Default for MergeConfig {
    fn default() -> Self {
        Self {
            require_approval: true,
            commit_on_complete: true,
        }
    }
}

/// `[validation]` de `project.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ValidationConfig {
    /// Barato y rápido: lint, formato, sintaxis.
    pub tier1: Vec<String>,
    /// Typecheck y pruebas dirigidas.
    pub tier2: Vec<String>,
    /// Suite completa y build.
    pub tier3: Vec<String>,
    /// Hasta qué tier tiene que pasar una tarea para completarse (0 = sin gate). Los tiers
    /// por encima del gate corren en el merge y en el gate del milestone.
    pub gate: u8,
    /// Cuántas validaciones fallidas se le devuelven al agente antes de bloquear la tarea.
    pub max_attempts: u32,
    /// Segundos máximos de cada comando.
    pub timeout_secs: u64,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            tier1: Vec::new(),
            tier2: Vec::new(),
            tier3: Vec::new(),
            gate: 2,
            max_attempts: 3,
            timeout_secs: 900,
        }
    }
}

impl ValidationConfig {
    /// Los comandos del tier 1–3 (vacío para cualquier otro).
    pub fn tier(&self, n: u8) -> &[String] {
        match n {
            1 => &self.tier1,
            2 => &self.tier2,
            3 => &self.tier3,
            _ => &[],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tier1.is_empty() && self.tier2.is_empty() && self.tier3.is_empty()
    }
}

/// `[worktree]` de `project.toml`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorktreeConfig {
    /// Comandos que corren en el worktree nuevo antes de lanzar al agente (`pnpm install`).
    pub prepare: Vec<String>,
    /// Segundos máximos de cada comando.
    pub prepare_timeout_secs: Option<u64>,
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
    fn harness_mode_defaults_to_native_and_rejects_unknown_values() {
        let (_d, home) = home();
        std::fs::create_dir_all(home.root()).unwrap();
        // El archivo creado con la plantilla trae el modo nativo.
        assert_eq!(
            load_or_create(&home).unwrap().harness.mode,
            HarnessMode::Native
        );
        std::fs::write(home.config_path(), "[harness]\nmode = \"symphony\"\n").unwrap();
        assert_eq!(
            load_or_create(&home).unwrap().harness.mode,
            HarnessMode::Symphony
        );
        std::fs::write(home.config_path(), "[harness]\nmode = \"otro\"\n").unwrap();
        let err = load_or_create(&home).unwrap_err().to_string();
        assert!(err.contains("otro") || err.contains("mode"), "{err}");
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

    /// P11.5.S2: `[providers.permissions]` — valores predeterminados, cambios y rechazos.
    #[test]
    fn provider_permissions_default_to_the_restrictive_values_and_validate() {
        let (_d, home) = home();
        std::fs::create_dir_all(home.root()).unwrap();
        let write = |t: &str| std::fs::write(home.config_path(), t).unwrap();

        // Ausente = los valores de siempre.
        write(
            "[providers]
quota_reserve = 0.1
",
        );
        let p = load_or_create(&home).unwrap().providers.permissions;
        assert_eq!(p, ProviderPermissions::default());
        assert_eq!(p.claude_permission_mode, "bypassPermissions");
        assert_eq!(p.codex_sandbox, "danger-full-access");
        assert!(p.antigravity_skip_permissions && p.copilot_allow_all_tools);

        // Parcial: lo no dicho conserva el predeterminado.
        write(
            "[providers.permissions]
codex_sandbox = \"read-only\"
copilot_allow_all_tools = true
",
        );
        let p = load_or_create(&home).unwrap().providers.permissions;
        assert_eq!(p.codex_sandbox, "read-only");
        assert!(p.copilot_allow_all_tools);
        assert_eq!(p.claude_permission_mode, "bypassPermissions");

        // Inválidos: el error nombra la clave.
        for (text, key) in [
            (
                "[providers.permissions]
codex_sandbox = \"todo\"
",
                "codex_sandbox",
            ),
            (
                "[providers.permissions]
claude_permission_mode = \"\"
",
                "claude_permission_mode",
            ),
            (
                "[providers.permissions]
claude_permission_mode = \"--dangerously-skip-permissions\"
",
                "claude_permission_mode",
            ),
            (
                "[providers.permissions]
codex_sandbx = \"read-only\"
",
                "codex_sandbx",
            ),
            (
                "[providers.permissions]
antigravity_skip_permissions = \"si\"
",
                "antigravity_skip_permissions",
            ),
        ] {
            write(text);
            let err = load_or_create(&home).unwrap_err().to_string();
            assert!(err.contains(key), "{key}: {err}");
        }
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
    fn project_root_is_found_from_inside_the_project_or_its_worktrees() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("app");
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        // Con `.git`: la raíz del repo.
        assert_eq!(project_root_of(&root.join("src/deep")), root);
        // Con `project.toml`: manda sobre un `.git` más cercano (un repo anidado).
        std::fs::create_dir_all(root.join("sub/.git")).unwrap();
        std::fs::create_dir_all(root.join(".symphony")).unwrap();
        std::fs::write(project_config_path(&root), "").unwrap();
        assert_eq!(project_root_of(&root.join("sub")), root);
        // Dentro de un worktree de Symphony (que tiene su propio `.git`): el proyecto dueño.
        let wt = root.join(".symphony/worktrees/p1/agent-001");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: x").unwrap();
        assert_eq!(project_root_of(&wt), root);
        // Sin nada: el propio directorio.
        let loose = d.path().join("suelta");
        std::fs::create_dir_all(&loose).unwrap();
        assert_eq!(project_root_of(&loose), loose);
    }

    #[test]
    fn home_paths() {
        let home = SymphonyHome::at("/tmp/x");
        assert_eq!(home.config_path(), Path::new("/tmp/x").join("config.toml"));
        assert_eq!(home.logs_dir(), Path::new("/tmp/x").join("logs"));
    }
}
