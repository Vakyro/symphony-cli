//! Grafo local del proyecto (P18.S4): archivos, símbolos L2 y dependencias Cargo.
//! No ejecuta herramientas externas ni resuelve dependencias por red.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use toml_edit::{DocumentMut, Item, TableLike};

pub const SUMMARY_BYTES: usize = 8_000;
const MAX_FILES: usize = 5_000;
const MAX_SOURCE_BYTES: u64 = 512 * 1024;
const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct Graph {
    pub files: Vec<File>,
    pub crates: Vec<Crate>,
    pub incomplete: bool,
}

#[derive(Debug, Serialize)]
pub struct File {
    pub path: String,
    pub crate_name: Option<String>,
    pub symbols: Vec<Symbol>,
}

#[derive(Debug, Serialize)]
pub struct Symbol {
    pub kind: String,
    pub name: String,
    pub line: usize,
}

#[derive(Debug, Serialize)]
pub struct Crate {
    pub name: String,
    pub manifest: String,
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Dependency {
    pub name: String,
    pub manifest: String,
    pub kind: String,
}

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("no se pudo leer o guardar el grafo: {0}")]
    Io(#[from] std::io::Error),
    #[error("no se pudo serializar el grafo: {0}")]
    Json(#[from] serde_json::Error),
}

/// Recorrido acotado, ordenado y sin seguir enlaces ni entrar en estado o artefactos.
pub fn build(root: &Path) -> Result<Graph, GraphError> {
    if !root.is_dir() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "raíz inexistente").into());
    }
    let mut graph = Graph {
        files: Vec::new(),
        crates: Vec::new(),
        incomplete: false,
    };
    let mut manifests = BTreeMap::new();
    let mut total = 0;
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .require_git(false)
        .follow_links(false)
        .sort_by_file_path(|a, b| a.cmp(b))
        .filter_entry(|e| {
            !matches!(
                e.file_name().to_str(),
                Some("target" | "node_modules" | ".git" | ".symphony")
            )
        })
        .build();
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                graph.incomplete = true;
                continue;
            }
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if graph.files.len() == MAX_FILES {
            graph.incomplete = true;
            break;
        }
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let mut symbols = Vec::new();
        if crate::ast::Lang::for_path(&rel).is_some()
            || path.file_name().is_some_and(|n| n == "Cargo.toml")
        {
            let source = (|| -> std::io::Result<String> {
                let f = std::fs::File::open(path)?;
                if f.metadata()?.len() > MAX_SOURCE_BYTES || total >= MAX_TOTAL_BYTES {
                    return Err(std::io::Error::other("presupuesto de lectura agotado"));
                }
                let mut text = String::new();
                f.take(MAX_SOURCE_BYTES + 1).read_to_string(&mut text)?;
                if text.len() > MAX_SOURCE_BYTES as usize {
                    return Err(std::io::Error::other("archivo demasiado grande"));
                }
                Ok(text)
            })();
            match source {
                Ok(source) => {
                    total += source.len();
                    if path.file_name().is_some_and(|n| n == "Cargo.toml") {
                        match source.parse::<DocumentMut>() {
                            Ok(doc) => {
                                manifests.insert(rel.clone(), doc);
                            }
                            Err(_) => graph.incomplete = true,
                        }
                    } else if let Some(found) = crate::ast::symbols(&rel, &source) {
                        // shortcut: 100 símbolos por archivo; ampliar si el grafo requiere detalle exhaustivo.
                        graph.incomplete |= found.len() > 100;
                        symbols = found
                            .into_iter()
                            .take(100)
                            .map(|s| Symbol {
                                kind: s.kind.into(),
                                name: s.name,
                                line: s.line,
                            })
                            .collect();
                    }
                }
                Err(_) => graph.incomplete = true,
            }
        }
        graph.files.push(File {
            path: rel,
            crate_name: None,
            symbols,
        });
    }
    for (path, doc) in &manifests {
        let Some(name) = doc
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(Item::as_str)
        else {
            continue;
        };
        let workspace = Path::new(path)
            .parent()
            .into_iter()
            .flat_map(Path::ancestors)
            .filter_map(|dir| {
                manifests.get(&dir.join("Cargo.toml").to_string_lossy().replace('\\', "/"))
            })
            .find_map(|d| d.get("workspace"));
        let mut dependencies = Vec::new();
        collect_dependencies(
            path,
            doc.as_table(),
            workspace,
            &manifests,
            "",
            &mut dependencies,
        );
        if let Some(targets) = doc.get("target").and_then(Item::as_table_like) {
            for (target, sections) in targets.iter() {
                if let Some(table) = sections.as_table_like() {
                    collect_dependencies(
                        path,
                        table,
                        workspace,
                        &manifests,
                        target,
                        &mut dependencies,
                    );
                }
            }
        }
        dependencies.sort();
        dependencies.dedup();
        graph.crates.push(Crate {
            name: name.into(),
            manifest: path.clone(),
            dependencies,
        });
    }
    for file in &mut graph.files {
        file.crate_name = graph
            .crates
            .iter()
            .filter(|c| {
                Path::new(&c.manifest)
                    .parent()
                    .is_some_and(|p| Path::new(&file.path).starts_with(p))
            })
            .max_by_key(|c| c.manifest.len())
            .map(|c| c.name.clone());
    }
    Ok(graph)
}

