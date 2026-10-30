//! P18.S4: construcción en segundo plano, con un trabajador y una petición pendiente por raíz.
//! El resumen se guarda en el object store y se ofrece mediante el índice ctx existente.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use symphony_context::graph;
use symphony_object_store::ObjectStore;
use symphony_store::{WriterHandle, context};
use tokio::sync::{Semaphore, watch};

use crate::runtime::{Runtime, now_ms};
use crate::scheduling::{Scheduling, Submitted, Waited};

pub(crate) const URI_PREFIX: &str = "ctx://file/.symphony/graphs/";

struct Worker {
    tx: watch::Sender<Option<String>>,
    summary: Arc<Mutex<Option<String>>>,
}

pub(crate) struct Graphs {
    workers: Mutex<HashMap<PathBuf, Worker>>,
    slots: Arc<Semaphore>,
}

impl Default for Graphs {
    fn default() -> Self {
        Self {
            workers: Mutex::default(),
            slots: Arc::new(Semaphore::new(1)),
        }
    }
}

fn key(root: &Path) -> String {
    blake3::hash(root.to_string_lossy().as_bytes())
        .to_hex()
        .to_string()
}

pub(crate) fn uri(root: &Path) -> String {
    format!("{URI_PREFIX}{}/summary.md", key(root))
}

impl Runtime {
    /// Solo encola: ni recorre el disco ni espera al scheduler en el camino de arranque.
    pub(crate) fn refresh_graph(&self, root: &Path, project: Option<String>) {
        self.graphs.request(
            root,
            project,
            &self.home,
            self.writer.clone(),
            self.scheduling.get().cloned(),
        );
    }

    pub(crate) fn graph_context(&self, root: &Path) -> String {
        let cached = self.graphs.workers.lock().ok().and_then(|workers| {
            workers
                .get(root)
                .and_then(|w| w.summary.lock().ok()?.clone())
        });
        format!(
            "\n\nContexto estructural del proyecto: {} (context_retrieve/context_search).\n{}",
            uri(root),
            cached.as_deref().unwrap_or("Se construye en segundo plano; si aún no está disponible, continúa y consúltalo después.")
        )
    }
}

impl Graphs {
    fn request(
        &self,
        root: &Path,
        project: Option<String>,
        home: &Path,
        writer: WriterHandle,
        scheduling: Option<Arc<Scheduling>>,
    ) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let Ok(mut workers) = self.workers.lock() else {
            return;
        };
        if let Some(w) = workers.get(root) {
            w.tx.send_replace(project);
            return;
        }
        let (tx, mut rx) = watch::channel(project);
        let summary = Arc::new(Mutex::new(None));
        workers.insert(
            root.to_path_buf(),
            Worker {
                tx,
                summary: summary.clone(),
            },
        );
        let root = root.to_path_buf();
        let dest = home.join("graphs").join(key(&root)).join("graph.json");
        let objects = ObjectStore::new(symphony_core::SymphonyHome::at(home).objects_dir());
        let slots = self.slots.clone();
        handle.spawn(async move {
            loop {
                let project = rx.borrow_and_update().clone();
                let Ok(_permit) = slots.acquire().await else { break };
                let result = rebuild(&root, &dest, project, &writer, &objects, scheduling.as_ref()).await;
                match result {
                    Ok(text) => {
                        if let Ok(mut cache) = summary.lock() { *cache = Some(text); }
                    }
                    Err(e) => tracing::warn!(root = %root.display(), error = %e, "no se pudo actualizar el grafo; se conserva el anterior"),
                }
                drop(_permit);
                if rx.changed().await.is_err() { break; }
            }
        });
    }
}

