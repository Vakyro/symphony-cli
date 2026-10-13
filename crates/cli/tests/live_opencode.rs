//! P11.5.S8–S9 (L3): lo que quedó sin ver en vivo, probado **solo con modelos gratuitos de
//! OpenCode** (permiso de Leo, 2026-10-10). Daemon real, `symphony` y su IPC.
//!
//!   SYMPHONY_LIVE=1 cargo nextest run -p symphony-cli --no-capture live_opencode
//!
//! Protecciones: cada modelo pasa por `free()` (si no es gratuito, el test falla antes de
//! lanzarlo) y todos los agentes se crean con `failover = NONE`: un fallo del modelo gratuito
//! nunca salta a un proveedor de pago.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::symphonyd;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use symphony_protocol::{Message, Outcome, Request, transport};

/// Dos modelos gratuitos distintos (el segundo, para los cambios de proveedor entre modelos).
const A: &str = "opencode/big-pickle";
const B: &str = "opencode/ling-3.1-flash-free";

const TASK: &str = "Crea 4 archivos de texto en el directorio actual, en orden. n1.txt contiene el numero 1. \
Para k de 2 a 4: lee n(k-1).txt y crea nk.txt con ese numero mas 1. Hazlo paso a paso, \
sin saltarte la lectura de cada archivo. Al terminar responde solo: hecho";

fn live() -> bool {
    std::env::var("SYMPHONY_LIVE").as_deref() == Ok("1")
}

/// Un modelo que no sea gratuito no se lanza: este archivo nunca gasta cuota.
fn free(model: &str) -> &str {
    assert!(
        model == A || model.ends_with("-free"),
        "este test solo usa modelos gratuitos de OpenCode: {model}"
    );
    model
}

fn git(dir: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

/// Estado de git del repo de Symphony (donde corre el test): un modelo que escribe fuera de su
/// worktree lo ensucia, y eso no puede pasar sin que el test lo diga (visto en la corrida 1 de
/// S9.2: `n1.txt`…`n8.txt` aparecieron en la raíz del repo).
fn workspace_status() -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

struct Env {
    _dir: tempfile::TempDir,
    home: PathBuf,
    repo: PathBuf,
    workspace_before: String,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let (home, repo) = (dir.path().join("home"), dir.path().join("repo"));
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@t.local"]);
        git(&repo, &["config", "user.name", "T"]);
        std::fs::write(repo.join("README.md"), "demo\n").unwrap();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(
            repo.join("src/zafiro.rs"),
            "// La constante zafirocuantico42 vive solo en este archivo.\npub const X: u32 = 42;\n",
        )
        .unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "init"]);
        Self {
            _dir: dir,
            home,
            repo,
            workspace_before: workspace_status(),
        }
    }

    fn cli(&self, args: &[&str]) -> (bool, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_symphony"))
            .current_dir(&self.repo)
            .args(args)
            .env("SYMPHONY_HOME", &self.home)
            .env("SYMPHONYD", symphonyd())
            .output()
            .unwrap();
        (
            out.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    }

    /// Una llamada IPC (el CLI no expone `agent.create --chat` ni `agent.switch --message`).
    fn ipc(&self, method: &str, params: Value) -> Result<Value, String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut conn = transport::connect(&self.home)
                .await
                .map_err(|e| e.to_string())?;
            conn.send(&Message::Request(Request::new("t1", method, params)))
                .await
                .map_err(|e| e.to_string())?;
            loop {
                match conn.recv().await {
                    Ok(Some(Message::Response(r))) if r.id == "t1" => {
                        return match r.outcome {
                            Outcome::Ok(v) => Ok(v),
                            Outcome::Error(e) => Err(format!("{}: {}", e.code, e.message)),
                        };
                    }
                    Ok(Some(_)) => continue,
                    _ => return Err("conexión cerrada".into()),
                }
            }
        })
    }

    /// Arranca el daemon (el primer comando lo hace solo).
    fn start(&self) {
        let (ok, out) = self.cli(&["status"]);
        assert!(ok, "no arrancó el daemon: {out}");
    }

    fn text(&self, sql: &str) -> Option<String> {
        let conn = symphony_store::open_reader(&self.home.join("symphony.db")).ok()?;
        conn.query_row(sql, [], |r| r.get(0)).ok()
    }

    fn num(&self, sql: &str) -> i64 {
        let conn = symphony_store::open_reader(&self.home.join("symphony.db")).unwrap();
        conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0)
    }

    fn state(&self, number: i64) -> String {
        self.text(&format!("SELECT state FROM agents WHERE number = {number}"))
            .unwrap_or_default()
    }

    fn runs(&self, number: i64) -> i64 {
        self.num(&format!(
            "SELECT COUNT(*) FROM agent_runs WHERE agent_id = (SELECT id FROM agents WHERE number = {number})"
        ))
    }

    /// Espera a que el agente quede en uno de `want` con más de `runs_over` runs.
    fn wait(&self, number: i64, want: &[&str], runs_over: i64, secs: u64) -> Result<(), String> {
        let start = Instant::now();
        loop {
            let state = self.state(number);
            if want.contains(&state.as_str()) && self.runs(number) > runs_over {
                return Ok(());
            }
            if matches!(state.as_str(), "FAILED" | "CANCELLED" | "BLOCKED")
                && self.runs(number) > runs_over
            {
                let why = self
                    .text(&format!(
                        "SELECT state_reason FROM agents WHERE number = {number}"
                    ))
                    .unwrap_or_default();
                return Err(format!("el agente #{number} quedó {state}: {why}"));
            }
            if start.elapsed() > Duration::from_secs(secs) {
                return Err(format!("el agente #{number} sigue {state} tras {secs} s"));
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Una fila por run: modelo, cómo terminó, tiempo al primer evento y total, tokens del handoff.
    fn report(&self, number: i64) -> String {
        let conn = symphony_store::open_reader(&self.home.join("symphony.db")).unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT r.seq, r.model_id, COALESCE(r.end_reason,'-'), r.started_at,
                        COALESCE(r.ended_at, r.started_at),
                        (SELECT MIN(e.occurred_at) FROM events e WHERE e.run_id = r.id
                           AND e.type IN ('AssistantText','AssistantDelta','ToolRequested','CommandRequested')),
                        (SELECT h.tokens_sent FROM handoffs h WHERE h.to_run_id = r.id),
                        (SELECT COUNT(*) FROM messages m WHERE m.run_id = r.id AND m.role = 'USER')
                 FROM agent_runs r WHERE r.agent_id = (SELECT id FROM agents WHERE number = ?1)
                 ORDER BY r.seq",
            )
            .unwrap();
        let rows = stmt
            .query_map([number], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                ))
            })
            .unwrap();
        let mut out = String::from(
            "seq  modelo                        fin        1.er evento   total   handoff_tokens\n",
        );
        for (seq, model, end, start, ended, first, handoff) in rows.flatten() {
            out.push_str(&format!(
                "{seq:>3}  {model:<28}  {end:<10} {:>9} ms {:>7} ms   {}\n",
                first.map_or("-".into(), |f| (f - start).to_string()),
                ended - start,
                handoff.map_or("-".into(), |t| t.to_string()),
            ));
        }
        out
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = self.cli(&["daemon", "stop"]);
        let after = workspace_status();
        if after != self.workspace_before && !std::thread::panicking() {
            panic!(
                "el modelo escribió fuera de su worktree y ensució el repo de Symphony:\n{after}"
            );
        }
    }
}

