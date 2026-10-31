//! P18.S6: «segundo cerebro» del proyecto en `<proyecto>/.symphony/brain/`.
//!
//! Markdowns deterministas —decisiones, cambios y bitácora por agente— que el daemon
//! regenera a partir de lo ya persistido en la base del proyecto. Sin embeddings ni LLM
//! (CONSTRAINTS): solo consultas SQL y plantillas. Solo el daemon escribe aquí; los CLIs
//! y la TUI lo leen. Ignorado por git (`.gitignore` que genera `symphony init`, P18.S5).

use std::fmt::Write as _;
use std::path::Path;

use rusqlite::{Connection, params};

use crate::export::stamp;

/// Nombre del directorio dentro de `.symphony/` del proyecto.
pub(crate) const BRAIN_DIR: &str = "brain";

/// Tope de entradas por documento: el cerebro es un resumen para releer, no un volcado.
const MAX_ENTRIES: usize = 300;
/// Límite de caracteres por detalle (los comandos ya llegan redactados de `tool_calls`).
const DETAIL_MAX: usize = 160;

/// Los tres documentos de una pasada del cerebro.
pub(crate) struct Brain {
    pub decisiones: String,
    pub cambios: String,
    /// `(archivo, contenido)` de la bitácora de cada agente (`agente-<n>.md`).
    pub bitacoras: Vec<(String, String)>,
}

/// Recorta un detalle a una línea acotada (los comandos traen saltos y pueden ser largos).
fn clip(text: &str) -> String {
    let flat = text.trim().replace(['\n', '\r'], " ");
    match flat.char_indices().nth(DETAIL_MAX) {
        Some((i, _)) => format!("{}…", &flat[..i]),
        None => flat,
    }
}

/// Motivo legible del cambio de executor (los códigos viven en `executor_changes.reason`).
fn reason_label(reason: &str) -> &str {
    match reason {
        "FAILOVER" => "failover por fallo del proveedor",
        "USER_SWITCH" => "cambio de modelo pedido por el usuario",
        "SUGGESTION_ACCEPTED" => "sugerencia aceptada",
        "RESTART" => "reinicio (fase siguiente o reintento)",
        "RECLAIM" => "recuperado por el scheduler",
        other => other,
    }
}

/// Marca del estado de una herramienta, como en el export de conversaciones.
fn status_mark(status: &str) -> &'static str {
    match status {
        "DONE" => "✓",
        "FAILED" | "DENIED" => "✗",
        _ => "…",
    }
}

/// Construye los documentos desde lo persistido. `now_ms` solo fecha el encabezado.
pub(crate) fn render(conn: &Connection, now_ms: i64) -> Result<Brain, rusqlite::Error> {
    Ok(Brain {
        decisiones: decisiones(conn, now_ms)?,
        cambios: cambios(conn, now_ms)?,
        bitacoras: bitacoras(conn)?,
    })
}

/// Decisiones de enrutamiento: cada cambio de executor, con el motivo y los modelos.
fn decisiones(conn: &Connection, now_ms: i64) -> Result<String, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT e.occurred_at, a.number, e.reason, mf.cli_model_id, mt.cli_model_id
         FROM executor_changes e
         JOIN agents a ON a.id = e.agent_id
         LEFT JOIN agent_runs rf ON rf.id = e.from_run_id
         LEFT JOIN models mf ON mf.id = rf.model_id
         LEFT JOIN agent_runs rt ON rt.id = e.to_run_id
         LEFT JOIN models mt ON mt.id = rt.model_id
         ORDER BY e.occurred_at DESC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![MAX_ENTRIES as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut out = format!(
        "# Decisiones de enrutamiento\n\n\
         Generado por el daemon (P18.S6) desde `executor_changes`; determinista, sin LLM. \
         Actualizado: {}\n\n",
        stamp(now_ms)
    );
    if rows.is_empty() {
        out.push_str("Sin decisiones registradas todavía.\n");
        return Ok(out);
    }
    out.push_str("| Fecha | Agente | De | A | Motivo |\n|---|---|---|---|---|\n");
    for (at, number, reason, from, to) in rows {
        let from = from.unwrap_or_else(|| "—".into());
        let to = to.unwrap_or_else(|| "—".into());
        let _ = writeln!(
            out,
            "| {} | #{number} | {from} | {to} | {} |",
            stamp(at),
            reason_label(&reason)
        );
    }
    Ok(out)
}