async fn rebuild(
    root: &Path,
    dest: &Path,
    project: Option<String>,
    writer: &WriterHandle,
    objects: &ObjectStore,
    scheduling: Option<&Arc<Scheduling>>,
) -> Result<String, String> {
    let id = format!("graph:{}", key(root));
    if let Some(s) = scheduling {
        let request = symphony_scheduler::Request {
            id: id.clone(),
            tag: id.clone(),
            agent: id.clone(),
            agent_label: "Grafo del proyecto".into(),
            class: symphony_scheduler::OpClass::Heavy,
            priority: -1,
            command: None,
            now_ms: now_ms(),
        };
        if let Submitted::Queued(q) = s.submit(request)
            && let Waited::TimedOut { reason } = s.wait(q, Duration::from_secs(60)).await
        {
            return Err(symphony_scheduler::explain(&reason));
        }
    }
    let (build_root, dest, store) = (root.to_path_buf(), dest.to_path_buf(), objects.clone());
    let built = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let graph = graph::build(&build_root).map_err(|e| e.to_string())?;
        let summary = symphony_core::redact(&graph.summary()).into_owned();
        graph.save(&dest).map_err(|e| e.to_string())?;
        store
            .write_object(summary.as_bytes())
            .map_err(|e| e.to_string())?;
        Ok(summary)
    })
    .await;
    if let Some(s) = scheduling {
        s.release(&id);
    }
    let summary = built.map_err(|e| e.to_string())??;
    if let Some(project) = project {
        let (text, store, uri) = (summary.clone(), objects.clone(), uri(root));
        let chunks = crate::context_index::chunks(&text);
        writer
            .write(Box::new(move |t| {
                let sql = |e| rusqlite::Error::ToSqlConversionFailure(Box::new(e));
                let blob = store
                    .put(t, text.as_bytes(), Some("text/markdown"))
                    .map_err(sql)?;
                if let context::FileUpsert::Stored { id, previous } =
                    context::upsert_file(t, &project, &uri, &blob.hash, now_ms())?
                {
                    store.add_ref(t, &blob.hash).map_err(sql)?;
                    if let Some(old) = previous {
                        store.release(t, &old).map_err(sql)?;
                    }
                    context::replace_chunks(t, id, &chunks)?;
                }
                Ok(())
            }))
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_store::Writer;

    #[tokio::test]
    async fn arranque_no_espera_al_grafo_y_refresco_actualiza_ctx() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        let home = tmp.path().join(".symphony");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(root.join("lib.rs"), "pub mod inicial;").unwrap();
        let db = home.join("db");
        let writer = Writer::start(&db).unwrap();
        writer.handle().write(Box::new(|t| {
            t.execute("INSERT INTO projects (id,name,root_path,default_branch,created_at) VALUES ('p','demo','repo','main',0)", [])?;
            Ok(())
        })).await.unwrap();
        let objects = ObjectStore::new(symphony_core::SymphonyHome::at(&home).objects_dir());
        let runtime = Runtime::new(
            &home,
            writer.handle(),
            symphony_store::open(&db).unwrap(),
            crate::bus::EventBus::new(writer.handle(), 16, objects.clone()),
            Vec::new(),
            None,
        );
        // El trabajador está detenido por una barrera real, no por medir milisegundos de disco.
        let held = runtime.graphs.slots.acquire().await.unwrap();
        runtime.refresh_graph(&root, Some("p".into()));
        let prompt = runtime.graph_context(&root);
        assert!(prompt.contains(&uri(&root)));
        assert!(prompt.contains("segundo plano"));
        tokio::task::yield_now().await;
        let dest = home.join("graphs").join(key(&root)).join("graph.json");
        assert!(
            !dest.exists(),
            "el arranque devolvió el control con el grafo aún bloqueado"
        );
        std::fs::write(root.join("lib.rs"), "pub mod actualizado;").unwrap();
        runtime.refresh_graph(&root, Some("p".into()));
        drop(held);
        let conn = symphony_store::open(&db).unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Ok(a) = crate::context_mcp::retrieve(&conn, &objects, "p", &uri(&root))
                    && a.value["text"]
                        .as_str()
                        .is_some_and(|s| s.contains("actualizado"))
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(dest.exists());
        std::fs::remove_file(root.join("lib.rs")).unwrap();
        runtime.refresh_graph(&root, Some("p".into()));
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let a = crate::context_mcp::retrieve(&conn, &objects, "p", &uri(&root)).unwrap();
                if !a.value["text"].as_str().unwrap().contains("actualizado") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_ne!(uri(&root), uri(&root.join("otro")));
    }
}
