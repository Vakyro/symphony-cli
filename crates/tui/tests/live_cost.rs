//! P07.5.S9 (L3, gasta cuota): coste real de los cambios de proveedor del chat.
//!
//!   SYMPHONY_LIVE=1 cargo nextest run -p symphony-tui --no-capture live_cost
//!
//! Solo con permiso de Leo. Conversa con Claude (`SYMPHONY_COST_CLAUDE`, por defecto
//! `claude/haiku`), cambia a Codex (`SYMPHONY_COST_CODEX`, por defecto `openai/gpt-5.6-luna`),
//! sigue sin cambiar, vuelve a Claude y sigue. Imprime una fila por turno con los tokens de
//! entrada que reportó el proveedor (`TurnUsage`) y los del handoff que armó Symphony.
//! Lee la base con el CLI `sqlite3` (la TUI no enlaza `rusqlite`).
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::{Daemon, build, git};

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use symphony_protocol::transport;
use symphony_protocol::{Message, Outcome, Request};

type Conn = symphony_protocol::Connection<transport::LocalStream>;

async fn call(conn: &mut Conn, method: &str, params: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let id = format!("cost-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    conn.send(&Message::Request(Request::new(&id, method, params)))
        .await
        .unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            match conn.recv().await.unwrap() {
                Some(Message::Response(r)) if r.id == id => return r.outcome,
                Some(_) => continue,
                None => panic!("el daemon cerró la conexión en `{method}`"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("`{method}` no respondió"));
    match outcome {
        Outcome::Ok(v) => v,
        Outcome::Error(e) => panic!("`{method}`: {} ({})", e.message, e.code),
    }
}

fn sql(db: &Path, query: &str) -> String {
    let out = Command::new("sqlite3")
        .arg("-readonly")
        .arg(db)
        .arg(query)
        .output()
        .expect("hace falta el CLI `sqlite3` en el PATH");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn count(db: &Path, query: &str) -> i64 {
    sql(db, query).parse().unwrap_or(0)
}

/// Espera a que haya un `TurnUsage` más y el agente vuelva a `READY` sin run abierto.
async fn wait_turn(db: &Path, before: i64) {
    let deadline = Instant::now() + Duration::from_secs(420);
    loop {
        let usage = count(db, "SELECT COUNT(*) FROM events WHERE type = 'TurnUsage'");
        let ready = sql(db, "SELECT state FROM agents LIMIT 1") == "READY";
        let open = count(db, "SELECT COUNT(*) FROM agent_runs WHERE ended_at IS NULL");
        if usage > before && ready && open == 0 {
            return;
        }
        assert!(Instant::now() < deadline, "el turno no terminó a tiempo");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

const TOPICS: [&str; 6] = [
    "la historia del ajedrez",
    "cómo funciona un motor de combustión",
    "el ciclo del agua",
    "la fotosíntesis",
    "el origen de internet",
    "la tectónica de placas",
];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_cost_of_provider_switches() {
    if std::env::var("SYMPHONY_LIVE").as_deref() != Ok("1") {
        eprintln!("omitido: define SYMPHONY_LIVE=1 (gasta cuota; solo con permiso de Leo)");
        return;
    }
    let claude = std::env::var("SYMPHONY_COST_CLAUDE").unwrap_or_else(|_| "claude/haiku".into());
    let codex =
        std::env::var("SYMPHONY_COST_CODEX").unwrap_or_else(|_| "openai/gpt-5.6-luna".into());
    let daemon_bin = build("symphony-daemon", "symphonyd");
    build("symphony-cli", "symphony");

    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let repo = dir.path().join("demo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t.local"]);
    git(&repo, &["config", "user.name", "T"]);
    std::fs::write(repo.join("README.md"), "# demo\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);

    let _daemon = Daemon(
        Command::new(&daemon_bin)
            .env("SYMPHONY_HOME", &home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut conn = None;
    for _ in 0..400 {
        if let Ok(c) = transport::connect(&home).await {
            conn = Some(c);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let mut conn = conn.expect("el daemon no arrancó");
    let root = repo.display().to_string();
    let db = home.join("symphony.db");

    call(&mut conn, "providers.refresh", json!({})).await;
    call(&mut conn, "project.init", json!({ "project_root": root })).await;
    let models = call(&mut conn, "models.list", json!({})).await;
    for want in [&claude, &codex] {
        let ok = models
            .as_array()
            .or_else(|| models["models"].as_array())
            .is_some_and(|l| l.iter().any(|m| m["id"] == *want && m["available"] == true));
        assert!(ok, "modelo `{want}` no disponible: {models}");
    }

    let say = |k: usize| {
        format!(
            "Turno {k}: escribe unas 150 palabras sobre {}. Sin herramientas, sin archivos.",
            TOPICS[k % TOPICS.len()]
        )
    };
    let usage = |db: &Path| count(db, "SELECT COUNT(*) FROM events WHERE type = 'TurnUsage'");
    let mut plan: Vec<(String, String)> = Vec::new();

    // 1) Claude crea el chat y conversa 6 turnos.
    let created = call(
        &mut conn,
        "agent.create",
        json!({ "title": say(0), "project_root": root, "model": claude,
                "failover": "ANY", "chat": true }),
    )
    .await;
    let agent = created["agent_id"].as_str().unwrap().to_string();
    wait_turn(&db, 0).await;
    plan.push(("crear (Claude)".into(), claude.clone()));
    for k in 1..6 {
        let before = usage(&db);
        call(
            &mut conn,
            "agent.send",
            json!({ "agent": agent, "text": say(k) }),
        )
        .await;
        wait_turn(&db, before).await;
        plan.push((format!("continuar (Claude) #{k}"), claude.clone()));
    }
    // 2) Cambio a Codex con mensaje; luego un mensaje sin cambio (línea base de Codex).
    let before = usage(&db);
    call(
        &mut conn,
        "agent.switch",
        json!({ "agent": agent, "model": codex, "message": say(6) }),
    )
    .await;
    wait_turn(&db, before).await;
    plan.push(("CAMBIO Claude→Codex".into(), codex.clone()));
    let before = usage(&db);
    call(
        &mut conn,
        "agent.send",
        json!({ "agent": agent, "text": say(7) }),
    )
    .await;
    wait_turn(&db, before).await;
    plan.push(("continuar (Codex)".into(), codex.clone()));
    // 3) Vuelta a Claude con mensaje; luego un mensaje sin cambio.
    let before = usage(&db);
    call(
        &mut conn,
        "agent.switch",
        json!({ "agent": agent, "model": claude, "message": say(8) }),
    )
    .await;
    wait_turn(&db, before).await;
    plan.push(("CAMBIO Codex→Claude".into(), claude.clone()));
    let before = usage(&db);
    call(
        &mut conn,
        "agent.send",
        json!({ "agent": agent, "text": say(9) }),
    )
    .await;
    wait_turn(&db, before).await;
    plan.push(("continuar (Claude)".into(), claude.clone()));

    // Una fila por TurnUsage: proveedor, tokens de entrada reportados y handoff de ese run.
    let rows = sql(
        &db,
        "SELECT r.provider_id || ' ' || r.seq, \
                json_extract(e.payload_json, '$.context_tokens'), \
                COALESCE(h.tokens_sent, '-'), COALESCE(h.tokens_raw_estimate, '-'), COALESCE(h.mode, '-') \
         FROM events e JOIN agent_runs r ON r.id = e.run_id \
         LEFT JOIN handoffs h ON h.to_run_id = r.id \
         WHERE e.type = 'TurnUsage' ORDER BY e.id",
    );
    println!(
        "\n#  | paso                      | run        | tokens entrada (proveedor) | handoff enviado | handoff raw | modo"
    );
    for (i, line) in rows.lines().enumerate() {
        let c: Vec<&str> = line.split('|').collect();
        let (step, model) = plan.get(i).cloned().unwrap_or_default();
        println!(
            "{:<2} | {:<25} | {:<10} | {:>26} | {:>15} | {:>11} | {}   ({model})",
            i + 1,
            step,
            c.first().unwrap_or(&""),
            c.get(1).unwrap_or(&""),
            c.get(2).unwrap_or(&""),
            c.get(3).unwrap_or(&""),
            c.get(4).unwrap_or(&"")
        );
    }
    assert!(
        rows.lines().count() >= plan.len(),
        "faltan filas de uso:\n{rows}"
    );
}