fn collect_dependencies(
    manifest: &str,
    table: &dyn TableLike,
    workspace: Option<&Item>,
    manifests: &BTreeMap<String, DocumentMut>,
    target: &str,
    out: &mut Vec<Dependency>,
) {
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        let Some(deps) = table.get(section).and_then(Item::as_table_like) else {
            continue;
        };
        for (alias, dep) in deps.iter() {
            let inherited = dep.get("workspace").and_then(Item::as_bool) == Some(true);
            let dep = if inherited {
                let Some(d) = workspace
                    .and_then(|w| w.get("dependencies"))
                    .and_then(|d| d.get(alias))
                else {
                    continue;
                };
                d
            } else {
                dep
            };
            let name = dep.get("package").and_then(Item::as_str).unwrap_or(alias);
            let local = if let Some(path) = dep.get("path").and_then(Item::as_str) {
                let base = if inherited {
                    Path::new(manifest)
                        .parent()
                        .into_iter()
                        .flat_map(Path::ancestors)
                        .find(|dir| {
                            manifests
                                .get(&dir.join("Cargo.toml").to_string_lossy().replace('\\', "/"))
                                .is_some_and(|d| d.get("workspace").is_some())
                        })
                } else {
                    Path::new(manifest).parent()
                };
                base.and_then(|b| normalize(&b.join(path).join("Cargo.toml")))
                    .filter(|p| manifests.contains_key(p))
            } else {
                // Sin path no se afirma que una dependencia del registro sea un crate local.
                None
            };
            if let Some(local) = local {
                out.push(Dependency {
                    name: manifests
                        .get(&local)
                        .and_then(|d| d.get("package"))
                        .and_then(|p| p.get("name"))
                        .and_then(Item::as_str)
                        .unwrap_or(name)
                        .into(),
                    manifest: local,
                    kind: if target.is_empty() {
                        section.into()
                    } else {
                        format!("{section} ({target})")
                    },
                });
            }
        }
    }
}

fn normalize(path: &Path) -> Option<String> {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            Component::ParentDir if out.pop() => {}
            _ => return None,
        }
    }
    Some(out.to_string_lossy().replace('\\', "/"))
}

impl Graph {
    /// Resumen con presupuesto fijo; el JSON conserva el detalle extraído.
    pub fn summary(&self) -> String {
        let mut out = format!(
            "# Grafo del proyecto\n{} archivos; {} crates. Parcial: {}.\n",
            self.files.len(),
            self.crates.len(),
            self.incomplete
        );
        for c in &self.crates {
            let _ = writeln!(out, "crate {} ({})", c.name, c.manifest);
            for d in &c.dependencies {
                let _ = writeln!(out, "  -> {} [{}]", d.name, d.kind);
            }
        }
        for file in &self.files {
            if out.len() > SUMMARY_BYTES {
                break;
            }
            let _ = write!(out, "{}:", file.path);
            for s in file.symbols.iter().take(8) {
                let _ = write!(out, " {} {}:{}", s.kind, s.name, s.line);
            }
            if file.symbols.len() > 8 {
                out.push_str(" …");
            }
            out.push('\n');
        }
        if out.len() > SUMMARY_BYTES {
            let mut end = SUMMARY_BYTES - "\n… resumen recortado\n".len();
            while !out.is_char_boundary(end) {
                end -= 1;
            }
            out.truncate(end);
            out.push_str("\n… resumen recortado\n");
        }
        out
    }