fn create_chat(e: &Env, title: &str, model: &str) -> String {
    let v = e
        .ipc(
            "agent.create",
            json!({
                "title": title, "project_root": e.repo, "model": free(model),
                "failover": "NONE", "chat": true,
            }),
        )
        .expect("agent.create");
    v["agent_id"].as_str().unwrap().to_string()
}

/// Un modelo gratuito limita las solicitudes (429) de vez en cuando: el turno fallido se anota
/// y el agente se reabre con el mismo modelo (`symphony resume`) tras una pausa.
fn recover(e: &Env) {
    std::thread::sleep(Duration::from_secs(25));
    if e.state(1) == "FAILED" {
        let before = e.runs(1);
        let _ = e.cli(&["resume", "1"]);
        let _ = e.wait(1, &["READY", "COMPLETED"], before, 240);
    }
}

/// S8: lo que cuesta un mensaje que retoma la sesión (`resume`) frente a uno que cambia de
/// modelo con handoff, en el chat y con modelos gratuitos. n = 3 de cada uno. Es una medición:
/// pasa si al menos un resume y un handoff terminaron; imprime la tabla pase lo que pase.
#[test]
fn live_opencode_resume_vs_handoff() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    let e = Env::new();
    e.start();
    let agent = create_chat(&e, "Responde solo con la palabra OK.", A);
    e.wait(1, &["READY"], 0, 240).expect("primer turno");

    // (qué, ms de la orden a READY, ¿terminó bien?)
    let mut turns: Vec<(String, u128, Result<(), String>)> = Vec::new();
    let mut turn = |e: &Env, what: String, method: &str, params: Value| {
        let before = e.runs(1);
        let t = Instant::now();
        let r = e
            .ipc(method, params)
            .and_then(|_| e.wait(1, &["READY"], before, 240));
        let ms = t.elapsed().as_millis();
        if r.is_err() {
            recover(e);
        } else {
            std::thread::sleep(Duration::from_secs(5));
        }
        turns.push((what, ms, r));
    };
    for i in 1..=3 {
        turn(
            &e,
            format!("resume {i} ({A})"),
            "agent.send",
            json!({ "agent": agent, "text": format!("Responde solo con la palabra LISTO{i}.") }),
        );
    }
    for i in 1..=3 {
        for target in [B, A] {
            turn(
                &e,
                format!("handoff {i} -> {target}"),
                "agent.switch",
                json!({
                    "agent": agent, "model": free(target),
                    "message": format!("Responde solo con la palabra CAMBIO{i}."),
                }),
            );
        }
    }
    eprintln!("\n=== S8: resume frente a handoff (OpenCode, modelos gratuitos) ===");
    for (what, ms, r) in &turns {
        eprintln!(
            "{what:<40} {ms:>7} ms  {}",
            r.as_ref()
                .map_or_else(|e| format!("FALLO: {e}"), |()| "ok".into())
        );
    }
    eprintln!("\n{}", e.report(1));
    eprintln!(
        "TurnUsage (tokens de contexto por turno): {:?}",
        e.text("SELECT group_concat(json_extract(payload_json,'$.TurnUsage.context_tokens'), ',') FROM events WHERE type = 'TurnUsage'")
    );
    let ok = |prefix: &str| {
        turns
            .iter()
            .filter(|(w, _, r)| w.starts_with(prefix) && r.is_ok())
            .count()
    };
    assert!(ok("resume") >= 1, "ningún resume terminó");
    assert!(ok("handoff") >= 1, "ningún handoff terminó");
}

