//! File watcher (P09.S5): un watcher por proyecto (no por agente) que mantiene los archivos
//! del proyecto buscables como `ctx://file/<ruta>`. Arranca con un recorrido inicial y después
//! aplica los cambios con debounce. Respeta `.gitignore` y omite lo oculto (`.git`,
//! `.symphony`), lo binario y lo grande. Los agentes trabajan en sus worktrees, fuera de la
//! raíz del proyecto: lo que se indexa es la copia principal (para buscar); `context.lines`
//! lee el archivo vivo del worktree del agente.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify::{RecursiveMode, Watcher as _};
use symphony_object_store::ObjectStore;
use symphony_store::WriterHandle;
use symphony_store::context::{self, FileUpsert};
use tokio::sync::mpsc;

/// Archivos más grandes no se indexan (se nombran en el handoff igual).
const MAX_FILE_BYTES: u64 = 512 * 1024;
/// ponytail: tope del recorrido inicial; un monorepo enorme indexa solo los primeros.
const MAX_FILES: usize = 5_000;
/// Silencio que se espera antes de aplicar una ráfaga de cambios.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// Archivos por transacción del writer.
const BATCH: usize = 200;

/// Watchers vivos, por id de proyecto.
#[derive(Default)]
pub struct Watchers {
    live: Mutex<HashMap<String, notify::RecommendedWatcher>>,
}

/// Un archivo para indexar (`text`) o para borrar del índice (`None`).
struct Change {
    rel: String,
    text: Option<String>,
}

/// `src\a.rs` → `ctx://file/src/a.rs` (y `@scope/x.ts` → `ctx://file/%40scope/x.ts`), solo si es
/// una dirección válida.
fn uri_of(rel: &str) -> Option<String> {
    use symphony_context::uri::CtxUri;
    let uri = CtxUri::File {
        path: rel.replace('\\', "/"),
        rev: None,
    }
    .to_string();
    CtxUri::parse(&uri).ok().map(|_| uri)
}

fn hidden(rel: &Path) -> bool {
    rel.components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
}

/// El texto del archivo si es indexable (existe, cabe y no es binario).
fn read_text(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn gitignore(root: &Path) -> Gitignore {
    let mut b = GitignoreBuilder::new(root);
    let _ = b.add(root.join(".gitignore"));
    b.build().unwrap_or_else(|_| Gitignore::empty())
}

/// ¿Lo ignora un `.gitignore` de una carpeta intermedia? (`gitignore` solo conoce el de la raíz,
/// y el recorrido inicial sí respeta los anidados.)
fn ignored_by_nested(root: &Path, path: &Path) -> bool {
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d == root || !d.starts_with(root) {
            break;
        }
        let file = d.join(".gitignore");
        if file.is_file() {
            let (matcher, _) = Gitignore::new(&file);
            if matcher.matched_path_or_any_parents(path, false).is_ignore() {
                return true;
            }
        }
        dir = d.parent();
    }
    false
}

/// Recorrido inicial: respeta los `.gitignore` anidados y omite lo oculto.
fn scan(root: &Path) -> Vec<Change> {
    let mut out = Vec::new();
    for entry in ignore::WalkBuilder::new(root)
        .hidden(true)
        .require_git(false)
        .build()
        .flatten()
    {
        if out.len() >= MAX_FILES {
            tracing::warn!(root = %root.display(), "el proyecto tiene más de {MAX_FILES} archivos: se indexan los primeros");
            break;
        }
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        if let Some(text) = read_text(path) {
            out.push(Change {
                rel: rel.to_string_lossy().into_owned(),
                text: Some(text),
            });
        }
    }
    out
}

