//! Skills y servidores MCP compartidos entre proveedores (P09.S9, STACK §16, IDEA §5.12).
//!
//! La fuente de verdad son archivos, no la base:
//! - Skills canónicas: `~/.symphony/skills/<nombre>/SKILL.md` (+ `skill.toml` opcional y recursos)
//!   y `<proyecto>/.symphony/skills/<nombre>/…`. Ante un nombre repetido gana el del proyecto.
//! - Servidores MCP: `~/.symphony/mcp.toml` y `<proyecto>/.symphony/mcp.toml`.
//!
//! Cada adapter traduce lo que su CLI entiende. Hoy: Claude carga las skills como un plugin de
//! sesión (`--plugin-dir`, sin tocar el repo ni `~/.claude`); Claude y Codex registran los
//! servidores MCP por invocación. Codex no tiene un directorio de skills por invocación: se
//! queda sin traducción (ver la bitácora de P09).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};
use symphony_store::shared::{FoundMcp, FoundSkill};

/// Nombre reservado: es el broker de contexto de Symphony.
pub const RESERVED_MCP_NAME: &str = "symphony-context";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDef {
    pub name: String,
    /// `USER` o `PROJECT`.
    pub scope: &'static str,
    pub dir: PathBuf,
    pub description: String,
    /// `skill.toml`: si no está vacío, solo esos proveedores (`anthropic`, `openai`, …).
    pub providers: Vec<String>,
    pub content_hash: String,
}

impl SkillDef {
    pub fn found(&self) -> FoundSkill {
        FoundSkill {
            name: self.name.clone(),
            scope: self.scope,
            path: self.dir.display().to_string(),
            content_hash: self.content_hash.clone(),
        }
    }

    pub fn applies_to(&self, provider: &str) -> bool {
        self.providers.is_empty() || self.providers.iter().any(|p| p == provider)
    }
}

#[derive(Debug, Deserialize, Default)]
struct SkillToml {
    description: Option<String>,
    #[serde(default)]
    providers: Vec<String>,
}

/// Un nombre de carpeta seguro: sin separadores ni `..`, para no salir de la raíz al copiar.
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn description_of(skill_md: &str) -> String {
    crate::skills::front_matter(skill_md)
        .1
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn scan_dir(dir: &Path, scope: &'static str, out: &mut Vec<SkillDef>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(fs::DirEntry::file_name);
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        if !safe_name(&name) || !fs::metadata(&path).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        let Ok(md) = fs::read_to_string(path.join("SKILL.md")) else {
            continue;
        };
        let toml_text = fs::read_to_string(path.join("skill.toml")).unwrap_or_default();
        let meta: SkillToml = toml_edit::de::from_str(&toml_text).unwrap_or_default();
        let mut hasher = blake3::Hasher::new();
        hasher.update(md.as_bytes());
        hasher.update(toml_text.as_bytes());
        out.push(SkillDef {
            name,
            scope,
            description: meta.description.unwrap_or_else(|| description_of(&md)),
            providers: meta.providers,
            content_hash: hasher.finalize().to_hex().to_string(),
            dir: path,
        });
    }
}

/// Las skills de usuario (`<home>/skills`) y las del proyecto (`<proyecto>/.symphony/skills`).
pub fn scan_skills(symphony_home: &Path, project_root: Option<&Path>) -> Vec<SkillDef> {
    let mut out = Vec::new();
    scan_dir(&symphony_home.join("skills"), "USER", &mut out);
    if let Some(p) = project_root {
        scan_dir(&p.join(".symphony/skills"), "PROJECT", &mut out);
    }
    out
}

/// Las que de verdad se usan: ante un nombre repetido, la del proyecto.
pub fn effective(all: &[SkillDef]) -> Vec<&SkillDef> {
    all.iter()
        .filter(|s| {
            s.scope == "PROJECT" || !all.iter().any(|o| o.scope == "PROJECT" && o.name == s.name)
        })
        .collect()
}

