//! Catálogo de skills y comandos de cada CLI, para el autocompletado del chat (P07.5.S9).
//!
//! Se lee del disco (no hace falta una sesión viva): las skills de usuario, las de plugins
//! y las del proyecto. Claude las invoca con `/nombre` y Codex con `$nombre`.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// Como se escribe tras el prefijo (`flintstone`, `ponytail:ponytail-review`).
    pub name: String,
    pub description: String,
    /// `proyecto`, `usuario` o `plugin`.
    pub source: &'static str,
}

impl Skill {
    pub fn to_json(&self) -> Value {
        json!({ "name": self.name, "description": self.description, "source": self.source })
    }
}

/// Nombre y descripción del encabezado YAML (`---` … `---`) de un `SKILL.md` o comando.
/// La descripción puede ser de una línea o plegada (`>` / `|`) en varias.
fn front_matter(text: &str) -> (Option<String>, Option<String>) {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (None, None);
    }
    let (mut name, mut desc): (Option<String>, Option<String>) = (None, None);
    let mut collecting = false;
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if collecting && (line.starts_with(' ') || line.starts_with('\t')) {
            let d = desc.get_or_insert_with(String::new);
            if !d.is_empty() {
                d.push(' ');
            }
            d.push_str(line.trim());
            continue;
        }
        collecting = false;
        if let Some(v) = line.strip_prefix("name:") {
            name = Some(v.trim().trim_matches(['"', '\'']).to_string());
        } else if let Some(v) = line.strip_prefix("description:") {
            let v = v.trim();
            if matches!(v, ">" | "|" | ">-" | "|-") {
                collecting = true;
                desc = Some(String::new());
            } else {
                desc = Some(v.trim_matches(['"', '\'']).to_string());
            }
        }
    }
    (name.filter(|n| !n.is_empty()), desc)
}

fn short(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= 140 {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(139).collect::<String>())
    }
}

fn push(out: &mut Vec<Skill>, prefix: Option<&str>, base: &str, file: &Path, source: &'static str) {
    let Ok(text) = fs::read_to_string(file) else {
        return;
    };
    let (name, desc) = front_matter(&text);
    let name = name.unwrap_or_else(|| base.to_string());
    let name = match prefix {
        Some(p) => format!("{p}:{name}"),
        None => name,
    };
    out.push(Skill {
        name,
        description: short(&desc.unwrap_or_default()),
        source,
    });
}

/// `dir/<nombre>/SKILL.md`.
fn scan_skills(dir: &Path, prefix: Option<&str>, source: &'static str, out: &mut Vec<Skill>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        // `metadata` sigue enlaces simbólicos (las skills de Codex suelen serlo).
        if fs::metadata(e.path()).is_ok_and(|m| m.is_dir()) {
            let base = e.file_name().to_string_lossy().into_owned();
            push(out, prefix, &base, &e.path().join("SKILL.md"), source);
        }
    }
}

/// `dir/*.md` (comandos personalizados de Claude).
fn scan_commands(dir: &Path, prefix: Option<&str>, source: &'static str, out: &mut Vec<Skill>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "md") {
            let base = p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            push(out, prefix, &base, &p, source);
        }
    }
}

/// Subcarpetas de `dir`, ordenadas.
fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| fs::metadata(p).is_ok_and(|m| m.is_dir()))
        .collect();
    v.sort();
    v
}

/// Skills de plugins: `cache/<mercado>/<plugin>/<versión>/{skills,commands}`; de cada plugin,
/// solo la última versión (por orden de nombre).
// ponytail: «última versión» por orden alfabético (4.10 < 4.9); basta para elegir una copia.
fn scan_plugins(cache: &Path, commands: bool, out: &mut Vec<Skill>) {
    for market in subdirs(cache) {
        for plugin in subdirs(&market) {
            let Some(version) = subdirs(&plugin).pop() else {
                continue;
            };
            let name = plugin
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            scan_skills(&version.join("skills"), Some(&name), "plugin", out);
            if commands {
                scan_commands(&version.join("commands"), Some(&name), "plugin", out);
            }
        }
    }
}