/// Lo que cambió en una ráfaga: los archivos que siguen existiendo se releen; los que ya no,
/// salen del índice.
fn changes(root: &Path, ignore: &Gitignore, paths: BTreeSet<PathBuf>) -> Vec<Change> {
    let mut out = Vec::new();
    for path in paths {
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        if rel.as_os_str().is_empty()
            || hidden(rel)
            || ignore.matched_path_or_any_parents(rel, false).is_ignore()
        {
            continue;
        }
        let rel_s = rel.to_string_lossy().into_owned();
        if path.is_dir() || ignored_by_nested(root, &path) {
            continue;
        }
        let text = read_text(&path);
        // Un archivo bloqueado a medias de un guardado no se lee: no es un borrado, se conserva
        // lo indexado hasta el próximo evento. Binario o demasiado grande sí sale del índice.
        if text.is_none()
            && path.is_file()
            && std::fs::read(&path).is_err()
            && std::fs::metadata(&path).is_ok_and(|m| m.len() <= MAX_FILE_BYTES)
        {
            continue;
        }
        out.push(Change { rel: rel_s, text });
    }
    out
}

/// Guarda una tanda de cambios en una transacción del writer.
async fn apply(writer: &WriterHandle, objects: &ObjectStore, project: &str, batch: Vec<Change>) {
    // El archivo del blob se escribe acá, fuera del hilo del writer.
    let prepared: Vec<(String, Option<(String, String)>)> = batch
        .into_iter()
        .filter_map(|c| {
            let uri = uri_of(&c.rel)?;
            let stored = match c.text {
                Some(text) => {
                    let s = objects.write_object(text.as_bytes()).ok()?;
                    Some((s.hash, text))
                }
                None => None,
            };
            Some((uri, stored))
        })
        .collect();
    if prepared.is_empty() {
        return;
    }
    let (store, owner) = (objects.clone(), project.to_string());
    let written = writer
        .write(Box::new(move |t| {
            let sql = |e: symphony_object_store::ObjectError| {
                rusqlite::Error::ToSqlConversionFailure(Box::new(e))
            };
            let now = crate::runtime::now_ms();
            for (uri, stored) in prepared {
                match stored {
                    Some((_, text)) => {
                        // `put` ve que el archivo ya existe y solo registra la fila del blob.
                        let blob = store
                            .put(t, text.as_bytes(), Some("text/plain"))
                            .map_err(sql)?;
                        if let FileUpsert::Stored { id, previous } =
                            context::upsert_file(t, &owner, &uri, &blob.hash, now)?
                        {
                            store.add_ref(t, &blob.hash).map_err(sql)?;
                            if let Some(old) = previous {
                                store.release(t, &old).map_err(sql)?;
                            }
                            crate::context_index::index_chunks(t, id, &text)?;
                        }
                    }
                    None => {
                        if let Some(old) = context::delete_file(t, &owner, &uri)? {
                            store.release(t, &old).map_err(sql)?;
                        }
                    }
                }
            }
            Ok(())
        }))
        .await;
    if let Err(e) = written {
        tracing::warn!(project, error = %e, "no se pudo indexar archivos del proyecto");
    }
}

/// Quita del índice los archivos del proyecto que no están en `keep`.
async fn prune_missing(
    writer: &WriterHandle,
    objects: &ObjectStore,
    project: &str,
    keep: std::collections::HashSet<String>,
) {
    let (store, owner) = (objects.clone(), project.to_string());
    let pruned = writer
        .write(Box::new(move |t| {
            for uri in context::file_uris(t, &owner)? {
                if keep.contains(&uri) {
                    continue;
                }
                if let Some(old) = context::delete_file(t, &owner, &uri)? {
                    store
                        .release(t, &old)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                }
            }
            Ok(())
        }))
        .await;
    if let Err(e) = pruned {
        tracing::warn!(project, error = %e, "no se pudo limpiar el índice de archivos");
    }
}

async fn apply_all(writer: &WriterHandle, objects: &ObjectStore, project: &str, all: Vec<Change>) {
    let mut all = all.into_iter().peekable();
    while all.peek().is_some() {
        let batch: Vec<Change> = all.by_ref().take(BATCH).collect();
        apply(writer, objects, project, batch).await;
    }
}

