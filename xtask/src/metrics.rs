//! Exportación local de métricas por run (P13.S4).

use rusqlite::{Connection, OpenFlags};
use std::{fs, path::Path};

pub fn run(root: &Path, args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut db = root.join(".symphony/symphony.db");
    let mut out = root.join("docs/progress/model-runs.csv");
    let mut args = args;
    while let Some(arg) = args.next() {
        let target = match arg.as_str() {
            "--db" => &mut db,
            "--out" => &mut out,
            _ => return Err(format!("opción desconocida: {arg}")),
        };
        *target = args
            .next()
            .ok_or_else(|| format!("falta ruta para {arg}"))?
            .into();
    }
    if db.canonicalize().ok() == out.canonicalize().ok() && out.exists() {
        return Err("la salida no puede ser la base de datos".into());
    }
    let conn = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("no se pudo abrir {}: {e}", db.display()))?;
    let csv = render(&conn).map_err(|e| format!("no se pudieron consultar las métricas: {e}"))?;
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .map_err(|e| format!("no se pudo crear {}: {e}", parent.display()))?;
    }
    fs::write(&out, csv).map_err(|e| format!("no se pudo escribir {}: {e}", out.display()))?;
    eprintln!("xtask: metrics → {}", out.display());
    Ok(())
}

