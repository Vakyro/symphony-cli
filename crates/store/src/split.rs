//! Partir la base global de antes de P15.5 en una base por proyecto (ADR-0018).
//!
//! Se copia la base entera (`VACUUM INTO`, una copia consistente aunque el daemon viejo la tenga
//! abierta) y se borra todo lo que no es del proyecto: primero las filas con otro `project_id` y
//! luego, hasta que no cambie nada, las que apuntan por clave foránea a una fila que ya no existe.
//! Las tablas globales (proveedores, modelos, profiles) no cuelgan de ningún proyecto y quedan.

use std::path::Path;

use rusqlite::{Connection, OpenFlags, params};

/// Pasadas máximas de limpieza por claves foráneas (la cadena más larga es corta).
const MAX_PASSES: usize = 32;

/// Los proyectos de una base global: `(id, root_path)`.
pub fn list_projects(legacy: &Path) -> rusqlite::Result<Vec<(String, String)>> {
    let conn = Connection::open_with_flags(legacy, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn.prepare("SELECT id, root_path FROM projects ORDER BY created_at")?;
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

/// Crea `target` con solo lo de `project_id`. Devuelve cuántas filas quitó. `target` no puede
/// existir.
pub fn split_project(legacy: &Path, target: &Path, project_id: &str) -> rusqlite::Result<usize> {
    {
        let src = Connection::open_with_flags(legacy, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        src.execute("VACUUM INTO ?1", [target.to_string_lossy()])?;
    }
    let conn = Connection::open(target)?;
    // Se borra en desorden: las claves foráneas se vuelven a comprobar al final.
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let tables: Vec<String> = conn
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;

    let mut removed = 0;
    for t in &tables {
        if columns(&conn, t)?.iter().any(|c| c == "project_id") {
            removed += conn.execute(
                &format!("DELETE FROM \"{t}\" WHERE project_id IS NOT NULL AND project_id <> ?1"),
                params![project_id],
            )?;
        }
    }
    removed += conn.execute("DELETE FROM projects WHERE id <> ?1", params![project_id])?;
    for _ in 0..MAX_PASSES {
        let mut pass = 0;
        for t in &tables {
            for (from, parent, to) in foreign_keys(&conn, t)? {
                pass += conn.execute(
                    &format!(
                        "DELETE FROM \"{t}\" WHERE \"{from}\" IS NOT NULL
                         AND \"{from}\" NOT IN (SELECT \"{to}\" FROM \"{parent}\")"
                    ),
                    [],
                )?;
            }
        }
        removed += pass;
        if pass == 0 {
            break;
        }
    }
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let broken: i64 = conn
        .prepare("PRAGMA foreign_key_check")?
        .query_map([], |_| Ok(()))?
        .count() as i64;
    if broken > 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "{broken} filas quedaron con una clave foránea rota"
        )));
    }
    conn.execute_batch("VACUUM")?;
    Ok(removed)
}

fn columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare(&format!("PRAGMA table_info(\"{table}\")"))?
        .query_map([], |r| r.get(1))?
        .collect()
}

/// `(columna, tabla padre, columna del padre)`. Una FK sin columna explícita apunta a la clave
/// primaria del padre; las de clave compuesta no se tocan (no hay en el esquema).
fn foreign_keys(conn: &Connection, table: &str) -> rusqlite::Result<Vec<(String, String, String)>> {
    let rows: Vec<(String, String, Option<String>)> = conn
        .prepare(&format!("PRAGMA foreign_key_list(\"{table}\")"))?
        .query_map([], |r| Ok((r.get(3)?, r.get(2)?, r.get(4)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut out = Vec::new();
    for (from, parent, to) in rows {
        let to = match to {
            Some(t) => t,
            None => {
                let pks: Vec<String> = conn
                    .prepare(&format!("PRAGMA table_info(\"{parent}\")"))?
                    .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(5)?)))?
                    .filter_map(|r| r.ok().filter(|(_, pk)| *pk > 0).map(|(n, _)| n))
                    .collect();
                match pks.as_slice() {
                    [one] => one.clone(),
                    _ => continue,
                }
            }
        };
        out.push((from, parent, to));
    }
    Ok(out)
}