impl Watchers {
    /// Empieza a vigilar el proyecto si todavía no se vigila. Nunca falla: sin watcher, la
    /// búsqueda sigue con lo ya indexado y `context.lines` lee el archivo vivo.
    pub fn ensure(&self, project: &str, root: &Path, writer: WriterHandle, objects: ObjectStore) {
        let Ok(mut live) = self.live.lock() else {
            return;
        };
        if live.contains_key(project) {
            return;
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let (tx, mut rx) = mpsc::unbounded_channel::<PathBuf>();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                for p in event.paths {
                    let _ = tx.send(p);
                }
            }
        });
        let mut watcher = match watcher {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(root = %root.display(), error = %e, "no se pudo crear el watcher");
                return;
            }
        };
        if let Err(e) = watcher.watch(root, RecursiveMode::Recursive) {
            tracing::warn!(root = %root.display(), error = %e, "no se pudo vigilar el proyecto");
            return;
        }
        live.insert(project.to_string(), watcher);

        let (project, root) = (project.to_string(), root.to_path_buf());
        handle.spawn(async move {
            let scan_root = root.clone();
            let initial = tokio::task::spawn_blocking(move || scan(&scan_root))
                .await
                .unwrap_or_default();
            // Con el recorrido completo, lo indexado que ya no está (borrado o ahora ignorado,
            // p. ej. un `target/` que entró antes de su regla) sale del índice.
            if initial.len() < MAX_FILES {
                let keep = initial.iter().filter_map(|c| uri_of(&c.rel)).collect();
                prune_missing(&writer, &objects, &project, keep).await;
            }
            apply_all(&writer, &objects, &project, initial).await;
            let ignore = gitignore(&root);
            while let Some(first) = rx.recv().await {
                let mut paths = BTreeSet::from([first]);
                // Debounce: junta la ráfaga hasta que haya `DEBOUNCE` de silencio.
                while let Ok(Some(p)) = tokio::time::timeout(DEBOUNCE, rx.recv()).await {
                    paths.insert(p);
                }
                let (r, ig) = (root.clone(), ignore.clone());
                let batch = tokio::task::spawn_blocking(move || changes(&r, &ig, paths))
                    .await
                    .unwrap_or_default();
                apply_all(&writer, &objects, &project, batch).await;
            }
        });
    }

    /// Cuántos proyectos se vigilan (tests y `status`).
    pub fn count(&self) -> usize {
        self.live.lock().map_or(0, |l| l.len())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn hidden_ignored_and_binary_files_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let write = |p: &str, b: &[u8]| {
            let path = root.join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b).unwrap();
        };
        write(".gitignore", b"target/\n*.log\n");
        write("src/a.rs", b"fn a() {}\n");
        write("target/x.rs", b"fn x() {}\n");
        write("debug.log", b"ruido\n");
        write(".git/config", b"[core]\n");
        write("logo.png", b"\x89PNG\0\0");
        let found: Vec<String> = scan(root)
            .into_iter()
            .map(|c| c.rel.replace('\\', "/"))
            .collect();
        assert_eq!(found, ["src/a.rs"], "ni ignorados, ni ocultos, ni binarios");

        let ig = gitignore(root);
        let paths = [
            "src/a.rs",
            "target/x.rs",
            "debug.log",
            ".git/config",
            "borrado.rs",
        ]
        .into_iter()
        .map(|p| root.join(p))
        .collect();
        let got: Vec<(String, bool)> = changes(root, &ig, paths)
            .into_iter()
            .map(|c| (c.rel.replace('\\', "/"), c.text.is_some()))
            .collect();
        assert_eq!(
            got,
            [
                ("borrado.rs".to_string(), false),
                ("src/a.rs".to_string(), true)
            ]
        );
        assert_eq!(uri_of("src\\a.rs").as_deref(), Some("ctx://file/src/a.rs"));
        assert_eq!(uri_of("../fuera.rs"), None);
        assert_eq!(
            uri_of("packages/@scope/a b#1.ts").as_deref(),
            Some("ctx://file/packages/%40scope/a b%231.ts")
        );
    }
}
