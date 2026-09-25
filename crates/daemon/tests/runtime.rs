//! P06.S1: creación de agentes (FLOW §6) — flujo feliz y cada rama de error.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use symphony_adapter_common::ProviderAdapter;
use symphony_core::{AgentState, ContextMode, FailoverPolicy};
use symphony_daemon::bus::EventBus;
use symphony_daemon::providers;
use symphony_daemon::runtime::{CreateAgent, CreateError, Execution, Runtime};
use symphony_object_store::ObjectStore;
use symphony_store::Writer;
use symphony_testkit::FakeAdapter;

fn fake_agent() -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let ok = Command::new(env!("CARGO"))
                .args([
                    "build",
                    "-q",
                    "-p",
                    "symphony-testkit",
                    "--bin",
                    "fake-agent",
                ])
                .status()
                .unwrap()
                .success();
            assert!(ok, "no se pudo construir fake-agent");
            PathBuf::from(env!("CARGO_BIN_EXE_symphonyd"))
                .with_file_name(format!("fake-agent{}", std::env::consts::EXE_SUFFIX))
        })
        .clone()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct Env {
    _dir: tempfile::TempDir,
    home: PathBuf,
    repo: PathBuf,
    db: PathBuf,
    writer: Writer,
    bus: EventBus,
    runtime: Runtime,
}

/// Home + repo con un commit + proveedor `fake` detectado con el guion dado.
/// `binary` reemplaza al fake-agent (para simular un CLI que no arranca).
async fn env(script: &str, binary: Option<PathBuf>) -> Env {
    env_with(&[("fake", script)], binary).await
}