/// S9.1: el broker MCP de contexto. Un modelo real llama a `context_search` y recibe el
/// fragmento: queda un `context_retrievals` de tipo SEARCH y la respuesta nombra el archivo.
#[test]
fn live_opencode_context_broker() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    let e = Env::new();
    e.start();
    // El primer agente registra el proyecto y deja indexados sus archivos.
    let (ok, out) = e.cli(&[
        "spawn",
        "Responde solo con la palabra OK.",
        "--model",
        free(A),
        "--failover",
        "none",
    ]);
    assert!(ok, "{out}");
    e.wait(1, &["COMPLETED", "READY"], 0, 240).unwrap();
    let start = Instant::now();
    while e.num("SELECT COUNT(*) FROM context_chunks") == 0 {
        assert!(start.elapsed() < Duration::from_secs(60), "sin indexar");
        std::thread::sleep(Duration::from_millis(250));
    }
    let (ok, out) = e.cli(&[
        "spawn",
        "Usa la herramienta MCP context_search para buscar la palabra zafirocuantico42 y \
         responde solo con la ruta del archivo donde aparece. No leas archivos de otra forma.",
        "--model",
        free(A),
        "--failover",
        "none",
    ]);
    assert!(ok, "{out}");
    e.wait(2, &["COMPLETED", "READY"], 0, 300).unwrap();
    let searches = e.num("SELECT COUNT(*) FROM context_retrievals WHERE operation = 'SEARCH'");
    let found =
        e.num("SELECT COUNT(*) FROM context_retrievals WHERE operation = 'SEARCH' AND found = 1");
    let answer = e
        .text("SELECT content FROM messages WHERE role = 'ASSISTANT' ORDER BY created_at DESC LIMIT 1")
        .unwrap_or_default();
    eprintln!(
        "\n=== S9.1: broker MCP ===\nbúsquedas={searches} con_resultado={found}\nrespuesta: {answer}\n"
    );
    assert!(
        searches > 0,
        "el modelo no llamó a context_search (¿OpenCode no cargó el MCP?)"
    );
    assert!(found > 0, "context_search no encontró el archivo");
}

