//! `symphonyd migrate-split <home del usuario>` (P15.5.S3, ADR-0018): reparte la base global de
//! antes de P15.5 en una base por proyecto. Es del daemon porque solo el daemon toca las bases.
//!
//! No borra nada: la base global y sus objetos quedan donde estaban (el usuario decide cuándo
//! quitarlos). Es idempotente: un proyecto que ya tiene su base no se vuelve a tocar.

use std::path::Path;

use rusqlite::{Connection, OpenFlags, params};

/// Reparte `<home>/symphony.db`. Devuelve una línea por proyecto, para mostrarlas.
pub fn split_all(user_home: &Path) -> Result<Vec<String>, String> {
    let legacy = user_home.join("symphony.db");
    if !legacy.is_file() {
        return Ok(vec!["no hay una base global que repartir.".into()]);
    }
    let projects = symphony_store::split::list_projects(&legacy).map_err(|e| e.to_string())?;
    let mut lines = Vec::new();
    for (id, root) in projects {
        let root = Path::new(&root);
        let state = root.join(".symphony");
        let target = state.join("symphony.db");
        if !root.is_dir() {
            lines.push(format!(
                "{}: la carpeta ya no existe, se omite.",
                root.display()
            ));
            continue;
        }
        if target.exists() {
            lines.push(format!("{}: ya tenía su base, no se toca.", root.display()));
            continue;
        }
        std::fs::create_dir_all(&state).map_err(|e| format!("{}: {e}", state.display()))?;
        let removed = symphony_store::split::split_project(&legacy, &target, &id)
            .map_err(|e| format!("{}: {e}", root.display()))?;
        let linked = link_tree(&user_home.join("objects"), &state.join("objects"));
        lines.push(format!(
            "{}: base creada ({removed} filas de otros proyectos quitadas, {linked} objetos).",
            root.display()
        ));
    }
    match seed_learning(&legacy, &user_home.join("learning.db")) {
        Ok(0) => {}
        Ok(n) => lines.push(format!(
            "lo aprendido por el router pasa a ser compartido ({n} muestras)."
        )),
        Err(e) => lines.push(format!("aviso: no se pudo compartir lo aprendido: {e}")),
    }
    lines.push(format!(
        "la base global {} sigue ahí, sin usarse. Los worktrees de los agentes que ya existían siguen en {}: \
         no borres esa carpeta mientras los uses (los agentes nuevos usan <proyecto>/.symphony/worktrees).",
        legacy.display(),
        user_home.join("worktrees").display()
    ));
    Ok(lines)
}

/// Los objetos son inmutables y se nombran por su hash: se enlazan (sin gastar disco) y, si el
/// sistema no deja, se copian. Devuelve cuántos quedaron.
fn link_tree(from: &Path, to: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(from) else {
        return 0;
    };
    let mut n = 0;
    for entry in entries.flatten() {
        let dest = to.join(entry.file_name());
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            n += link_tree(&entry.path(), &dest);
        } else if kind.is_file() {
            if let Some(dir) = dest.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if dest.exists()
                || std::fs::hard_link(entry.path(), &dest).is_ok()
                || std::fs::copy(entry.path(), &dest).is_ok()
            {
                n += 1;
            }
        }
    }
    n
}

/// Lo que el router aprendió hasta ahora (conteo y suma por profile y modelo) pasa a la base
/// compartida como muestras sintéticas con la misma media, sin duplicar si se repite.
fn seed_learning(legacy: &Path, shared_path: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    let src = Connection::open_with_flags(legacy, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = src.prepare(
        "SELECT profile_id, model_id, sample_count, outcome_score_sum
         FROM profile_models WHERE sample_count > 0",
    )?;
    let rows: Vec<(String, String, i64, f64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    let shared = crate::learning::open_shared(shared_path)?;
    let mut added = 0;
    for (profile, model, n, sum) in rows {
        let mean = sum / n as f64;
        for i in 0..n {
            added += shared.execute(
                "INSERT OR IGNORE INTO samples (run_id, profile, model, score) VALUES (?1, ?2, ?3, ?4)",
                params![format!("legacy:{profile}:{model}:{i}"), profile, model, mean],
            )?;
        }
    }
    Ok(added)
}