/// Como `env`, con varios proveedores fake (`provider`, guion), en ese orden.
async fn env_with(providers_: &[(&'static str, &str)], binary: Option<PathBuf>) -> Env {
    env_with_watchdog(
        providers_,
        binary,
        Duration::from_secs(5),
        Duration::from_secs(600),
    )
    .await
}

async fn env_with_watchdog(
    providers_: &[(&'static str, &str)],
    binary: Option<PathBuf>,
    heartbeat_every: Duration,
    stale_after: Duration,
) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("README.md"), "demo\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);

    std::fs::create_dir_all(&home).unwrap();
    let db = home.join("symphony.db");
    let writer = Writer::start(&db).unwrap();
    let mut detected: Vec<Box<dyn ProviderAdapter>> = Vec::new();
    let mut used: Vec<Arc<dyn ProviderAdapter>> = Vec::new();
    for (provider, script) in providers_ {
        let script_path = dir.path().join(format!("{provider}.toml"));
        std::fs::write(&script_path, script).unwrap();
        detected.push(Box::new(
            FakeAdapter::new(fake_agent(), &script_path).with_provider(provider),
        ));
        used.push(Arc::new(
            FakeAdapter::new(binary.clone().unwrap_or_else(fake_agent), &script_path)
                .with_provider(provider),
        ));
    }
    providers::save(&writer.handle(), providers::detect_all(&detected), 1)
        .await
        .unwrap();
    let bus = EventBus::new(writer.handle(), 64, ObjectStore::new(home.join("objects")));
    let runtime = Runtime::new_with_watchdog(
        &home,
        writer.handle(),
        writer.reader().unwrap(),
        bus.clone(),
        used,
        None,
        symphony_daemon::runtime::WatchdogTiming {
            heartbeat_every,
            stale_after,
        },
    );
    Env {
        _dir: dir,
        home,
        repo,
        db,
        writer,
        bus,
        runtime,
    }
}

fn req(e: &Env, title: &str, execution: Execution) -> CreateAgent {
    CreateAgent {
        project_root: e.repo.clone(),
        title: title.into(),
        description: None,
        execution,
        failover: FailoverPolicy::Any,
        context_mode: ContextMode::Balanced,
        priority: 0,
    }
}

fn count(e: &Env, table: &str) -> i64 {
    symphony_store::open_reader(&e.db)
        .unwrap()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn one<T: rusqlite::types::FromSql>(e: &Env, sql: &str) -> T {
    symphony_store::open_reader(&e.db)
        .unwrap()
        .query_row(sql, [], |r| r.get(0))
        .unwrap()
}

/// Nada quedó a medias: ni filas, ni worktrees, ni ramas de agentes.
fn assert_nothing_created(e: &Env) {
    for t in ["tasks", "agents", "worktrees", "agent_runs", "checkpoints"] {
        assert_eq!(count(e, t), 0, "{t} no está vacía");
    }
    assert!(!e.home.join("worktrees").exists() || dir_is_empty_tree(&e.home.join("worktrees")));
    assert_eq!(git(&e.repo, &["branch", "--list", "symphony/*"]), "");
    assert_eq!(git(&e.repo, &["worktree", "list"]).lines().count(), 1);
}

fn dir_is_empty_tree(p: &Path) -> bool {
    std::fs::read_dir(p)
        .unwrap()
        .flatten()
        .all(|d| d.path().is_dir() && dir_is_empty_tree(&d.path()))
}

const WORK: &str = r#"
[[step]]
kind = "say"
text = "Arreglando"

[[step]]
kind = "edit"
path = "fix.txt"
content = "arreglado\n"
"#;

#[tokio::test(flavor = "multi_thread")]
async fn exact_model_creates_everything_and_runs_the_executor() {
    let e = env(WORK, None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Arreglar la rotación de tokens",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    assert_eq!((created.number, created.task_code.as_str()), (1, "T-1"));
    assert_eq!(created.state, AgentState::Running);
    let (run_id, model) = created.run.clone().unwrap();
    assert_eq!(model, "fake/fast");
    assert!(created.worktree.join("README.md").is_file());
    assert!(created.worktree.starts_with(e.home.join("worktrees")));

    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    // El executor trabajó en el worktree, no en el repo base.
    assert!(created.worktree.join("fix.txt").is_file());
    assert!(!e.repo.join("fix.txt").exists());
    assert_eq!(
        git(&created.worktree, &["branch", "--show-current"]),
        created.branch
    );
    let base = git(&e.repo, &["rev-parse", "HEAD"]);

    let id = created.agent_id.to_string();
    let q = |sql: &str| one::<String>(&e, &sql.replace("{a}", &id));
    assert_eq!(q("SELECT state FROM agents WHERE id='{a}'"), "COMPLETED");
    assert_eq!(
        q("SELECT execution_mode FROM agents WHERE id='{a}'"),
        "EXACT"
    );
    assert_eq!(
        q("SELECT requested_model_id FROM agents WHERE id='{a}'"),
        "fake/fast"
    );
    assert_eq!(
        q("SELECT t.status FROM tasks t JOIN agents a ON a.task_id=t.id WHERE a.id='{a}'"),
        "DONE"
    );
    // El modelo vive en el run (AGENT ≠ MODEL).
    assert_eq!(
        q(&format!(
            "SELECT model_id || '|' || status || '|' || end_reason || '|' || (cli_session_id IS NOT NULL) FROM agent_runs WHERE id='{run_id}'"
        )),
        "fake/fast|EXITED|COMPLETED|1"
    );
    // Checkpoint inicial: objetivo y commit base.
    assert_eq!(
        q(
            "SELECT seq || '|' || objective || '|' || head_commit FROM checkpoints WHERE agent_id='{a}' AND seq = 1"
        ),
        format!("1|Arreglar la rotación de tokens|{base}")
    );
    assert_eq!(q("SELECT status FROM worktrees"), "READY");
    assert_eq!(q("SELECT status FROM sessions"), "ACTIVE");
    // Los eventos del stream llegaron con el run.
    let started: i64 = one(
        &e,
        &format!(
            "SELECT COUNT(*) FROM events WHERE agent_id='{id}' AND run_id='{run_id}' AND type='AgentStarted'"
        ),
    );
    assert_eq!(started, 1);
    assert_eq!(count(&e, "recovery_items"), 0);

    // Un segundo agente del mismo proyecto: misma sesión, número y código siguientes.
    let second = e
        .runtime
        .create_agent(req(&e, "Otra cosa", Execution::DecideLater))
        .await
        .unwrap();
    assert_eq!((second.number, second.task_code.as_str()), (2, "T-2"));
    assert_eq!(count(&e, "sessions"), 1);
    assert_eq!(count(&e, "projects"), 1);
    assert_ne!(second.worktree, created.worktree);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn decide_later_creates_a_ready_agent_without_executor() {
    let e = env(WORK, None).await;
    let created = e
        .runtime
        .create_agent(req(&e, "Revisar el login", Execution::DecideLater))
        .await
        .unwrap();
    assert_eq!(created.state, AgentState::Ready);
    assert!(created.run.is_none());
    assert!(created.worktree.join("README.md").is_file());
    assert_eq!(count(&e, "agent_runs"), 0);
    assert_eq!(count(&e, "checkpoints"), 1);
    assert_eq!(one::<String>(&e, "SELECT status FROM tasks"), "READY");
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn unavailable_exact_model_asks_the_user_and_creates_nothing() {
    let e = env(WORK, None).await;
    let err = e
        .runtime
        .create_agent(req(
            &e,
            "Tarea importante",
            Execution::Exact("fake/ultra".into()),
        ))
        .await
        .unwrap_err();
    let CreateError::ExactModelUnavailable {
        model,
        reason,
        task,
    } = &err
    else {
        panic!("{err:?}")
    };
    assert_eq!(
        (model.as_str(), task.as_str()),
        ("fake/ultra", "Tarea importante")
    );
    assert!(reason.contains("no existe"), "{reason}");
    for option in ["esperar", "escoger otro modelo", "profile", "cancelar"] {
        assert!(err.to_string().contains(option), "{err}");
    }
    assert_nothing_created(&e);

    // Proveedor deshabilitado por el usuario: mismo camino.
    symphony_store::open(&e.db)
        .unwrap()
        .execute("UPDATE providers SET enabled = 0", [])
        .unwrap();
    let err = e
        .runtime
        .create_agent(req(
            &e,
            "Tarea importante",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, CreateError::ExactModelUnavailable { reason, .. } if reason.contains("deshabilitado")),
        "{err:?}"
    );
    assert_nothing_created(&e);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn no_ready_provider_keeps_the_task_text() {
    let e = env(WORK, None).await;
    symphony_store::open(&e.db)
        .unwrap()
        .execute("UPDATE providers SET setup_state = 'NOT_FOUND'", [])
        .unwrap();
    let err = e
        .runtime
        .create_agent(req(&e, "No perder esto", Execution::DecideLater))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, CreateError::NoEligibleProvider { task } if task == "No perder esto"),
        "{err:?}"
    );
    assert_eq!(err.code(), "no_eligible_provider");
    // Con modelo exacto, el motivo nombra al proveedor.
    let err = e
        .runtime
        .create_agent(req(
            &e,
            "No perder esto",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, CreateError::ExactModelUnavailable { reason, .. } if reason.contains("no está listo (NOT_FOUND)")),
        "{err:?}"
    );
    assert_nothing_created(&e);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_requests_are_rejected_before_touching_anything() {
    let e = env(WORK, None).await;
    let err = e
        .runtime
        .create_agent(req(&e, "   ", Execution::DecideLater))
        .await
        .unwrap_err();
    assert!(matches!(err, CreateError::EmptyTask), "{err:?}");

    let err = e
        .runtime
        .create_agent(req(&e, "Con profile", Execution::Profile("@code".into())))
        .await
        .unwrap_err();
    assert!(
        matches!(err, CreateError::ProfilesNotSupported { .. }),
        "{err:?}"
    );

    let mut not_repo = req(&e, "Fuera de git", Execution::DecideLater);
    not_repo.project_root = e.home.clone();
    let err = e.runtime.create_agent(not_repo).await.unwrap_err();
    assert!(matches!(err, CreateError::NotARepo { .. }), "{err:?}");
    assert_nothing_created(&e);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn workspace_failure_leaves_no_half_built_agent() {
    let e = env(WORK, None).await;
    // La rama del agente ya existe: `git worktree add -b` falla.
    let clash = e
        .runtime
        .create_agent(req(&e, "Primero", Execution::DecideLater))
        .await
        .unwrap();
    std::fs::remove_dir_all(e.home.join("worktrees")).unwrap();
    git(&e.repo, &["worktree", "prune"]);
    symphony_store::open(&e.db)
        .unwrap()
        .execute_batch("DELETE FROM checkpoints; DELETE FROM agents; DELETE FROM worktrees; DELETE FROM tasks;")
        .unwrap();
    // Queda la rama `…/agent-001` del intento anterior, sin agente.
    assert_eq!(
        git(&e.repo, &["branch", "--list", "symphony/*"]).trim_start_matches(['*', '+', ' ']),
        clash.branch
    );

    let err = e
        .runtime
        .create_agent(req(&e, "Segundo", Execution::DecideLater))
        .await
        .unwrap_err();
    let CreateError::Workspace { detail, task } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(task, "Segundo");
    assert!(
        detail.contains("already exists") || detail.contains("ya existe"),
        "{detail}"
    );
    assert!(err.to_string().contains("reintentar o cancelar"), "{err}");
    for t in ["tasks", "agents", "worktrees", "checkpoints"] {
        assert_eq!(count(&e, t), 0, "{t}");
    }
    assert!(dir_is_empty_tree(&e.home.join("worktrees")));
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn store_failure_after_the_worktree_rolls_it_back() {
    let e = env(WORK, None).await;
    symphony_store::open(&e.db)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER boom BEFORE INSERT ON agents BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    let err = e
        .runtime
        .create_agent(req(
            &e,
            "Se cae la base",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, CreateError::Store(m) if m.contains("boom")),
        "{err:?}"
    );
    // La transacción se deshizo entera y el worktree/rama también.
    assert_eq!(count(&e, "projects"), 0);
    assert_eq!(count(&e, "sessions"), 0);
    assert_nothing_created(&e);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn executor_that_cannot_start_leaves_a_failed_agent_with_a_reason() {
    let missing = std::env::temp_dir().join("symphony-no-such-cli-xyz");
    let e = env(WORK, Some(missing)).await;
    let created = e
        .runtime
        .create_agent(req(&e, "Lanzar", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    assert_eq!(created.state, AgentState::Failed);
    let reason = created.state_reason.unwrap();
    assert!(reason.contains("no se pudo lanzar fake-agent"), "{reason}");
    e.writer.handle().flush().await.unwrap();
    let id = created.agent_id.to_string();
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT state || '|' || state_reason FROM agents WHERE id='{id}'")
        ),
        format!("FAILED|{reason}")
    );
    assert_eq!(
        one::<String>(&e, "SELECT status || '|' || end_reason FROM agent_runs"),
        "FAILED|CRASH"
    );
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT kind FROM recovery_items WHERE agent_id='{id}'")
        ),
        "EXECUTOR_EXITED"
    );
    // El workspace y el checkpoint quedan para reintentar.
    assert!(created.worktree.join("README.md").is_file());
    assert_eq!(count(&e, "checkpoints"), 1);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn executor_crash_fails_the_agent_and_opens_recovery() {
    let e = env(
        "[[step]]\nkind = \"say\"\ntext = \"hola\"\n\n[[step]]\nkind = \"crash\"\n",
        None,
    )
    .await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Se va a caer",
            Execution::Exact("fake/smart".into()),
        ))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let id = created.agent_id.to_string();
    let state: String = one(
        &e,
        &format!("SELECT state || '|' || state_reason FROM agents WHERE id='{id}'"),
    );
    assert!(state.starts_with("FAILED|fake-agent terminó"), "{state}");
    assert_eq!(
        one::<String>(&e, "SELECT status || '|' || end_reason FROM agent_runs"),
        "FAILED|CRASH"
    );
    assert_eq!(count(&e, "recovery_items"), 1);
    // Ningún agente con dos runs abiertos.
    assert_eq!(
        one::<i64>(&e, "SELECT COUNT(*) FROM agent_runs WHERE ended_at IS NULL"),
        0
    );
    e.writer.shutdown();
}

/// P06.S2: una sesión deja la conversación y las tool calls coherentes.
#[tokio::test(flavor = "multi_thread")]
async fn session_mirrors_conversation_and_tool_calls() {
    let long = "x".repeat(symphony_daemon::recorder::INLINE_MAX + 100);
    let script = format!(
        r#"
[[step]]
kind = "say"
text = "Primero miro el estado"

[[step]]
kind = "run"
command = ["git", "status", "--short"]

[[step]]
kind = "edit"
path = "src/fix.txt"
content = "ok\n"

[[step]]
kind = "run"
command = ["git", "no-such-subcommand"]

[[step]]
kind = "say"
text = "{long}"
"#
    );
    let e = env(&script, None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Arreglar el build",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let run_id = created.run.unwrap().0.to_string();
    let conn = symphony_store::open_reader(&e.db).unwrap();

    // Conversación: prompt, texto corto y texto largo (al object store), en orden y con su run.
    type Msg = (String, Option<String>, Option<String>, Option<String>);
    let msgs: Vec<Msg> = conn
        .prepare(
            "SELECT role, content, content_object_id, run_id FROM messages ORDER BY created_at, rowid",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let roles: Vec<&str> = msgs.iter().map(|m| m.0.as_str()).collect();
    assert_eq!(roles, ["USER", "ASSISTANT", "ASSISTANT"], "{msgs:?}");
    assert_eq!(msgs[0].1.as_deref(), Some("Arreglar el build"));
    assert_eq!(msgs[1].1.as_deref(), Some("Primero miro el estado"));
    assert!(msgs.iter().all(|m| m.3.as_deref() == Some(run_id.as_str())));
    let (_, content, object, _) = &msgs[2];
    assert!(content.is_none());
    let (kind, hash, refs): (String, String, i64) = conn
        .query_row(
            "SELECT o.kind, o.blob_hash, b.ref_count FROM context_objects o
             JOIN blobs b ON b.hash = o.blob_hash WHERE o.id = ?1",
            [object.as_deref().unwrap()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((kind.as_str(), refs), ("CONVERSATION", 1));
    let stored = ObjectStore::new(e.home.join("objects")).get(&hash).unwrap();
    assert_eq!(stored, long.as_bytes());

    // Tool calls: una por herramienta, todas cerradas, con resultado y comando.
    type Call = (String, Option<String>, String, Option<i32>, i64, bool);
    let calls: Vec<Call> = conn
        .prepare(
            "SELECT tool_name, command, status, exit_code, op_class, finished_at >= started_at
             FROM tool_calls WHERE run_id = ?1 ORDER BY requested_at, rowid",
        )
        .unwrap()
        .query_map([&run_id], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let summary: Vec<(&str, Option<&str>, &str)> = calls
        .iter()
        .map(|c| (c.0.as_str(), c.1.as_deref(), c.2.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            ("Bash", Some("git status --short"), "DONE"),
            ("Write", None, "DONE"),
            ("Bash", Some("git no-such-subcommand"), "FAILED"),
        ]
    );
    assert_eq!(calls[0].3, Some(0));
    assert!(calls[2].3.is_some_and(|c| c != 0), "{calls:?}");
    assert!(
        calls.iter().all(|c| c.5),
        "toda tool call cerrada: {calls:?}"
    );
    assert_eq!((calls[0].4, calls[1].4), (2, 1));
    e.writer.shutdown();
}

// --- P06.S3: checkpoints incrementales --------------------------------------

type CkRow = (
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    String,
);

/// (seq, plan_tail, current_step, next_step, head_commit, diff_object_id, summary_json), por seq.
fn checkpoints_of(e: &Env, agent: &str) -> Vec<CkRow> {
    symphony_store::open_reader(&e.db)
        .unwrap()
        .prepare(
            "SELECT seq, plan_tail, current_step, next_step, head_commit, diff_object_id, summary_json
             FROM checkpoints WHERE agent_id = ?1 ORDER BY seq",
        )
        .unwrap()
        .query_map([agent], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn diff_text(e: &Env, object: &str) -> String {
    let hash: String = one(
        e,
        &format!("SELECT blob_hash FROM context_objects WHERE id = '{object}'"),
    );
    let bytes = ObjectStore::new(e.home.join("objects")).get(&hash).unwrap();
    String::from_utf8(bytes).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn session_leaves_an_up_to_date_checkpoint() {
    let script = r#"
[[step]]
kind = "say"
text = "Plan:\n- [x] leer\n- [ ] correr los tests de auth\nToken de prueba: Bearer sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123"

[[step]]
kind = "edit"
path = "README.md"
content = "demo\narreglado\n"

[[step]]
kind = "edit"
path = "src/nuevo.txt"
content = "nuevo\n"

[[step]]
kind = "run"
command = ["git", "no-such-subcommand"]
"#;
    let e = env(script, None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Arreglar auth",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let id = created.agent_id.to_string();
    let rows = checkpoints_of(&e, &id);
    assert!(rows.len() >= 2, "hubo checkpoints incrementales: {rows:?}");
    let seqs: Vec<i64> = rows.iter().map(|r| r.0).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");

    let (_, plan_tail, current, next, head, diff, summary) = rows.last().unwrap().clone();
    let base = git(&e.repo, &["rev-parse", "HEAD"]);
    assert_eq!(head, base);
    assert_eq!(next.as_deref(), Some("correr los tests de auth"));
    let plan_tail = plan_tail.unwrap();
    assert!(
        plan_tail.contains("correr los tests de auth"),
        "{plan_tail}"
    );
    assert!(
        !plan_tail.contains("sk-ant-api03"),
        "plan sin redactar: {plan_tail}"
    );
    assert_eq!(current.as_deref(), Some("Bash: git no-such-subcommand"));
    let diff = diff_text(&e, &diff.unwrap());
    assert!(diff.contains("+arreglado"), "{diff}");
    let summary: serde_json::Value = serde_json::from_str(&summary).unwrap();
    let files: Vec<&str> = summary["files_touched"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(files, ["README.md", "src/nuevo.txt"], "{summary}");
    assert_eq!(summary["files_touched"][1]["untracked"], true);
    assert_eq!(summary["commands_run"], 1);
    assert_eq!(summary["last_command"]["ok"], false);
    let failure = summary["failures"][0].as_str().unwrap();
    assert!(
        failure.starts_with("`git no-such-subcommand` falló"),
        "{failure}"
    );
    e.writer.shutdown();
}

#[derive(Debug, Clone)]
enum Op {
    /// Escribe `content` en uno de 3 archivos (el 0 es el README versionado) y avisa la edición.
    Edit {
        file: usize,
        content: u8,
    },
    Say {
        next: bool,
    },
    Command {
        ok: bool,
    },
    Turn,
    /// Evento que no amerita checkpoint.
    Noise,
}

fn op() -> impl proptest::strategy::Strategy<Value = Op> {
    use proptest::prelude::*;
    prop_oneof![
        3 => (0..3usize, 0..3u8).prop_map(|(file, content)| Op::Edit { file, content }),
        1 => any::<bool>().prop_map(|next| Op::Say { next }),
        2 => any::<bool>().prop_map(|ok| Op::Command { ok }),
        1 => Just(Op::Turn),
        1 => Just(Op::Noise),
    ]
}

/// 50 eventos → checkpoints monótonos por `seq`, ninguno referencia objetos
/// inexistentes, refs de blobs coherentes y retención de los usados en handoffs.
async fn run_checkpoint_case(ops: Vec<Op>) {
    use symphony_adapter_common::{AgentEvent, ToolKind};
    use symphony_daemon::bus::{BusEvent, EventSource};
    use symphony_daemon::checkpoint::KEEP;

    let e = env(WORK, None).await;
    let created = e
        .runtime
        .create_agent(req(&e, "Propiedad", Execution::DecideLater))
        .await
        .unwrap();
    let agent = created.agent_id.to_string();
    let run = symphony_core::RunId::new().to_string();
    // Un run y un handoff que usa el checkpoint inicial: no se puede podar.
    symphony_store::open(&e.db)
        .unwrap()
        .execute_batch(&format!(
            "INSERT INTO agent_runs (id, agent_id, seq, provider_id, model_id, status, started_at)
                 VALUES ('{run}','{agent}',1,'fake','fake/fast','RUNNING',0);
             INSERT INTO handoffs (id, agent_id, checkpoint_id, to_run_id, mode, created_at)
                 SELECT 'h1', agent_id, id, '{run}', 'BALANCED', 0 FROM checkpoints WHERE agent_id='{agent}';"
        ))
        .unwrap();
    let project: String = one(&e, "SELECT id FROM projects");
    let files = ["README.md", "a.txt", "dir/b.txt"];
    let publish = |event| {
        e.bus.publish(BusEvent {
            project_id: project.clone(),
            agent_id: Some(agent.clone()),
            run_id: Some(run.clone()),
            source: EventSource::Hook,
            event,
            occurred_at: 1,
        })
    };
    let mut significant = 0;
    for op in &ops {
        let events = match op {
            Op::Edit { file, content } => {
                let path = created.worktree.join(files[*file]);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                // content 0 en el README = contenido original (diff vacío si no hay más cambios).
                let text = if *content == 0 && *file == 0 {
                    "demo\n".to_string()
                } else {
                    format!("v{content}\n")
                };
                std::fs::write(&path, text).unwrap();
                vec![AgentEvent::FileModified {
                    tool: "Write".into(),
                    path: Some(files[*file].into()),
                }]
            }
            Op::Say { next } => vec![AgentEvent::AssistantText {
                text: if *next {
                    "Siguiente: seguir".into()
                } else {
                    "pensando".into()
                },
            }],
            Op::Command { ok } => vec![
                AgentEvent::ToolRequested {
                    tool_use_id: None,
                    tool: "Bash".into(),
                    kind: ToolKind::Command,
                    command: Some("npm test".into()),
                },
                AgentEvent::ToolFinished {
                    tool_use_id: None,
                    tool: "Bash".into(),
                    kind: ToolKind::Command,
                    ok: *ok,
                    exit_code: Some(i32::from(!ok)),
                },
            ],
            Op::Turn => vec![AgentEvent::TurnFinished { last_message: None }],
            Op::Noise => vec![AgentEvent::TurnStarted],
        };
        for event in events {
            if matches!(
                event,
                AgentEvent::FileModified { .. }
                    | AgentEvent::ToolFinished { .. }
                    | AgentEvent::TurnFinished { .. }
            ) {
                significant += 1;
            }
            publish(event).await.unwrap();
            // Sin agrupar: un checkpoint por evento significativo, para contar exacto.
            e.bus.checkpoints_idle().await;
        }
    }
    e.writer.handle().flush().await.unwrap();

    let rows = checkpoints_of(&e, &agent);
    let seqs: Vec<i64> = rows.iter().map(|r| r.0).collect();
    let created_total = 1 + significant;
    assert_eq!(*seqs.last().unwrap(), created_total, "{seqs:?}");
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
    assert_eq!(seqs[0], 1, "el checkpoint del handoff se conserva");
    let expected = if created_total <= KEEP as i64 {
        created_total
    } else {
        KEEP as i64 + 1
    };
    assert_eq!(rows.len() as i64, expected, "{seqs:?}");

    // Ningún checkpoint referencia objetos inexistentes; refs de blobs coherentes.
    let conn = symphony_store::open_reader(&e.db).unwrap();
    let dangling: i64 = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM checkpoints c WHERE c.diff_object_id IS NOT NULL
                       AND NOT EXISTS (SELECT 1 FROM context_objects o WHERE o.id = c.diff_object_id))
                  + (SELECT COUNT(*) FROM checkpoint_refs r
                       WHERE NOT EXISTS (SELECT 1 FROM context_objects o WHERE o.id = r.object_id))
                  + (SELECT COUNT(*) FROM context_objects o
                       WHERE NOT EXISTS (SELECT 1 FROM blobs b WHERE b.hash = o.blob_hash))",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dangling, 0);
    let orphans: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM context_objects o WHERE o.kind = 'GIT_DIFF'
             AND NOT EXISTS (SELECT 1 FROM checkpoints c WHERE c.diff_object_id = o.id)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(orphans, 0, "diffs sin checkpoint quedaron sin podar");
    let bad_refs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM blobs b WHERE b.ref_count <>
                 (SELECT COUNT(*) FROM context_objects o WHERE o.blob_hash = b.hash)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(bad_refs, 0, "ref_count de blobs incoherente");
    let store = ObjectStore::new(e.home.join("objects"));
    for (_, _, _, _, _, diff, _) in &rows {
        if let Some(object) = diff {
            let hash: String = one(
                &e,
                &format!("SELECT blob_hash FROM context_objects WHERE id = '{object}'"),
            );
            store.get(&hash).unwrap();
        }
    }
    // El último checkpoint refleja el diff real del worktree.
    let real = git(
        &created.worktree,
        &[
            "diff",
            "--no-ext-diff",
            "--no-color",
            &git(&e.repo, &["rev-parse", "HEAD"]),
        ],
    );
    let last = rows.last().unwrap();
    if created_total > 1 {
        match &last.5 {
            Some(object) => assert_eq!(diff_text(&e, object).trim_end(), real),
            None => assert_eq!(real, ""),
        }
    }
    e.writer.shutdown();
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 4, ..Default::default() })]
    #[test]
    fn checkpoints_stay_monotonic_and_consistent(ops in proptest::collection::vec(op(), 50)) {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run_checkpoint_case(ops));
    }
}

// --- P06.S4: handoff v1 ------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn handoff_combines_the_checkpoint_with_live_git() {
    let script = r#"
[[step]]
kind = "say"
text = "Voy por partes.\n- [x] leer\n- [ ] correr los tests de auth"

[[step]]
kind = "edit"
path = "README.md"
content = "demo\narreglado\n"

[[step]]
kind = "edit"
path = "src/nuevo.txt"
content = "contenido nuevo\n"

[[step]]
kind = "run"
command = ["git", "no-such-subcommand"]
"#;
    let e = env(script, None).await;
    let objective = "Arreglar auth";
    let created = e
        .runtime
        .create_agent(req(&e, objective, Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let (run, _) = created.run.clone().unwrap();

    // El primer spawn quedó registrado como handoff sin checkpoint.
    let first: String = one(
        &e,
        &format!(
            "SELECT COALESCE(checkpoint_id,'-') || '|' || mode || '|' || tokens_sent || '|' || tokens_raw_estimate
             FROM handoffs WHERE to_run_id = '{run}'"
        ),
    );
    let tokens = symphony_context::handoff::estimate_tokens(objective);
    assert_eq!(first, format!("-|BALANCED|{tokens}|{tokens}"));

    // Algo que pasó después del último checkpoint: git manda.
    std::fs::write(created.worktree.join("tardio.txt"), "escrito tarde\n").unwrap();

    let h = e
        .runtime
        .prepare_handoff(created.agent_id, "El executor anterior se quedó sin cuota.")
        .await
        .unwrap();
    let latest: String = one(
        &e,
        &format!(
            "SELECT id FROM checkpoints WHERE agent_id = '{}' ORDER BY seq DESC LIMIT 1",
            created.agent_id
        ),
    );
    assert_eq!(h.checkpoint_id.map(|c| c.to_string()), Some(latest));
    assert_eq!(h.mode, ContextMode::Balanced);
    let p = &h.prompt;
    for want in [
        "El executor anterior se quedó sin cuota.",
        "## Objetivo\nArreglar auth",
        "## Qué seguía\ncorrer los tests de auth",
        "$ git no-such-subcommand\n(falló con código",
        "- `git no-such-subcommand` falló",
        " M README.md",
        "?? src/nuevo.txt",
        "+arreglado",
        "--- src/nuevo.txt (archivo nuevo)\ncontenido nuevo",
        "--- tardio.txt (archivo nuevo)\nescrito tarde",
    ] {
        assert!(p.contains(want), "falta {want:?} en:\n{p}");
    }
    assert_eq!(h.tokens_sent, symphony_context::handoff::estimate_tokens(p));
    assert!(h.tokens_raw_estimate > h.tokens_sent, "{h:?}");
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn quota_exhausted_hands_the_same_agent_to_the_next_provider() {
    let first = r#"
[[step]]
kind = "edit"
path = "from-alpha.txt"
content = "kept\n"
[[step]]
kind = "say"
text = "Next: continue with beta"
[[step]]
kind = "quota_exhausted"
resets_at = 1790300000
"#;
    let second = r#"
[[step]]
kind = "edit"
path = "from-beta.txt"
content = "continued\n"
"#;
    let e = env_with(&[("alpha", first), ("beta", second)], None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "continuar tras cuota",
            Execution::Exact("alpha/fast".into()),
        ))
        .await
        .unwrap();

    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    let conn = symphony_store::open_reader(&e.db).unwrap();
    let runs: Vec<(String, String, String, Option<String>)> = conn
        .prepare("SELECT provider_id, model_id, status, end_reason FROM agent_runs ORDER BY seq")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        runs,
        vec![
            (
                "alpha".into(),
                "alpha/fast".into(),
                "HANDED_OFF".into(),
                Some("QUOTA_EXHAUSTED".into()),
            ),
            (
                "beta".into(),
                "beta/fast".into(),
                "EXITED".into(),
                Some("COMPLETED".into()),
            ),
        ]
    );
    assert_eq!(count(&e, "agents"), 1);
    assert_eq!(count(&e, "tasks"), 1);
    assert_eq!(count(&e, "worktrees"), 1);
    assert_eq!(count(&e, "provider_failures"), 1);
    assert_eq!(count(&e, "executor_changes"), 1);
    assert_eq!(count(&e, "handoffs"), 2);
    assert_eq!(
        one::<i64>(
            &e,
            "SELECT COUNT(*) FROM handoffs WHERE outcome = 'CONTINUED'"
        ),
        2
    );
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "COMPLETED");
    assert_eq!(one::<String>(&e, "SELECT status FROM tasks"), "DONE");
    assert_eq!(
        one::<String>(&e, "SELECT reason FROM executor_changes"),
        "FAILOVER"
    );
    let separator: String = one(
        &e,
        "SELECT content FROM messages WHERE role = 'EXECUTOR_CHANGE'",
    );
    assert!(separator.contains("alpha / alpha/fast → beta / beta/fast"));
    assert!(separator.contains("Agent #1, task and workspace unchanged"));
    assert!(created.worktree.join("from-alpha.txt").is_file());
    assert!(created.worktree.join("from-beta.txt").is_file());
    assert_eq!(
        git(&created.worktree, &["branch", "--show-current"]),
        created.branch
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn disabled_failover_waits_for_a_provider_and_opens_recovery() {
    let script = "[[step]]\nkind = \"quota_exhausted\"\n";
    let e = env(script, None).await;
    let mut request = req(
        &e,
        "esperar tras cuota",
        Execution::Exact("fake/fast".into()),
    );
    request.failover = FailoverPolicy::None;
    e.runtime.create_agent(request).await.unwrap();

    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    assert_eq!(count(&e, "agent_runs"), 1);
    assert_eq!(count(&e, "provider_failures"), 1);
    assert_eq!(count(&e, "executor_changes"), 1);
    assert_eq!(count(&e, "recovery_items"), 1);
    assert_eq!(
        one::<String>(&e, "SELECT state FROM agents"),
        "WAITING_PROVIDER"
    );
    assert_eq!(
        one::<String>(&e, "SELECT kind FROM recovery_items"),
        "RATE_LIMITED"
    );
    assert_eq!(
        one::<i64>(
            &e,
            "SELECT COUNT(*) FROM executor_changes WHERE to_run_id IS NULL"
        ),
        1
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn manual_switch_replaces_only_the_executor_and_remembers_the_model() {
    let first = "[[step]]\nkind = \"hang\"\n";
    let second = "[[step]]\nkind = \"edit\"\npath = \"switched.txt\"\ncontent = \"ok\\n\"\n";
    let e = env_with(&[("alpha", first), ("beta", second)], None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "cambio manual",
            Execution::Exact("alpha/fast".into()),
        ))
        .await
        .unwrap();
    let new_run = e
        .runtime
        .switch(created.agent_id, "beta/smart")
        .await
        .unwrap();

    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    assert_eq!(count(&e, "agents"), 1);
    assert_eq!(count(&e, "tasks"), 1);
    assert_eq!(count(&e, "worktrees"), 1);
    assert_eq!(count(&e, "agent_runs"), 2);
    assert_eq!(count(&e, "executor_changes"), 1);
    assert_eq!(
        one::<i64>(
            &e,
            "SELECT COUNT(*) FROM handoffs WHERE outcome = 'CONTINUED'"
        ),
        2
    );
    let conn = symphony_store::open_reader(&e.db).unwrap();
    let changed: (String, String, String) = conn
        .query_row(
            "SELECT reason, from_run_id, to_run_id FROM executor_changes",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(changed.0, "USER_SWITCH");
    assert_eq!(changed.2, new_run.to_string());
    assert_ne!(changed.1, changed.2);
    assert_eq!(
        one::<String>(&e, "SELECT requested_model_id FROM agents"),
        "beta/smart"
    );
    assert!(created.worktree.join("switched.txt").is_file());
    assert_eq!(
        git(&created.worktree, &["branch", "--show-current"]),
        created.branch
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn hung_executor_is_failed_and_reclaim_keeps_its_workspace() {
    let script = r#"
[[step]]
kind = "edit"
path = "before-hang.txt"
content = "safe\n"
[[step]]
kind = "hang"
"#;
    let e = env_with_watchdog(
        &[("fake", script)],
        None,
        Duration::from_millis(100),
        Duration::from_millis(1500),
    )
    .await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "recuperar cuelgue",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();

    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(
        one::<String>(&e, "SELECT end_reason FROM agent_runs"),
        "NO_HEARTBEAT"
    );
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "FAILED");
    assert_eq!(
        one::<String>(&e, "SELECT kind FROM recovery_items"),
        "NO_HEARTBEAT"
    );
    assert!(one::<Option<i64>>(&e, "SELECT last_heartbeat_at FROM agent_runs").is_some());
    assert!(created.worktree.join("before-hang.txt").is_file());

    e.runtime.reclaim(created.agent_id).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(count(&e, "agent_runs"), 2);
    assert_eq!(
        one::<String>(&e, "SELECT reason FROM executor_changes"),
        "RECLAIM"
    );
    assert_eq!(
        one::<i64>(
            &e,
            "SELECT COUNT(*) FROM recovery_items WHERE status = 'RESOLVED' AND resolution = 'RECLAIM'"
        ),
        1
    );
    assert!(created.worktree.join("before-hang.txt").is_file());
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn crashed_executor_can_restart_from_its_checkpoint() {
    let e = env("[[step]]\nkind = \"crash\"\n", None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "reiniciar crash",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    e.runtime.wait_executors().await;

    e.runtime.restart(created.agent_id).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    assert_eq!(count(&e, "agent_runs"), 2);
    assert_eq!(
        one::<String>(&e, "SELECT reason FROM executor_changes"),
        "RESTART"
    );
    assert_eq!(
        one::<i64>(
            &e,
            "SELECT COUNT(*) FROM recovery_items WHERE status = 'RESOLVED' AND resolution = 'RESTART'"
        ),
        1
    );
    assert_eq!(
        git(&created.worktree, &["branch", "--show-current"]),
        created.branch
    );
    e.writer.shutdown();
}

/// P06.S8: Prueba de aceptación forced kill (IDEA §8, Test D).
/// fake-agent A trabaja y se cuelga/muere sin cleanup;
/// fake-agent B continúa solo con el handoff estructurado;
/// se verifica que la tarea se completa y todos los artefactos y estados quedan íntegros.
#[tokio::test(flavor = "multi_thread")]
async fn forced_kill_test_d_acceptance_test() {
    let script_a = r#"
[[step]]
kind = "edit"
path = "email.js"
content = "export function validateEmail(e) { return e.includes('@'); }\n"
[[step]]
kind = "edit"
path = "password.js"
content = "export function hashPassword(p) { return 'hash:' + p; }\n"
[[step]]
kind = "say"
text = "Creados módulos email y password.\nNext: implementar UserStore en users.js y tests en users_test.js"
[[step]]
kind = "hang"
"#;
    let script_b = r#"
[[step]]
kind = "edit"
path = "users.js"
content = "import { validateEmail } from './email.js';\nexport class UserStore { constructor() { this.users = []; } }\n"
[[step]]
kind = "edit"
path = "users_test.js"
content = "import { UserStore } from './users.js';\n// tests passed\n"
[[step]]
kind = "say"
text = "Tarea completada con éxito: módulo users y tests agregados."
"#;

    let e = env_with_watchdog(
        &[("claude", script_a), ("codex", script_b)],
        None,
        Duration::from_millis(100),
        Duration::from_millis(1500),
    )
    .await;

    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Implementar sistema de usuarios con autenticación",
            Execution::Exact("claude/fast".into()),
        ))
        .await
        .unwrap();

    // 1. fake-agent A trabaja, crea archivos y se cuelga. El watchdog lo mata forzosamente.
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    // Comprobamos que el run 1 murió por NO_HEARTBEAT y el agente quedó en FAILED
    assert_eq!(
        one::<String>(&e, "SELECT end_reason FROM agent_runs WHERE seq = 1"),
        "NO_HEARTBEAT"
    );
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "FAILED");
    assert!(created.worktree.join("email.js").is_file());
    assert!(created.worktree.join("password.js").is_file());

    // 2. fake-agent B (Codex) retoma el trabajo mediante switch con handoff automático
    let _new_run = e
        .runtime
        .switch(created.agent_id, "codex/fast")
        .await
        .unwrap();

    // 3. fake-agent B corre con el handoff recibido y completa la tarea
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    // 4. Verificaciones completas de aceptación (Test D)
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "COMPLETED");
    assert_eq!(one::<String>(&e, "SELECT status FROM tasks"), "DONE");
    assert_eq!(count(&e, "agent_runs"), 2);
    assert_eq!(count(&e, "executor_changes"), 1);

    let conn = symphony_store::open_reader(&e.db).unwrap();
    let runs: Vec<(String, String, String, Option<String>)> = conn
        .prepare("SELECT provider_id, model_id, status, end_reason FROM agent_runs ORDER BY seq")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        runs,
        vec![
            (
                "claude".into(),
                "claude/fast".into(),
                "FAILED".into(),
                Some("NO_HEARTBEAT".into()),
            ),
            (
                "codex".into(),
                "codex/fast".into(),
                "EXITED".into(),
                Some("COMPLETED".into()),
            ),
        ]
    );

    // Los handoffs registran el traspaso
    assert_eq!(count(&e, "handoffs"), 2);
    assert_eq!(
        one::<i64>(
            &e,
            "SELECT COUNT(*) FROM handoffs WHERE outcome = 'CONTINUED'"
        ),
        1
    );

    // Los mensajes registran los prompts enviados a cada run
    let user_messages: Vec<String> = conn
        .prepare("SELECT content FROM messages WHERE role = 'USER' ORDER BY created_at")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(user_messages.len(), 2);
    let handoff_prompt = &user_messages[1];
    assert!(
        handoff_prompt.contains("Implementar sistema de usuarios con autenticación"),
        "debe contener el objetivo"
    );
    assert!(
        handoff_prompt.contains("implementar UserStore en users.js y tests en users_test.js"),
        "debe contener el qué seguía extraído del último mensaje"
    );
    assert!(
        handoff_prompt.contains("email.js") && handoff_prompt.contains("password.js"),
        "debe reflejar los archivos creados por el agente A"
    );

    // Todos los archivos existen en el worktree final
    assert!(created.worktree.join("email.js").is_file());
    assert!(created.worktree.join("password.js").is_file());
    assert!(created.worktree.join("users.js").is_file());
    assert!(created.worktree.join("users_test.js").is_file());

    // La rama de git se mantuvo consistente
    assert_eq!(
        git(&created.worktree, &["branch", "--show-current"]),
        created.branch
    );

    e.writer.shutdown();
}