fn render(conn: &Connection) -> rusqlite::Result<String> {
    // Agregar cada relación antes del JOIN evita multiplicar los tokens y conteos.
    let mut stmt = conn.prepare(
        "WITH usage AS (
            SELECT run_id, sum(tokens_in) AS tokens_in, sum(tokens_out) AS tokens_out,
                   CASE WHEN count(DISTINCT source) > 1 THEN 'mixto' ELSE min(source) END AS source
            FROM usage_records GROUP BY run_id
        ), tools AS (
            SELECT run_id, count(*) AS n FROM tool_calls GROUP BY run_id
        ), replies AS (
            SELECT run_id, count(*) AS n FROM messages WHERE role = 'ASSISTANT' GROUP BY run_id
        )
        SELECT r.id, strftime('%Y-%m-%dT%H:%M:%fZ', r.started_at / 1000.0, 'unixepoch'),
               a.number, t.code, t.title, r.provider_id, r.model_id, r.status,
               r.end_reason, r.exit_code, (r.ended_at - r.started_at) / 1000.0,
               u.tokens_in, u.tokens_out, u.source, coalesce(tools.n, 0), coalesce(replies.n, 0)
        FROM agent_runs r JOIN agents a ON a.id = r.agent_id JOIN tasks t ON t.id = a.task_id
        LEFT JOIN usage u ON u.run_id = r.id LEFT JOIN tools ON tools.run_id = r.id
        LEFT JOIN replies ON replies.run_id = r.id ORDER BY r.started_at, r.id",
    )?;
    let mut csv = String::from(
        "run_id,date_utc,agent_number,task_code,task_title,provider_id,model_id,status,end_reason,exit_code,duration_seconds,tokens_in,tokens_out,token_source,tool_calls,assistant_messages\n",
    );
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        for i in 0..16 {
            if i > 0 {
                csv.push(',');
            }
            use rusqlite::types::ValueRef;
            let value = match row.get_ref(i)? {
                ValueRef::Null => String::new(),
                ValueRef::Integer(n) => n.to_string(),
                ValueRef::Real(n) => n.to_string(),
                _ => row.get::<_, String>(i)?,
            };
            if value.contains([',', '"', '\n', '\r']) {
                csv.push('"');
                csv.push_str(&value.replace('"', "\"\""));
                csv.push('"');
            } else {
                csv.push_str(&value);
            }
        }
        csv.push('\n');
    }
    Ok(csv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exporta_runs_con_migraciones_reales() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("symphony-metrics-{}", std::process::id()));
        fs::create_dir_all(&dir)?;
        let db = dir.join("metrics.db");
        let out = dir.join("metrics.csv");
        let mut conn = Connection::open(&db)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut migrations = fs::read_dir(crate::workspace_root().join("migrations"))?
            .map(|entry| entry.map(|e| e.path()))
            .collect::<Result<Vec<_>, _>>()?;
        migrations.retain(|p| p.extension().is_some_and(|e| e == "sql"));
        migrations.sort();
        for path in migrations {
            let tx = conn.transaction()?;
            tx.execute_batch(&fs::read_to_string(path)?)?;
            tx.commit()?;
        }
        conn.execute_batch(r#"
            INSERT INTO projects (id,name,root_path,default_branch,created_at) VALUES ('p','p','/p','main',0);
            INSERT INTO sessions (id,project_id,status,started_at) VALUES ('s','p','ACTIVE',0);
            INSERT INTO tasks (id,project_id,code,kind,title,status,created_at,updated_at)
                VALUES ('t','p','P13.S4','WORK','Título, "CSV"
segunda línea','RUNNING',0,0);
            INSERT INTO providers (id,display_name,cli_name,adapter_mode,setup_state) VALUES ('fake','Fake','fake','CLI','READY');
            INSERT INTO models (id,provider_id,cli_model_id,display_name,discovered_at) VALUES ('m','fake','m','M',0);
            INSERT INTO agents (id,project_id,session_id,task_id,number,state,execution_mode,failover_policy,context_mode,created_at,updated_at)
                VALUES ('a','p','s','t',17,'RUNNING','DECIDE_LATER','NONE','SAFE',0,0);
            INSERT INTO agent_runs (id,agent_id,seq,provider_id,model_id,status,end_reason,exit_code,started_at,ended_at)
                VALUES ('r1','a',1,'fake','m','EXITED','COMPLETED',0,0,1250),
                       ('r2','a',2,'fake','m','RUNNING',NULL,NULL,2000,NULL);
            INSERT INTO usage_records VALUES ('u1','r1','fake','m',10,3,'REPORTED',0),('u2','r1','fake','m',20,4,'ESTIMATED',0);
            INSERT INTO tool_calls (id,agent_id,run_id,tool_name,op_class,status,requested_at)
                VALUES ('c1','a','r1','shell',0,'DONE',0),('c2','a','r1','read',0,'DONE',0);
            INSERT INTO messages (id,agent_id,run_id,role,created_at)
                VALUES ('v1','a','r1','ASSISTANT',0),('v2','a','r1','ASSISTANT',0),('v3','a','r1','USER',0),('v4','a','r2','ASSISTANT',0);
        "#)?;
        drop(conn);
        run(
            &crate::workspace_root(),
            [
                "--db".into(),
                db.to_string_lossy().into_owned(),
                "--out".into(),
                out.to_string_lossy().into_owned(),
            ]
            .into_iter(),
        )?;
        let csv = fs::read_to_string(&out)?;
        let title = "\"Título, \"\"CSV\"\"\nsegunda línea\"";
        assert_eq!(csv.lines().next().map(|s| s.split(',').count()), Some(16));
        assert!(csv.ends_with(&format!("r1,1970-01-01T00:00:00.000Z,17,P13.S4,{title},fake,m,EXITED,COMPLETED,0,1.25,30,7,mixto,2,2\nr2,1970-01-01T00:00:02.000Z,17,P13.S4,{title},fake,m,RUNNING,,,,,,,0,1\n")));
        assert!(
            run(
                &dir,
                [
                    "--db".into(),
                    db.to_string_lossy().into_owned(),
                    "--out".into(),
                    db.to_string_lossy().into_owned()
                ]
                .into_iter()
            )
            .is_err()
        );
        assert!(run(&dir, ["--db".into()].into_iter()).is_err());
        assert!(run(&dir, ["--desconocida".into()].into_iter()).is_err());
        let conn = Connection::open(&db)?;
        conn.execute("UPDATE usage_records SET source = 'REPORTED'", [])?;
        assert!(render(&conn)?.contains(",30,7,REPORTED,2,2"));
        conn.execute("UPDATE usage_records SET source = 'ESTIMATED'", [])?;
        assert!(render(&conn)?.contains(",30,7,ESTIMATED,2,2"));
        drop(conn);
        fs::remove_dir_all(dir)?;
        Ok(())
    }
}