/// Copia `src` en `dst` sin enlaces ni `skill.toml`; tope de profundidad y de archivos.
fn copy_tree(src: &Path, dst: &Path, depth: u32, budget: &mut u32) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for e in fs::read_dir(src)? {
        let e = e?;
        let name = e.file_name();
        let from = e.path();
        let to = dst.join(&name);
        let ft = e.file_type()?;
        if ft.is_symlink() || name == "skill.toml" {
            continue;
        }
        if ft.is_dir() && depth > 0 {
            copy_tree(&from, &to, depth - 1, budget)?;
        } else if ft.is_file() && *budget > 0 {
            *budget -= 1;
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// El plugin de sesión de Claude con las skills dadas: `<dir>/.claude-plugin/plugin.json` y
/// `<dir>/skills/<nombre>/…`. Se regenera solo si cambió algo. `None` si no hay skills.
pub fn write_claude_plugin(dir: &Path, skills: &[&SkillDef]) -> std::io::Result<Option<PathBuf>> {
    if skills.is_empty() {
        return Ok(None);
    }
    let mut hasher = blake3::Hasher::new();
    for s in skills {
        hasher.update(s.name.as_bytes());
        hasher.update(s.content_hash.as_bytes());
        hasher.update(s.dir.to_string_lossy().as_bytes());
    }
    let stamp = hasher.finalize().to_hex().to_string();
    let stamp_file = dir.join(".symphony-stamp");
    if fs::read_to_string(&stamp_file).is_ok_and(|s| s == stamp) {
        return Ok(Some(dir.to_path_buf()));
    }
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    fs::create_dir_all(dir.join(".claude-plugin"))?;
    let manifest = json!({
        "name": "symphony",
        "version": "0.1.0",
        "description": "Skills compartidas de Symphony",
    });
    fs::write(dir.join(".claude-plugin/plugin.json"), manifest.to_string())?;
    for s in skills {
        let mut budget = 200;
        copy_tree(&s.dir, &dir.join("skills").join(&s.name), 4, &mut budget)?;
    }
    fs::write(stamp_file, stamp)?;
    Ok(Some(dir.to_path_buf()))
}

// --- MCP ---------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct McpFile {
    #[serde(default)]
    servers: BTreeMap<String, McpEntry>,
}

#[derive(Debug, Deserialize)]
struct McpEntry {
    transport: Option<String>,
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    url: Option<String>,
    share_mode: Option<String>,
    enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpDef {
    pub name: String,
    pub scope: &'static str,
    pub http: bool,
    pub per_client: bool,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub url: Option<String>,
    pub enabled_default: bool,
}

impl McpDef {
    pub fn found(&self) -> FoundMcp {
        FoundMcp {
            name: self.name.clone(),
            scope: self.scope,
            transport: if self.http { "HTTP" } else { "STDIO" },
            share_mode: if self.per_client {
                "PER_CLIENT"
            } else {
                "SHARED"
            },
            command: self.command.clone(),
            url: self.url.clone(),
            enabled_default: self.enabled_default,
        }
    }

    /// Lo que cada adapter necesita para registrarlo (`SYMPHONY_MCP_EXTRA`).
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "transport": if self.http { "http" } else { "stdio" },
            "command": self.command,
            "args": self.args,
            "env": self.env,
            "url": self.url,
        })
    }
}

/// Lee un `mcp.toml`. Lo inválido se descarta y queda en `warnings`, sin tumbar el resto.
pub fn parse_mcp(text: &str, scope: &'static str, warnings: &mut Vec<String>) -> Vec<McpDef> {
    let file: McpFile = match toml_edit::de::from_str(text) {
        Ok(f) => f,
        Err(e) => {
            warnings.push(format!("mcp.toml ({scope}): {e}"));
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for (name, e) in file.servers {
        // Sin `.`: el nombre es una clave TOML en `-c mcp_servers.<nombre>.…` de Codex.
        if !safe_name(&name) || name.contains('.') || name == RESERVED_MCP_NAME {
            warnings.push(format!("mcp.toml: nombre no válido o reservado: `{name}`"));
            continue;
        }
        let http = match e.transport.as_deref().unwrap_or("stdio") {
            "stdio" => false,
            "http" => true,
            other => {
                warnings.push(format!(
                    "mcp.toml: `{name}`: transporte `{other}` no soportado"
                ));
                continue;
            }
        };
        let per_client = match e.share_mode.as_deref().unwrap_or("per_client") {
            "per_client" => true,
            // Una sola instancia compartida exige que el daemon haga de proxy MCP: sin hacer.
            "shared" => false,
            other => {
                warnings.push(format!(
                    "mcp.toml: `{name}`: share_mode `{other}` no válido"
                ));
                continue;
            }
        };
        if (http && e.url.is_none()) || (!http && e.command.is_none()) {
            warnings.push(format!(
                "mcp.toml: `{name}`: falta `{}`",
                if http { "url" } else { "command" }
            ));
            continue;
        }
        out.push(McpDef {
            name,
            scope,
            http,
            per_client,
            command: e.command,
            args: e.args,
            env: e.env,
            url: e.url,
            enabled_default: e.enabled.unwrap_or(true),
        });
    }
    out
}

/// Los servidores de usuario (`<home>/mcp.toml`) y los del proyecto; el del proyecto gana.
pub fn scan_mcp(
    symphony_home: &Path,
    project_root: Option<&Path>,
    warnings: &mut Vec<String>,
) -> Vec<McpDef> {
    let mut out = Vec::new();
    if let Ok(t) = fs::read_to_string(symphony_home.join("mcp.toml")) {
        out.extend(parse_mcp(&t, "USER", warnings));
    }
    if let Some(p) = project_root
        && let Ok(t) = fs::read_to_string(p.join(".symphony/mcp.toml"))
    {
        out.extend(parse_mcp(&t, "PROJECT", warnings));
    }
    out
}

/// Los servidores que se registran por invocación: habilitados y de instancia por cliente.
pub fn effective_mcp<'a>(all: &'a [McpDef], enabled: &dyn Fn(&McpDef) -> bool) -> Vec<&'a McpDef> {
    all.iter()
        .filter(|s| {
            s.per_client
                && enabled(s)
                && (s.scope == "PROJECT"
                    || !all.iter().any(|o| o.scope == "PROJECT" && o.name == s.name))
        })
        .collect()
}