/// Cambios: herramientas de edición/escritura por agente, lo más reciente primero.
fn cambios(conn: &Connection, now_ms: i64) -> Result<String, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT t.requested_at, a.number, t.tool_name, t.status, t.command
         FROM tool_calls t
         JOIN agents a ON a.id = t.agent_id
         WHERE lower(t.tool_name) LIKE '%edit%'
            OR lower(t.tool_name) LIKE '%write%'
            OR lower(t.tool_name) LIKE '%patch%'
         ORDER BY t.requested_at DESC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![MAX_ENTRIES as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut out = format!(
        "# Cambios por agente\n\n\
         Generado por el daemon (P18.S6) desde `tool_calls` de edición/escritura; determinista, \
         sin LLM. Actualizado: {}\n\n",
        stamp(now_ms)
    );
    if rows.is_empty() {
        out.push_str("Sin cambios registrados todavía.\n");
        return Ok(out);
    }
    out.push_str("| Fecha | Agente | Herramienta | Estado | Detalle |\n|---|---|---|---|---|\n");
    for (at, number, tool, status, command) in rows {
        let detail = command.as_deref().map(clip).unwrap_or_default();
        let detail = if detail.is_empty() {
            String::new()
        } else {
            format!("`{detail}`")
        };
        let _ = writeln!(
            out,
            "| {} | #{number} | `{tool}` | {} {status} | {detail} |",
            stamp(at),
            status_mark(&status)
        );
    }
    Ok(out)
}

/// Bitácora: un documento por agente con su tarea y la tabla de runs.
fn bitacoras(conn: &Connection) -> Result<Vec<(String, String)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.number, a.state, a.state_reason, a.created_at, t.code, t.title
         FROM agents a
         LEFT JOIN tasks t ON t.id = a.task_id
         ORDER BY a.number",
    )?;
    let agents = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut out = Vec::new();
    for (id, number, state, reason, created, code, title) in agents {
        let mut runs_stmt = conn.prepare(
            "SELECT r.seq, r.started_at, r.ended_at, m.cli_model_id, r.status, r.end_reason
             FROM agent_runs r
             LEFT JOIN models m ON m.id = r.model_id
             WHERE r.agent_id = ?1
             ORDER BY r.seq",
        )?;
        let runs = runs_stmt
            .query_map(params![id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(runs_stmt);

        let mut text = format!("# Agente #{number}\n\n");
        let _ = writeln!(text, "- Creado: {}", stamp(created));
        let task = match (&code, &title) {
            (Some(c), Some(t)) => format!("{c} · {t}"),
            (Some(c), None) => c.clone(),
            _ => "—".into(),
        };
        let _ = writeln!(text, "- Tarea: {task}");
        let _ = writeln!(text, "- Estado: {state}");
        if let Some(r) = &reason {
            let _ = writeln!(text, "- Motivo: {}", clip(r));
        }
        text.push_str("\n## Runs\n\n");
        if runs.is_empty() {
            text.push_str("Sin runs todavía.\n");
        } else {
            text.push_str(
                "| # | Inicio | Fin | Modelo | Estado | Motivo |\n|---|---|---|---|---|---|\n",
            );
            for (seq, started, ended, model, status, end) in runs {
                let fin = ended.map(stamp).unwrap_or_else(|| "—".into());
                let model = model.unwrap_or_else(|| "—".into());
                let motivo = end.unwrap_or_else(|| "—".into());
                let _ = writeln!(
                    text,
                    "| {seq} | {} | {fin} | {model} | {status} | {motivo} |",
                    stamp(started)
                );
            }
        }
        out.push((format!("agente-{number}.md"), text));
    }
    Ok(out)
}

/// Escribe los documentos en `home/brain/`. Idempotente: se regenera entero cada vez y
/// quita las bitácoras de agentes que ya no existen (solo toca `agente-*.md`, lo nuestro).
pub(crate) fn write(home: &Path, brain: &Brain) -> std::io::Result<()> {
    let dir = home.join(BRAIN_DIR);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("decisiones.md"), &brain.decisiones)?;
    std::fs::write(dir.join("cambios.md"), &brain.cambios)?;
    let bitacora = dir.join("bitacora");
    std::fs::create_dir_all(&bitacora)?;
    let alive: std::collections::HashSet<&str> = brain
        .bitacoras
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    for entry in std::fs::read_dir(&bitacora)? {
        let path = entry?.path();
        let stale = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("agente-") && n.ends_with(".md") && !alive.contains(n));
        if stale {
            let _ = std::fs::remove_file(&path);
        }
    }
    for (name, text) in &brain.bitacoras {
        std::fs::write(bitacora.join(name), text)?;
    }
    Ok(())
}