/// Catálogo del proveedor (`anthropic` u `openai`), sin repetidos y ordenado por nombre.
/// Ante un nombre repetido gana el del proyecto, luego el de usuario, luego el de plugin.
pub fn catalog(provider: &str, home: &Path, project: Option<&Path>) -> Vec<Skill> {
    let mut all = Vec::new();
    match provider {
        "anthropic" => {
            if let Some(p) = project {
                scan_skills(&p.join(".claude/skills"), None, "proyecto", &mut all);
                scan_commands(&p.join(".claude/commands"), None, "proyecto", &mut all);
            }
            scan_skills(&home.join(".claude/skills"), None, "usuario", &mut all);
            scan_commands(&home.join(".claude/commands"), None, "usuario", &mut all);
            scan_plugins(&home.join(".claude/plugins/cache"), true, &mut all);
        }
        "openai" => {
            if let Some(p) = project {
                scan_skills(&p.join(".agents/skills"), None, "proyecto", &mut all);
                scan_skills(&p.join(".codex/skills"), None, "proyecto", &mut all);
            }
            scan_skills(&home.join(".codex/skills"), None, "usuario", &mut all);
            scan_skills(&home.join(".agents/skills"), None, "usuario", &mut all);
            scan_plugins(&home.join(".codex/plugins/cache"), false, &mut all);
        }
        _ => {}
    }
    let mut seen = std::collections::HashSet::new();
    all.retain(|s| seen.insert(s.name.clone()));
    all.sort_by_key(|s| s.name.to_lowercase());
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, text: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    #[test]
    fn front_matter_reads_one_line_and_folded_descriptions() {
        let one = "---\nname: uno\ndescription: Hace una cosa.\n---\ncuerpo";
        assert_eq!(
            front_matter(one),
            (Some("uno".into()), Some("Hace una cosa.".into()))
        );
        let folded =
            "---\nname: dos\ndescription: >\n  Primera parte\n  segunda parte.\nother: x\n---\n";
        assert_eq!(
            front_matter(folded),
            (
                Some("dos".into()),
                Some("Primera parte segunda parte.".into())
            )
        );
        assert_eq!(front_matter("sin encabezado"), (None, None));
    }

    #[test]
    fn claude_catalog_merges_project_user_and_plugin_without_repeats() {
        let dir = tempfile::tempdir().unwrap();
        let (home, proj) = (dir.path().join("home"), dir.path().join("proj"));
        write(
            &home.join(".claude/skills/flintstone/SKILL.md"),
            "---\nname: flintstone\ndescription: Modo breve\n---\n",
        );
        write(
            &home.join(".claude/skills/solo-usuario/SKILL.md"),
            "---\ndescription: X\n---\n",
        );
        write(
            &proj.join(".claude/skills/flintstone/SKILL.md"),
            "---\nname: flintstone\ndescription: del proyecto\n---\n",
        );
        write(
            &home.join(".claude/commands/deploy.md"),
            "---\ndescription: Despliega\n---\n",
        );
        // Dos versiones de un plugin: solo cuenta la última.
        for v in ["1.0.0", "2.0.0"] {
            write(
                &home.join(format!(
                    ".claude/plugins/cache/mkt/ponytail/{v}/skills/review/SKILL.md"
                )),
                &format!("---\nname: review\ndescription: v{v}\n---\n"),
            );
        }
        let names: Vec<_> = catalog("anthropic", &home, Some(&proj))
            .into_iter()
            .map(|s| (s.name, s.description, s.source))
            .collect();
        assert_eq!(
            names,
            vec![
                ("deploy".into(), "Despliega".into(), "usuario"),
                ("flintstone".into(), "del proyecto".into(), "proyecto"),
                ("ponytail:review".into(), "v2.0.0".into(), "plugin"),
                ("solo-usuario".into(), "X".into(), "usuario"),
            ]
        );
    }

    #[test]
    fn codex_catalog_reads_its_own_dirs_and_unknown_providers_are_empty() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        write(
            &home.join(".codex/skills/deslop/SKILL.md"),
            "---\nname: deslop\ndescription: Limpia\n---\n",
        );
        write(
            &home.join(".agents/skills/otra/SKILL.md"),
            "---\ndescription: Otra\n---\n",
        );
        write(
            &home.join(".claude/skills/solo-claude/SKILL.md"),
            "---\ndescription: no\n---\n",
        );
        let names: Vec<_> = catalog("openai", home, None)
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["deslop", "otra"]);
        assert!(catalog("otro", home, None).is_empty());
    }
}
