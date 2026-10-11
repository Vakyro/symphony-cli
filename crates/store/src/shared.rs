//! Skills y servidores MCP compartidos (DB §3.J, migración 003). El filesystem manda: el
//! archivo define la skill o el servidor, y estas tablas solo los indexan y guardan el
//! interruptor `enabled` del usuario, que sobrevive a cada sincronización.

use rusqlite::{Connection, OptionalExtension, params};
use symphony_core::{McpServerId, SkillId};

use crate::repo::RepoError;

/// Lo que el escaneo encontró de una skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundSkill {
    pub name: String,
    /// `USER` o `PROJECT`.
    pub scope: &'static str,
    pub path: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRow {
    pub id: String,
    pub name: String,
    pub scope: String,
    pub path: String,
    pub content_hash: Option<String>,
    pub enabled: bool,
}

/// Lo que el escaneo encontró de un servidor MCP (`mcp.toml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundMcp {
    pub name: String,
    pub scope: &'static str,
    /// `STDIO` o `HTTP`.
    pub transport: &'static str,
    /// `SHARED` o `PER_CLIENT`.
    pub share_mode: &'static str,
    pub command: Option<String>,
    pub url: Option<String>,
    /// Valor por omisión del archivo; solo se usa al crear la fila.
    pub enabled_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRow {
    pub id: String,
    pub name: String,
    pub scope: String,
    pub transport: String,
    pub share_mode: String,
    pub command: Option<String>,
    pub url: Option<String>,
    pub enabled: bool,
}

fn project_for(scope: &str, project: &str) -> Option<String> {
    (scope == "PROJECT").then(|| project.to_string())
}

/// Deja `skills` igual a lo encontrado: nuevas, actualizadas (conservan `enabled`) y borradas.
/// Solo toca las de usuario y las de este proyecto.
pub fn sync_skills(
    conn: &Connection,
    project: &str,
    found: &[FoundSkill],
    now: i64,
) -> Result<(), RepoError> {
    for s in found {
        let proj = project_for(s.scope, project);
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM skills WHERE scope = ?1 AND COALESCE(project_id, '') = COALESCE(?2, '') AND name = ?3",
                params![s.scope, proj, s.name],
                |r| r.get(0),
            )
            .optional()?;
        match existing {
            Some(id) => {
                conn.execute(
                    "UPDATE skills SET path = ?2, content_hash = ?3, updated_at = ?4 WHERE id = ?1",
                    params![id, s.path, s.content_hash, now],
                )?;
            }
            None => {
                conn.execute(
                    "INSERT INTO skills (id, name, scope, project_id, path, content_hash, enabled, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7)",
                    params![
                        SkillId::new().to_string(),
                        s.name,
                        s.scope,
                        proj,
                        s.path,
                        s.content_hash,
                        now
                    ],
                )?;
            }
        }
    }
    let mut stmt = conn.prepare(
        "SELECT id, name, scope FROM skills WHERE scope <> 'BUILTIN' AND (project_id IS NULL OR project_id = ?1)",
    )?;
    let rows: Vec<(String, String, String)> = stmt
        .query_map([project], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (id, name, scope) in rows {
        if !found.iter().any(|f| f.name == name && f.scope == scope) {
            conn.execute("DELETE FROM skills WHERE id = ?1", [id])?;
        }
    }
    Ok(())
}

/// Las skills del usuario y las del proyecto, por nombre.
pub fn list_skills(conn: &Connection, project: &str) -> Result<Vec<SkillRow>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, scope, path, content_hash, enabled FROM skills
         WHERE project_id IS NULL OR project_id = ?1 ORDER BY name COLLATE NOCASE, scope",
    )?;
    let rows = stmt.query_map([project], |r| {
        Ok(SkillRow {
            id: r.get(0)?,
            name: r.get(1)?,
            scope: r.get(2)?,
            path: r.get(3)?,
            content_hash: r.get(4)?,
            enabled: r.get::<_, i64>(5)? == 1,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Enciende o apaga una skill por nombre (en los ámbitos visibles). Devuelve cuántas filas.
pub fn set_skill_enabled(
    conn: &Connection,
    project: &str,
    name: &str,
    enabled: bool,
) -> Result<usize, RepoError> {
    Ok(conn.execute(
        "UPDATE skills SET enabled = ?3 WHERE name = ?2 AND (project_id IS NULL OR project_id = ?1)",
        params![project, name, i64::from(enabled)],
    )?)
}

/// Como [`sync_skills`], para `mcp_servers`.
pub fn sync_mcp(conn: &Connection, project: &str, found: &[FoundMcp]) -> Result<(), RepoError> {
    for s in found {
        let proj = project_for(s.scope, project);
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM mcp_servers WHERE scope = ?1 AND COALESCE(project_id, '') = COALESCE(?2, '') AND name = ?3",
                params![s.scope, proj, s.name],
                |r| r.get(0),
            )
            .optional()?;
        match existing {
            Some(id) => {
                conn.execute(
                    "UPDATE mcp_servers SET transport = ?2, share_mode = ?3, command = ?4, url = ?5 WHERE id = ?1",
                    params![id, s.transport, s.share_mode, s.command, s.url],
                )?;
            }
            None => {
                conn.execute(
                    "INSERT INTO mcp_servers (id, name, scope, project_id, transport, share_mode, command, url, enabled, status)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'STOPPED')",
                    params![
                        McpServerId::new().to_string(),
                        s.name,
                        s.scope,
                        proj,
                        s.transport,
                        s.share_mode,
                        s.command,
                        s.url,
                        i64::from(s.enabled_default)
                    ],
                )?;
            }
        }
    }
    let mut stmt = conn.prepare(
        "SELECT id, name, scope FROM mcp_servers WHERE project_id IS NULL OR project_id = ?1",
    )?;
    let rows: Vec<(String, String, String)> = stmt
        .query_map([project], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (id, name, scope) in rows {
        if !found.iter().any(|f| f.name == name && f.scope == scope) {
            conn.execute("DELETE FROM mcp_servers WHERE id = ?1", [id])?;
        }
    }
    Ok(())
}

pub fn list_mcp(conn: &Connection, project: &str) -> Result<Vec<McpRow>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, scope, transport, share_mode, command, url, enabled FROM mcp_servers
         WHERE project_id IS NULL OR project_id = ?1 ORDER BY name COLLATE NOCASE, scope",
    )?;
    let rows = stmt.query_map([project], |r| {
        Ok(McpRow {
            id: r.get(0)?,
            name: r.get(1)?,
            scope: r.get(2)?,
            transport: r.get(3)?,
            share_mode: r.get(4)?,
            command: r.get(5)?,
            url: r.get(6)?,
            enabled: r.get::<_, i64>(7)? == 1,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn set_mcp_enabled(
    conn: &Connection,
    project: &str,
    name: &str,
    enabled: bool,
) -> Result<usize, RepoError> {
    Ok(conn.execute(
        "UPDATE mcp_servers SET enabled = ?3 WHERE name = ?2 AND (project_id IS NULL OR project_id = ?1)",
        params![project, name, i64::from(enabled)],
    )?)
}