impl crate::runtime::Runtime {
    /// Regenera el «segundo cerebro» del proyecto. Barato (consultas acotadas + archivos
    /// pequeños), así que corre en línea al cerrar runs y al cambiar de fase. Nunca
    /// propaga el fallo: un cerebro desactualizado no puede romper la ejecución.
    pub(crate) fn refresh_brain(&self) {
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let brain = {
                let conn = self
                    .reader
                    .lock()
                    .map_err(|_| "lector de la base no disponible".to_string())?;
                render(&conn, crate::runtime::now_ms())
                    .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?
            };
            write(&self.home, &brain)?;
            Ok(())
        })();
        if let Err(e) = result {
            tracing::warn!(error = %e, "no se pudo actualizar el segundo cerebro; se conserva el anterior");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Base real en un proyecto temporal, con un agente que editó, decidió y corrió.
    fn seeded() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let conn = symphony_store::open(&tmp.path().join("db")).unwrap();
        conn.execute(
            "INSERT INTO projects (id,name,root_path,default_branch,created_at)
             VALUES ('p','demo','repo','main',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (id,project_id,code,kind,title,status,created_at,updated_at)
             VALUES ('t','p','P18.S6','WORK','segundo cerebro','RUNNING',0,0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (id,project_id,status,started_at) VALUES ('s','p','ACTIVE',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO worktrees (id,project_id,path,branch,base_ref,deps_strategy,status,
                                    created_at)
             VALUES ('w','p','wt','symphony/x','main','NONE','READY',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO providers (id,display_name,cli_name,adapter_mode,setup_state)
             VALUES ('pr','Demo','demo','CLI','READY')",
            [],
        )
        .unwrap();
        for (id, name) in [("m1", "demo/a"), ("m2", "demo/b")] {
            conn.execute(
                "INSERT INTO models (id,provider_id,cli_model_id,display_name,discovered_at)
                 VALUES (?1,'pr',?2,?2,0)",
                params![id, name],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO agents (id,project_id,session_id,task_id,worktree_id,number,state,
                                 execution_mode,failover_policy,context_mode,created_at,updated_at)
             VALUES ('ag','p','s','t','w',7,'COMPLETED','DECIDE_LATER','ANY','SAFE',1000,9000)",
            [],
        )
        .unwrap();
        for (seq, run, model, status, end) in [
            (1, "r1", "m1", "HANDED_OFF", Some("QUOTA_EXHAUSTED")),
            (2, "r2", "m2", "EXITED", Some("COMPLETED")),
        ] {
            conn.execute(
                "INSERT INTO agent_runs (id,agent_id,seq,provider_id,model_id,status,end_reason,
                                         started_at,ended_at)
                 VALUES (?1,'ag',?2,'pr',?3,?4,?5,2000,8000)",
                params![run, seq, model, status, end],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO executor_changes (id,agent_id,from_run_id,to_run_id,reason,occurred_at)
             VALUES ('ec','ag','r1','r2','FAILOVER',3000)",
            [],
        )
        .unwrap();
        for (id, tool, command, status) in [
            ("c1", "Edit", Some("src/lib.rs"), "DONE"),
            ("c2", "Bash", Some("cargo test"), "DONE"),
            ("c3", "Write", None, "FAILED"),
        ] {
            conn.execute(
                "INSERT INTO tool_calls (id,agent_id,run_id,tool_name,command,op_class,status,
                                         requested_at)
                 VALUES (?1,'ag','r2',?2,?3,1,?4,4000)",
                params![id, tool, command, status],
            )
            .unwrap();
        }
        (tmp, conn)
    }

    #[test]
    fn render_genera_los_tres_documentos_desde_lo_persistido() {
        let (_tmp, conn) = seeded();
        let brain = render(&conn, 10_000).unwrap();

        assert!(brain.decisiones.contains("# Decisiones de enrutamiento"));
        assert!(
            brain
                .decisiones
                .contains("failover por fallo del proveedor")
        );
        assert!(brain.decisiones.contains("demo/a"));
        assert!(brain.decisiones.contains("demo/b"));
        assert!(brain.decisiones.contains("#7"));

        assert!(brain.cambios.contains("# Cambios por agente"));
        assert!(brain.cambios.contains("`Edit`"));
        assert!(brain.cambios.contains("✓ DONE"));
        assert!(brain.cambios.contains("✗ FAILED"));
        assert!(brain.cambios.contains("`src/lib.rs`"));
        // Bash no es una herramienta de edición: no entra en «cambios».
        assert!(!brain.cambios.contains("cargo test"));

        assert_eq!(brain.bitacoras.len(), 1);
        let (name, text) = &brain.bitacoras[0];
        assert_eq!(name, "agente-7.md");
        assert!(text.contains("# Agente #7"));
        assert!(text.contains("P18.S6 · segundo cerebro"));
        assert!(text.contains("| 1 |"));
        assert!(text.contains("| 2 |"));
        assert!(text.contains("QUOTA_EXHAUSTED"));
        assert!(text.contains("COMPLETED"));
    }

    #[test]
    fn render_con_base_vacia_deja_texto_amable() {
        let tmp = tempfile::tempdir().unwrap();
        let conn = symphony_store::open(&tmp.path().join("db")).unwrap();
        let brain = render(&conn, 0).unwrap();
        assert!(brain.decisiones.contains("Sin decisiones"));
        assert!(brain.cambios.contains("Sin cambios"));
        assert!(brain.bitacoras.is_empty());
    }

    #[tokio::test]
    async fn refresh_brain_escribe_solo_el_daemon_y_regenera_entero() {
        use symphony_store::Writer;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        let home = tmp.path().join(".symphony");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let db = home.join("db");
        let writer = Writer::start(&db).unwrap();
        writer
            .handle()
            .write(Box::new(|t| {
                t.execute(
                    "INSERT INTO projects (id,name,root_path,default_branch,created_at)
                     VALUES ('p','demo','repo','main',0)",
                    [],
                )?;
                t.execute(
                    "INSERT INTO sessions (id,project_id,status,started_at)
                     VALUES ('s','p','ACTIVE',0)",
                    [],
                )?;
                t.execute(
                    "INSERT INTO tasks (id,project_id,code,kind,title,status,created_at,updated_at)
                     VALUES ('t','p','T1','WORK','tarea','RUNNING',0,0)",
                    [],
                )?;
                t.execute(
                    "INSERT INTO agents (id,project_id,session_id,task_id,number,state,
                                         execution_mode,failover_policy,context_mode,
                                         created_at,updated_at)
                     VALUES ('ag','p','s','t',1,'RUNNING','DECIDE_LATER','NONE','RAW',0,0)",
                    [],
                )?;
                Ok(())
            }))
            .await
            .unwrap();
        let runtime = crate::runtime::Runtime::new(
            &home,
            writer.handle(),
            symphony_store::open(&db).unwrap(),
            crate::bus::EventBus::new(writer.handle(), 16, {
                use symphony_object_store::ObjectStore;
                ObjectStore::new(symphony_core::SymphonyHome::at(&home).objects_dir())
            }),
            Vec::new(),
            None,
        );
        runtime.refresh_brain();

        let brain = home.join("brain");
        let bitacora = std::fs::read_to_string(brain.join("bitacora").join("agente-1.md")).unwrap();
        assert!(bitacora.contains("# Agente #1"));
        assert!(
            std::fs::read_to_string(brain.join("decisiones.md"))
                .unwrap()
                .contains("Sin decisiones")
        );
        assert!(
            std::fs::read_to_string(brain.join("cambios.md"))
                .unwrap()
                .contains("Sin cambios")
        );

        // Una bitácora huérfana (agente borrado) desaparece en la siguiente pasada.
        std::fs::write(brain.join("bitacora").join("agente-99.md"), "viejo").unwrap();
        runtime.refresh_brain();
        assert!(!brain.join("bitacora").join("agente-99.md").exists());
        // Todo dentro del `.symphony/` del proyecto temporal: nada fuera del worktree.
        assert!(brain.starts_with(&home));
    }
}