/// S9.2: handoff A → B (dos modelos de OpenCode) a media tarea, en los cuatro modos de contexto.
#[test]
fn live_opencode_handoff_in_every_context_mode() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    let mut summary = Vec::new();
    let mut passed = 0usize;
    for mode in ["raw", "safe", "balanced", "aggressive"] {
        let e = Env::new();
        e.start();
        let (ok, out) = e.cli(&[
            "spawn",
            TASK,
            "--model",
            free(A),
            "--failover",
            "none",
            "--context-mode",
            mode,
        ]);
        assert!(ok, "[{mode}] spawn: {out}");
        let wt: PathBuf = e
            .text("SELECT path FROM worktrees LIMIT 1")
            .expect("worktree")
            .into();
        let files = |wt: &Path| {
            (1..=4)
                .filter(|k| wt.join(format!("n{k}.txt")).is_file())
                .count()
        };
        // A trabaja hasta su primer archivo; ahí se lo corta (o termina antes: tarea corta).
        let t = Instant::now();
        while files(&wt) == 0 && t.elapsed() < Duration::from_secs(90) {
            if e.state(1) == "COMPLETED" {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if e.state(1) == "COMPLETED" {
            summary.push(format!(
                "{mode}: A terminó antes del corte (sin handoff que medir)"
            ));
            continue;
        }
        let at_cut = files(&wt);
        let before = e.runs(1);
        let (ok, out) = e.cli(&["switch", "1", "--model", free(B)]);
        if !ok {
            summary.push(format!("{mode}: switch falló: {out}"));
            continue;
        }
        let r = e.wait(1, &["COMPLETED"], before, 300);
        let done = files(&wt);
        let tokens = e.text("SELECT tokens_sent FROM handoffs ORDER BY created_at DESC LIMIT 1");
        let good = r.is_ok() && done == 4;
        passed += usize::from(good);
        summary.push(format!(
            "{mode}: corte con {at_cut}/4, termina con {done}/4, handoff={tokens:?} tokens, resultado={r:?}"
        ));
        // Un 429 del modelo gratuito no es un fallo de Symphony: pausa antes del siguiente modo.
        std::thread::sleep(Duration::from_secs(90));
    }
    eprintln!("\n=== S9.2: handoff por modo ===\n{}\n", summary.join("\n"));
    // El plan gratuito limita las solicitudes (429) cuando una tarea encadena varias: eso es una
    // medición inconclusa, no un fallo de Symphony (PLAN § P11.5.S9).
    let limited = summary.iter().filter(|s| s.contains("rate limit")).count();
    if passed < 2 && limited > 0 {
        eprintln!(
            "INCONCLUSO: {limited} modo(s) cortados por el límite de solicitudes del modelo gratuito"
        );
        return;
    }
    assert!(
        passed >= 2,
        "al menos dos modos deben completar la tarea tras el handoff: {summary:?}"
    );
}

/// S9.3: plan por fases con un CLI real. Se mide si el modelo escribe el plan y si Symphony
/// abre sesión nueva por fase (`PHASE_DONE`). No es un criterio de aprobación: el resultado es
/// la medición (un modelo gratuito puede no seguir la instrucción).
#[test]
fn live_opencode_phase_plan() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    let e = Env::new();
    e.start();
    let (ok, out) = e.cli(&[
        "spawn",
        "Tarea en dos fases. Fase 1: crea a.txt con el texto A y b.txt con el texto B. \
         Fase 2: crea c.txt con el texto C y d.txt con el texto D.",
        "--model",
        free(A),
        "--failover",
        "none",
    ]);
    assert!(ok, "{out}");
    let r = e.wait(1, &["COMPLETED", "FAILED", "BLOCKED"], 0, 420);
    let phases = e.num("SELECT COUNT(*) FROM agent_runs WHERE end_reason = 'PHASE_DONE'");
    let runs = e.runs(1);
    eprintln!(
        "\n=== S9.3: plan por fases ===\nestado={} runs={runs} fases_cerradas={phases} resultado={r:?}\n{}",
        e.state(1),
        e.report(1)
    );
    assert!(r.is_ok(), "{r:?}");
}

/// S9.4: el review agent con un CLI real devuelve el bloque `review` y Symphony lo guarda.
#[test]
fn live_opencode_review_block() {
    if !live() {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    let e = Env::new();
    e.start();
    let (ok, out) = e.cli(&[
        "spawn",
        "Crea el archivo saludo.txt con el texto hola.",
        "--model",
        free(A),
        "--failover",
        "none",
    ]);
    assert!(ok, "{out}");
    e.wait(1, &["COMPLETED"], 0, 240).unwrap();
    let (ok, out) = e.cli(&["review", "1", "--model", free(A)]);
    assert!(ok, "{out}");
    let start = Instant::now();
    let (status, rec, findings) = loop {
        let status = e
            .text("SELECT status FROM reviews LIMIT 1")
            .unwrap_or_default();
        if !status.is_empty() && status != "RUNNING" {
            break (
                status,
                e.text("SELECT recommendation FROM reviews LIMIT 1"),
                e.text("SELECT findings_json FROM reviews LIMIT 1"),
            );
        }
        assert!(
            start.elapsed() < Duration::from_secs(300),
            "la revisión no terminó"
        );
        std::thread::sleep(Duration::from_millis(500));
    };
    eprintln!(
        "\n=== S9.4: review ===\nstatus={status} recomendación={rec:?}\nhallazgos={findings:?}\n"
    );
    assert_eq!(
        status, "DONE",
        "el revisor no devolvió un bloque válido: {findings:?}"
    );
}