/// Las variables que los adapters traducen para un run de `provider` (`SYMPHONY_PLUGIN_DIR` y
/// `SYMPHONY_MCP_EXTRA`). `plugin_dir` es donde se materializa el plugin de Claude; los
/// interruptores `skill_on` y `mcp_on` salen del índice (por omisión, encendido).
pub fn launch_env(
    provider: &str,
    plugin_dir: &Path,
    skills: &[SkillDef],
    servers: &[McpDef],
    skill_on: &dyn Fn(&SkillDef) -> bool,
    mcp_on: &dyn Fn(&McpDef) -> bool,
) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if provider == "anthropic" {
        let active: Vec<&SkillDef> = effective(skills)
            .into_iter()
            .filter(|s| s.applies_to(provider) && skill_on(s))
            .collect();
        match write_claude_plugin(plugin_dir, &active) {
            Ok(Some(d)) => env.push((
                symphony_adapter_common::ENV_PLUGIN_DIR.to_string(),
                d.display().to_string(),
            )),
            Ok(None) => {}
            Err(e) => tracing::warn!("no se pudo preparar el plugin de skills: {e}"),
        }
    }
    let extra: Vec<Value> = effective_mcp(servers, mcp_on)
        .iter()
        .map(|d| d.to_json())
        .collect();
    if !extra.is_empty() {
        env.push((
            symphony_adapter_common::ENV_MCP_EXTRA.to_string(),
            Value::Array(extra).to_string(),
        ));
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, text: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    #[test]
    fn project_skill_wins_and_unsafe_names_are_skipped() {
        let d = tempfile::tempdir().unwrap();
        let (home, proj) = (d.path().join("home"), d.path().join("proj"));
        write(
            &home.join("skills/revisar/SKILL.md"),
            "---\ndescription: de usuario\n---\n",
        );
        write(
            &proj.join(".symphony/skills/revisar/SKILL.md"),
            "---\ndescription: del proyecto\n---\n",
        );
        write(
            &proj.join(".symphony/skills/solo/SKILL.md"),
            "sin encabezado",
        );
        write(
            &proj.join(".symphony/skills/solo/skill.toml"),
            "description = \"desde toml\"\nproviders = [\"anthropic\"]\n",
        );
        write(&home.join("skills/con espacio/SKILL.md"), "x");
        write(&home.join("skills/sin-md/otra.txt"), "x");
        let all = scan_skills(&home, Some(&proj));
        let names: Vec<_> = all.iter().map(|s| (s.name.as_str(), s.scope)).collect();
        assert_eq!(
            names,
            [
                ("revisar", "USER"),
                ("revisar", "PROJECT"),
                ("solo", "PROJECT")
            ]
        );
        let eff = effective(&all);
        assert_eq!(eff.len(), 2);
        assert_eq!(
            eff.iter()
                .find(|s| s.name == "revisar")
                .unwrap()
                .description,
            "del proyecto"
        );
        let solo = eff.iter().find(|s| s.name == "solo").unwrap();
        assert_eq!(solo.description, "desde toml");
        assert!(solo.applies_to("anthropic") && !solo.applies_to("openai"));
    }

    #[test]
    fn claude_plugin_is_written_once_and_rewritten_on_change() {
        let d = tempfile::tempdir().unwrap();
        let (home, out) = (d.path().join("home"), d.path().join("plugin"));
        write(
            &home.join("skills/uno/SKILL.md"),
            "---\ndescription: Uno\n---\ncuerpo",
        );
        write(&home.join("skills/uno/resources/nota.txt"), "recurso");
        write(&home.join("skills/uno/skill.toml"), "description = \"x\"");
        let all = scan_skills(&home, None);
        let eff = effective(&all);
        let dir = write_claude_plugin(&out, &eff).unwrap().unwrap();
        assert!(dir.join(".claude-plugin/plugin.json").is_file());
        assert!(dir.join("skills/uno/SKILL.md").is_file());
        assert!(dir.join("skills/uno/resources/nota.txt").is_file());
        assert!(!dir.join("skills/uno/skill.toml").exists());
        let manifest: Value = serde_json::from_str(
            &fs::read_to_string(dir.join(".claude-plugin/plugin.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["name"], "symphony");

        // Sin cambios no se reescribe; con cambios sí.
        fs::write(dir.join("marca"), "x").unwrap();
        write_claude_plugin(&out, &eff).unwrap();
        assert!(dir.join("marca").exists());
        write(
            &home.join("skills/uno/SKILL.md"),
            "---\ndescription: Dos\n---\n",
        );
        let all = scan_skills(&home, None);
        write_claude_plugin(&out, &effective(&all)).unwrap();
        assert!(!dir.join("marca").exists());
        assert!(write_claude_plugin(&out, &[]).unwrap().is_none());
    }

    #[test]
    fn mcp_toml_accepts_stdio_and_http_and_reports_what_it_drops() {
        let text = r#"
[servers.fs]
command = "npx"
args = ["-y", "server-fs", "."]
env = { RAIZ = "." }

[servers.remoto]
transport = "http"
url = "https://example.test/mcp"
enabled = false

[servers.symphony-context]
command = "x"

[servers.sin-comando]
transport = "stdio"

[servers.raro]
command = "x"
share_mode = "otro"

[servers.compartido]
command = "y"
share_mode = "shared"
"#;
        let mut warnings = Vec::new();
        let defs = parse_mcp(text, "USER", &mut warnings);
        let names: Vec<_> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["compartido", "fs", "remoto"]);
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        let fs_def = defs.iter().find(|d| d.name == "fs").unwrap();
        assert_eq!(fs_def.args, ["-y", "server-fs", "."]);
        assert_eq!(fs_def.env["RAIZ"], ".");
        assert!(fs_def.per_client && fs_def.enabled_default);
        let remote = defs.iter().find(|d| d.name == "remoto").unwrap();
        assert!(remote.http && !remote.enabled_default);
        // `shared` se indexa pero no se registra por invocación.
        let eff = effective_mcp(&defs, &|d| d.enabled_default);
        assert_eq!(
            eff.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            ["fs"]
        );
        let mut w = Vec::new();
        assert!(parse_mcp("esto no es toml [", "USER", &mut w).is_empty());
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn launch_env_gives_claude_the_plugin_and_everyone_the_servers() {
        let d = tempfile::tempdir().unwrap();
        let (home, plugin) = (d.path().join("home"), d.path().join("plugin"));
        write(
            &home.join("skills/uno/SKILL.md"),
            "---
description: Uno
---
",
        );
        write(
            &home.join("skills/dos/SKILL.md"),
            "---
description: Dos
---
",
        );
        write(
            &home.join("mcp.toml"),
            "[servers.fs]
command = \"npx\"
[servers.off]
command = \"x\"
enabled = false
",
        );
        let skills = scan_skills(&home, None);
        let mut w = Vec::new();
        let servers = scan_mcp(&home, None, &mut w);
        let get = |env: &[(String, String)], k: &str| {
            env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
        };

        let claude = launch_env(
            "anthropic",
            &plugin,
            &skills,
            &servers,
            &|s| s.name != "dos",
            &|d| d.enabled_default,
        );
        let dir = PathBuf::from(get(&claude, "SYMPHONY_PLUGIN_DIR").unwrap());
        assert!(dir.join("skills/uno/SKILL.md").is_file());
        assert!(!dir.join("skills/dos").exists(), "la apagada no entra");
        let extra: Value =
            serde_json::from_str(&get(&claude, "SYMPHONY_MCP_EXTRA").unwrap()).unwrap();
        assert_eq!(extra.as_array().unwrap().len(), 1);
        assert_eq!(extra[0]["name"], "fs");
        assert_eq!(extra[0]["command"], "npx");

        // Codex no tiene plugin de sesión; los servidores sí.
        let codex = launch_env("openai", &plugin, &skills, &servers, &|_| true, &|_| true);
        assert!(get(&codex, "SYMPHONY_PLUGIN_DIR").is_none());
        let extra: Value =
            serde_json::from_str(&get(&codex, "SYMPHONY_MCP_EXTRA").unwrap()).unwrap();
        assert_eq!(extra.as_array().unwrap().len(), 2);

        // Nada que registrar: ninguna variable.
        assert!(launch_env("anthropic", &plugin, &[], &[], &|_| true, &|_| true).is_empty());
    }
}