    /// Reemplazo atómico: un error conserva la última versión completa.
    pub fn save(&self, path: &Path) -> Result<(), GraphError> {
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("destino sin directorio"))?;
        std::fs::create_dir_all(parent)?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer(tmp.as_file_mut(), self)?;
        tmp.flush()?;
        tmp.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grafo_extrae_modulos_simbolos_y_dependencias_locales() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("a/src")).unwrap();
        std::fs::create_dir_all(root.join("b/src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers=['a','b']\n[workspace.dependencies]\nalias={package='b',path='b'}\n").unwrap();
        std::fs::write(root.join("a/Cargo.toml"), "[package]\nname='a'\nversion='0.1.0'\n[dependencies]\nalias={workspace=true}\n[dev-dependencies]\nb={path='../b'}\n[target.'cfg(unix)'.build-dependencies]\nb={path='../b'}\n").unwrap();
        std::fs::write(
            root.join("b/Cargo.toml"),
            "[package]\nname='b'\nversion='0.1.0'\n",
        )
        .unwrap();
        std::fs::write(
            root.join("a/src/lib.rs"),
            "pub mod api;\npub struct App;\npub fn start() {}\n",
        )
        .unwrap();
        std::fs::write(root.join("b/src/lib.rs"), "pub enum State { Ready }").unwrap();
        let graph = build(root).unwrap();
        let file = graph
            .files
            .iter()
            .find(|f| f.path == "a/src/lib.rs")
            .unwrap();
        assert_eq!(file.crate_name.as_deref(), Some("a"));
        assert!(
            file.symbols
                .iter()
                .any(|s| s.kind == "mod" && s.name == "api")
        );
        assert!(
            file.symbols
                .iter()
                .any(|s| s.kind == "fn" && s.name == "start" && s.line == 3)
        );
        assert_eq!(graph.crates[0].dependencies.len(), 3);
        assert!(
            graph.crates[0]
                .dependencies
                .iter()
                .all(|d| d.manifest == "b/Cargo.toml")
        );
        assert!(graph.summary().contains("-> b"));
        let dest = root.join(".symphony/graph.json");
        graph.save(&dest).unwrap();
        let first = std::fs::read(&dest).unwrap();
        build(root).unwrap().save(&dest).unwrap();
        assert_eq!(first, std::fs::read(dest).unwrap());
        std::fs::remove_file(root.join("b/src/lib.rs")).unwrap();
        assert!(
            !build(root)
                .unwrap()
                .files
                .iter()
                .any(|f| f.path == "b/src/lib.rs")
        );
    }

    #[test]
    fn grafo_respeta_ignorados_limites_y_manifiestos_rotos() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join(".gitignore"), "secreto.rs\n").unwrap();
        std::fs::write(root.join("secreto.rs"), "fn secreto() {}").unwrap();
        std::fs::write(root.join("target/artefacto.rs"), "fn generado() {}").unwrap();
        std::fs::write(root.join("Cargo.toml"), "[roto").unwrap();
        std::fs::write(
            root.join("src/grande.rs"),
            "x".repeat(MAX_SOURCE_BYTES as usize + 1),
        )
        .unwrap();
        let mut graph = build(root).unwrap();
        assert!(graph.incomplete);
        assert_eq!(graph.files.len(), 2);
        graph.files.extend((0..1000).map(|n| File {
            path: format!("módulo_{n}.rs"),
            crate_name: None,
            symbols: Vec::new(),
        }));
        let summary = graph.summary();
        assert!(summary.len() <= SUMMARY_BYTES);
        assert!(summary.contains("resumen recortado"));
        assert!(build(&root.join("inexistente")).is_err());
    }
}
