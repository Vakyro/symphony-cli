//! P09.S8: `symphony mcp serve` habla MCP por stdio y reenvía al daemon real.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

mod common;
use common::{DaemonGuard, path_with_fake_claude, symphonyd};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
}

fn symphony(home: &Path, cwd: &Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_symphony"))
        .current_dir(cwd)
        .args(args)
        .env("SYMPHONY_HOME", home)
        .env("SYMPHONYD", symphonyd())
        .env("PATH", path_with_fake_claude(home))
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl Mcp {
    fn start(home: &Path, cwd: &Path, env: &[(&str, &str)]) -> Mcp {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_symphony"));
        cmd.current_dir(cwd)
            .args(["mcp", "serve"])
            .env("SYMPHONY_HOME", home)
            .env("SYMPHONYD", symphonyd())
            .env("PATH", path_with_fake_claude(home))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for l in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        let mut m = Mcp {
            child,
            stdin,
            lines,
            next_id: 0,
        };
        let init = m.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }),
        );
        assert!(
            init["result"]["capabilities"]["tools"].is_object(),
            "{init}"
        );
        m.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        m
    }

    fn send(&mut self, v: &Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self.lines.recv_timeout(left).expect("respuesta MCP");
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == json!(id) {
                return v;
            }
        }
    }

    /// `(es_error, texto)` de una llamada a herramienta.
    fn call(&mut self, tool: &str, args: Value) -> (bool, String) {
        let v = self.request("tools/call", json!({ "name": tool, "arguments": args }));
        if let Some(err) = v.get("error") {
            return (true, err["message"].as_str().unwrap_or("").to_string());
        }
        let r = &v["result"];
        let text = r["content"][0]["text"].as_str().unwrap_or("").to_string();
        (r["isError"].as_bool().unwrap_or(false), text)
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn mcp_broker_serves_context_tools_from_the_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let (home, repo) = (dir.path().join("home"), dir.path().join("repo"));
    std::fs::create_dir_all(repo.join("src")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@test.local"]);
    git(&repo, &["config", "user.name", "Tester"]);
    let body: String = (1..=12)
        .map(|i| format!("linea {i} rotarbroker\n"))
        .collect();
    std::fs::write(repo.join("src/auth.ts"), &body).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    let daemon = DaemonGuard::new(&home);

    let (ok, out, err) = symphony(
        &home,
        &repo,
        &["spawn", "Tarea", "--model", "claude/sonnet"],
    );
    assert!(ok, "{out}{err}");

    // Identidad del run, como la entrega el executor a los CLIs.
    let conn = symphony_store::open_reader(&home.join("symphony.db")).unwrap();
    let (agent, run, project): (String, String, String) = conn
        .query_row(
            "SELECT a.id, r.id, a.project_id FROM agents a JOIN agent_runs r ON r.agent_id = a.id LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    let mut mcp = Mcp::start(
        &home,
        &repo,
        &[
            ("SYMPHONY_AGENT_ID", &agent),
            ("SYMPHONY_RUN_ID", &run),
            ("SYMPHONY_PROJECT_ID", &project),
        ],
    );

    let tools = mcp.request("tools/list", json!({}));
    let mut names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["context_lines", "context_retrieve", "context_search"]
    );

    // El watcher indexa en segundo plano: se espera al primer acierto.
    let started = Instant::now();
    let found = loop {
        let (is_err, text) = mcp.call("context_search", json!({ "query": "rotarbroker" }));
        assert!(!is_err, "{text}");
        if text.contains("ctx://file/src/auth.ts") {
            break text;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "sin indexar: {text}"
        );
        std::thread::sleep(Duration::from_millis(200));
    };
    assert!(found.contains("líneas"), "{found}");

    let (is_err, all) = mcp.call(
        "context_retrieve",
        json!({ "uri": "ctx://file/src/auth.ts" }),
    );
    assert!(!is_err && all.trim_end() == body.trim_end(), "{all}");

    let (is_err, some) = mcp.call(
        "context_lines",
        json!({ "uri": "ctx://file/src/auth.ts", "from": 2, "to": 3 }),
    );
    assert!(!is_err, "{some}");
    assert_eq!(some, "linea 2 rotarbroker\nlinea 3 rotarbroker");

    let (_, past) = mcp.call(
        "context_lines",
        json!({ "uri": "ctx://file/src/auth.ts", "from": 50, "to": 60 }),
    );
    assert!(past.contains("12 líneas"), "{past}");

    // Una dirección que intenta escapar es un error de herramienta; el broker sigue vivo.
    let (is_err, msg) = mcp.call(
        "context_retrieve",
        json!({ "uri": "ctx://file/../../etc/passwd" }),
    );
    assert!(is_err && !msg.is_empty(), "{msg}");
    let (is_err, _) = mcp.call("context_search", json!({ "query": "rotarbroker" }));
    assert!(!is_err, "el broker sigue vivo");

    // Cada acceso quedó registrado para el run; el `lines` fuera de rango cuenta como miss.
    let (ok, stats, err) = symphony(&home, &repo, &["context", "stats"]);
    assert!(ok, "{stats}{err}");
    let line = stats
        .lines()
        .find(|l| l.contains("recuperaciones:"))
        .unwrap_or_default();
    assert!(line.contains("(1 sin resultado)"), "{stats}");

    let _ = daemon.stop();
}

/// ADR-0019: con `SYMPHONY_HARNESS` el broker suma las herramientas de archivos y shell, que
/// trabajan sobre el directorio del agente. No hace falta daemon: no consultan nada.
#[test]
fn mcp_broker_serves_the_harness_tools_when_the_agent_runs_in_the_harness() {
    let dir = tempfile::tempdir().unwrap();
    let (home, work) = (dir.path().join("home"), dir.path().join("work"));
    std::fs::create_dir_all(&work).unwrap();
    let mut mcp = Mcp::start(&home, &work, &[("SYMPHONY_HARNESS", "1")]);

    let tools = mcp.request("tools/list", json!({}));
    let mut names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "context_lines",
            "context_retrieve",
            "context_search",
            "edit",
            "glob",
            "grep",
            "read",
            "shell",
            "write"
        ]
    );

    // Rutas relativas: cuelgan del directorio del agente.
    let (err, out) = mcp.call(
        "write",
        json!({ "path": "notas/a.txt", "content": "uno\ndos\n" }),
    );
    assert!(!err, "{out}");
    assert_eq!(
        std::fs::read_to_string(work.join("notas/a.txt")).unwrap(),
        "uno\ndos\n"
    );
    let (err, out) = mcp.call(
        "edit",
        json!({ "path": "notas/a.txt", "old": "dos", "new": "DOS" }),
    );
    assert!(!err, "{out}");
    let (err, out) = mcp.call("read", json!({ "path": "notas/a.txt" }));
    assert!(!err && out.contains("     2\tDOS"), "{out}");
    let (err, out) = mcp.call("grep", json!({ "pattern": "DOS" }));
    assert!(!err && out.contains("notas/a.txt:2:DOS"), "{out}");
    let (err, out) = mcp.call("glob", json!({ "pattern": "**/*.txt" }));
    assert!(!err && out.trim() == "notas/a.txt", "{out}");
    let (err, out) = mcp.call("shell", json!({ "command": "echo desde-el-harness" }));
    assert!(!err && out.contains("desde-el-harness"), "{out}");
    // Un error del modelo (texto que no existe) vuelve como error de la herramienta.
    let (err, out) = mcp.call(
        "edit",
        json!({ "path": "notas/a.txt", "old": "zzz", "new": "y" }),
    );
    assert!(err && out.contains("no encontré"), "{out}");
}
