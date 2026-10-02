//! P06.S1: creación de agentes (FLOW §6) — flujo feliz y cada rama de error.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use symphony_adapter_common::{AgentEvent, ProviderAdapter};
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
            symphony_testkit::pinned_bin(
                &PathBuf::from(env!("CARGO_BIN_EXE_symphonyd"))
                    .with_file_name(format!("fake-agent{}", std::env::consts::EXE_SUFFIX)),
            )
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
        chat: false,
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

/// P07.5.S4: crear el chat dos veces devuelve el mismo agente, en `symphony/chat`.
#[tokio::test(flavor = "multi_thread")]
async fn chat_is_created_once_per_project_on_its_own_branch() {
    let e = env(WORK, None).await;
    let mut r = req(&e, "Chat", Execution::DecideLater);
    r.chat = true;
    let first = e.runtime.create_agent(r.clone()).await.unwrap();
    let again = e.runtime.create_agent(r).await.unwrap();
    assert_eq!(again.agent_id, first.agent_id);
    assert_eq!(again.worktree, first.worktree);
    assert_eq!(first.branch, "symphony/chat");
    assert_eq!(first.task_code, "CHAT");
    assert_eq!(count(&e, "agents"), 1);
    assert_eq!(count(&e, "worktrees"), 1);
    assert_eq!(count(&e, "tasks"), 1);
    assert_eq!(
        git(&e.repo, &["rev-parse", "symphony/chat"]),
        git(&e.repo, &["rev-parse", "HEAD"])
    );
    e.writer.shutdown();
}

/// P07.5.S4: el turno del chat termina en `READY` («esperando mensaje»), no en `COMPLETED`.
#[tokio::test(flavor = "multi_thread")]
async fn chat_waits_for_a_message_after_its_turn() {
    let e = env(WORK, None).await;
    let mut r = req(&e, "Hola", Execution::Exact("fake/fast".into()));
    r.chat = true;
    e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "READY");
    assert_eq!(
        one::<String>(&e, "SELECT state_reason FROM agents"),
        "esperando mensaje"
    );
    assert_eq!(one::<String>(&e, "SELECT status FROM tasks"), "READY");
    e.writer.shutdown();
}

/// P07.5.S5: un turno con cambios deja un commit y el worktree limpio; uno sin cambios, ninguno.
#[tokio::test(flavor = "multi_thread")]
async fn chat_commits_a_turn_only_when_it_changed_files() {
    let e = env(WORK, None).await;
    let mut r = req(&e, "Hola", Execution::Exact("fake/fast".into()));
    r.chat = true;
    let created = e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    let subjects = |n: &str| git(&e.repo, &["log", "--format=%s", n]);
    assert_eq!(subjects("symphony/chat"), "chat: turno 1 (fake/fast)\ninit");
    assert_eq!(git(&created.worktree, &["status", "--porcelain"]), "");
    assert_eq!(
        git(&created.worktree, &["show", "HEAD:fix.txt"]),
        "arreglado"
    );

    // El segundo turno reescribe el mismo contenido: no hay cambios, no hay commit.
    e.runtime
        .continue_session(created.agent_id, "otra vez")
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(subjects("symphony/chat"), "chat: turno 1 (fake/fast)\ninit");
    assert_eq!(subjects("main"), "init");
    e.writer.shutdown();
}

/// P07.5.S5: el bus recibe los cambios de estado desde el punto único de escritura.
#[tokio::test(flavor = "multi_thread")]
async fn bus_announces_agent_state_changes() {
    let e = env(WORK, None).await;
    let mut rx = e.bus.subscribe();
    let mut r = req(&e, "Hola", Execution::Exact("fake/fast".into()));
    r.chat = true;
    let created = e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;

    let seen = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let ev = rx.recv().await.unwrap();
            if let AgentEvent::StateChanged { from, to, reason } = &ev.event {
                assert_eq!(ev.agent_id, Some(created.agent_id.to_string()));
                return (from.clone(), to.clone(), reason.clone());
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        seen,
        (
            "RUNNING".to_string(),
            "READY".to_string(),
            Some("esperando mensaje".to_string())
        )
    );
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
        .create_agent(req(
            &e,
            "Con profile",
            Execution::Profile("@inexistente".into()),
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, CreateError::UnknownProfile { profile, .. } if profile == "@inexistente"),
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
        "## Conversación hasta ahora",
        "**Usuario:** Arreglar auth",
        "**Asistente:** Voy por partes.",
    ] {
        assert!(p.contains(want), "falta {want:?} en:\n{p}");
    }
    assert_eq!(h.tokens_sent, symphony_context::handoff::estimate_tokens(p));
    // Nada se recortó: lo que costaría en RAW es lo que se envió.
    assert!(h.tokens_raw_estimate >= h.tokens_sent, "{h:?}");
    e.writer.shutdown();
}

/// P07.5.S3 (criterio de PLAN): con una conversación de 10 turnos, el handoff en modo
/// `raw` lleva todos, en orden, sin omitir nada.
#[tokio::test(flavor = "multi_thread")]
async fn handoff_of_a_ten_turn_conversation_keeps_every_turn_in_raw() {
    let script = "[[step]]\nkind = \"say\"\ntext = \"ok\"\n";
    let e = env(script, None).await;
    let mut r = req(&e, "Charla larga", Execution::Exact("fake/fast".into()));
    r.context_mode = ContextMode::Raw;
    let created = e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    for i in 1..10 {
        e.writer.handle().flush().await.unwrap();
        e.runtime
            .continue_session(created.agent_id, &format!("mensaje {i}"))
            .await
            .unwrap();
        e.runtime.wait_executors().await;
    }
    e.writer.handle().flush().await.unwrap();

    let h = e
        .runtime
        .prepare_handoff(created.agent_id, "cambio")
        .await
        .unwrap();
    let p = &h.prompt;
    assert_eq!(h.mode, ContextMode::Raw);
    assert!(p.contains("**Usuario:** Charla larga"), "{p}");
    let mut at = 0;
    for i in 1..10 {
        let want = format!("**Usuario:** mensaje {i}");
        let found = p[at..]
            .find(&want)
            .unwrap_or_else(|| panic!("falta {want:?} o está fuera de orden en:\n{p}"));
        at += found + want.len();
    }
    assert_eq!(p.matches("**Usuario:**").count(), 10);
    assert_eq!(p.matches("**Asistente:** ok").count(), 10);
    assert!(!p.contains("omitidos"));
    e.writer.shutdown();
}

/// P07.5.S3: el handoff lleva la conversación (usuario y asistente) y no anida el
/// prompt de un handoff anterior, que ya trae esa misma conversación.
#[tokio::test(flavor = "multi_thread")]
async fn handoff_carries_the_conversation_without_nesting_earlier_handoffs() {
    let first = "[[step]]\nkind = \"say\"\ntext = \"primero\"\n[[step]]\nkind = \"hang\"\n";
    let second = "[[step]]\nkind = \"say\"\ntext = \"segundo\"\n";
    let e = env_with(&[("alpha", first), ("beta", second)], None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Arreglar auth",
            Execution::Exact("alpha/fast".into()),
        ))
        .await
        .unwrap();
    // Esperar a que el primer executor haya dicho algo antes de cambiarlo.
    let started = std::time::Instant::now();
    loop {
        e.writer.handle().flush().await.unwrap();
        if count(&e, "messages WHERE role = 'ASSISTANT'") > 0 {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(15), "sin respuesta");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    e.runtime
        .switch(created.agent_id, "beta/smart")
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    let h = e
        .runtime
        .prepare_handoff(created.agent_id, "otro cambio")
        .await
        .unwrap();
    let p = &h.prompt;
    let order: Vec<usize> = [
        "**Usuario:** Arreglar auth",
        "**Asistente:** primero",
        // Sin nada del usuario entre medias, las dos respuestas quedan juntas.
        "

segundo",
    ]
    .iter()
    .map(|want| {
        p.find(want)
            .unwrap_or_else(|| panic!("falta {want:?} en:\n{p}"))
    })
    .collect();
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "fuera de orden:\n{p}"
    );
    assert_eq!(
        p.matches("Retomas una tarea de código").count(),
        1,
        "el prompt del handoff anterior no debe entrar a la conversación:\n{p}"
    );
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

/// P07.5.S6, Journey de chat: empieza en un proveedor, cambia a otro a mitad con un mensaje
/// nuevo y el segundo recibe la conversación sin que el usuario repita nada.
#[tokio::test(flavor = "multi_thread")]
async fn chat_switches_provider_mid_conversation_with_a_new_message() {
    let first = "[[step]]\nkind = \"edit\"\npath = \"alpha.txt\"\ncontent = \"a\\n\"\n";
    let second = "[[step]]\nkind = \"edit\"\npath = \"beta.txt\"\ncontent = \"b\\n\"\n";
    let e = env_with(&[("alpha", first), ("beta", second)], None).await;
    let mut r = req(
        &e,
        "Empieza el arreglo",
        Execution::Exact("alpha/fast".into()),
    );
    r.chat = true;
    let created = e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "READY");

    e.runtime
        .switch_with_message(created.agent_id, "beta/fast", Some("ahora sigue tú"))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "READY");
    assert_eq!(one::<i64>(&e, "SELECT COUNT(*) FROM agents"), 1);
    assert_eq!(
        one::<String>(&e, "SELECT reason FROM executor_changes"),
        "USER_SWITCH"
    );
    assert_eq!(
        git(&e.repo, &["log", "--format=%s", "symphony/chat"]),
        "chat: turno 2 (beta/fast)\nchat: turno 1 (alpha/fast)\ninit"
    );
    // El mensaje nuevo llegó al modelo (va en su prompt) y quedó como `USER` aparte.
    let sent: String = one(
        &e,
        "SELECT content FROM messages WHERE role = 'USER' AND run_id =
            (SELECT id FROM agent_runs WHERE provider_id = 'beta') ORDER BY id LIMIT 1",
    );
    assert!(sent.contains("ahora sigue tú"), "{sent}");
    assert!(sent.contains("Empieza el arreglo"), "{sent}");
    let h = e
        .runtime
        .prepare_handoff(created.agent_id, "otro cambio")
        .await
        .unwrap();
    assert!(
        h.prompt.contains("**Usuario:** Empieza el arreglo"),
        "{}",
        h.prompt
    );
    assert!(
        h.prompt.contains("**Usuario:** ahora sigue tú"),
        "{}",
        h.prompt
    );
    assert_eq!(h.prompt.matches("**Usuario:**").count(), 2, "{}", h.prompt);
    e.writer.shutdown();
}

/// P07.5.S6: política por umbral. Apagada, el chat sigue en su proveedor; encendida y con el
/// último turno por encima del umbral, el siguiente mensaje pasa al otro proveedor.
#[tokio::test(flavor = "multi_thread")]
async fn chat_rotates_provider_only_when_the_token_threshold_is_on_and_reached() {
    let first = "[[step]]\nkind = \"usage\"\ntokens = 5000\n";
    let second = "[[step]]\nkind = \"edit\"\npath = \"beta.txt\"\ncontent = \"b\\n\"\n";
    let e = env_with(&[("alpha", first), ("beta", second)], None).await;
    let mut r = req(&e, "Hola", Execution::Exact("alpha/fast".into()));
    r.chat = true;
    let created = e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let providers = |e: &Env| -> String {
        one(
            e,
            "SELECT GROUP_CONCAT(provider_id, ',') FROM (SELECT provider_id FROM agent_runs ORDER BY seq)",
        )
    };

    // Apagada (por defecto): se reanuda la sesión de alpha.
    e.runtime
        .continue_session(created.agent_id, "sigue")
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(providers(&e), "alpha,alpha");

    // Encendida: el último turno de alpha usó 5000 ≥ 1000, el mensaje va a beta.
    e.runtime.set_chat_switch_tokens(Some(1000));
    e.runtime
        .continue_session(created.agent_id, "y ahora")
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(providers(&e), "alpha,alpha,beta");
    assert_eq!(
        one::<String>(&e, "SELECT reason FROM executor_changes"),
        "FAILOVER"
    );
    assert_eq!(one::<i64>(&e, "SELECT COUNT(*) FROM provider_failures"), 0);
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "READY");

    // Beta no reportó uso: el siguiente mensaje se queda en beta (no rebota).
    e.runtime
        .continue_session(created.agent_id, "otra más")
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(providers(&e), "alpha,alpha,beta,beta");
    e.writer.shutdown();
}

/// P07.5.S6: cuota agotada en el chat → failover con los parsers existentes y, al terminar,
/// el chat sigue abierto (`READY`) y con su turno commiteado.
#[tokio::test(flavor = "multi_thread")]
async fn chat_fails_over_on_quota_and_keeps_waiting_for_messages() {
    let first = "[[step]]\nkind = \"say\"\ntext = \"voy\"\n[[step]]\nkind = \"quota_exhausted\"\nresets_at = 1790300000\n";
    let second = "[[step]]\nkind = \"edit\"\npath = \"beta.txt\"\ncontent = \"b\\n\"\n";
    let e = env_with(&[("alpha", first), ("beta", second)], None).await;
    let mut r = req(
        &e,
        "Arregla el login",
        Execution::Exact("alpha/fast".into()),
    );
    r.chat = true;
    e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "READY");
    assert_eq!(one::<String>(&e, "SELECT status FROM tasks"), "READY");
    assert_eq!(
        one::<String>(&e, "SELECT reason FROM executor_changes"),
        "FAILOVER"
    );
    assert_eq!(one::<i64>(&e, "SELECT COUNT(*) FROM provider_failures"), 1);
    assert_eq!(
        git(&e.repo, &["log", "--format=%s", "-1", "symphony/chat"]),
        "chat: turno 1 (beta/fast)"
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

/// «Abrir en el CLI» (ADR-0005): nunca con el executor vivo ni sin sesión;
/// terminado el turno, la sesión del último run en su worktree.
#[tokio::test(flavor = "multi_thread")]
async fn attach_opens_the_last_cli_session_only_when_idle() {
    let script = r#"
[[step]]
kind = "say"
text = "trabajando"
[[step]]
kind = "sleep"
ms = 1500
"#;
    let e = env(script, None).await;
    let waiting = e
        .runtime
        .create_agent(req(&e, "sin modelo", Execution::DecideLater))
        .await
        .unwrap();
    let err = e.runtime.attach_spec(waiting.agent_id).unwrap_err().0;
    assert!(err.contains("todavía no tiene una sesión"), "{err}");

    let created = e
        .runtime
        .create_agent(req(&e, "con modelo", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    let err = e.runtime.attach_spec(created.agent_id).unwrap_err().0;
    assert!(err.contains("está trabajando"), "{err}");

    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let session: String = one(
        &e,
        &format!(
            "SELECT cli_session_id FROM agent_runs WHERE agent_id = '{}'",
            created.agent_id
        ),
    );
    let (spec, cli) = e.runtime.attach_spec(created.agent_id).unwrap();
    assert_eq!(spec.program, fake_agent());
    assert_eq!(spec.args[0], "attach");
    assert_eq!(spec.args[1].to_string_lossy(), session);
    assert_eq!(spec.cwd.as_deref(), Some(created.worktree.as_path()));
    assert!(!cli.is_empty());

    e.runtime
        .note_attached(created.agent_id, cli)
        .await
        .unwrap();
    e.writer.handle().flush().await.unwrap();
    let note: String = one(&e, "SELECT content FROM messages WHERE role = 'SYSTEM'");
    assert!(note.contains("Sesión abierta en"), "{note}");
    e.writer.shutdown();
}

/// ADR-0009: un CLI que da su id de sesión solo por stderr (Kimi) lo deja guardado y
/// permite retomar la sesión con ese mismo id.
#[tokio::test(flavor = "multi_thread")]
async fn session_id_from_stderr_is_stored_and_resumed() {
    let script = "[[step]]
kind = \"say\"
text = \"listo\"
";
    let e = env_with(&[("fake-stderr", script)], None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "id por stderr",
            Execution::Exact("fake-stderr/fast".into()),
        ))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let agent = created.agent_id;
    let first: String = one(
        &e,
        &format!("SELECT cli_session_id FROM agent_runs WHERE agent_id = '{agent}'"),
    );
    assert!(
        first.starts_with("fake-"),
        "id de stderr sin guardar: {first}"
    );

    let run = e.runtime.continue_session(agent, "sigue").await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let second: String = one(
        &e,
        &format!("SELECT cli_session_id FROM agent_runs WHERE id = '{run}'"),
    );
    assert_eq!(second, first, "el run nuevo debe retomar la misma sesión");
    e.writer.shutdown();
}

/// P07.5.S1: un mensaje después del turno retoma la sesión del CLI (mismo modelo y
/// mismo session id, sin handoff) y reabre al agente `COMPLETED`.
#[tokio::test(flavor = "multi_thread")]
async fn message_after_the_turn_resumes_the_cli_session() {
    let script = "[[step]]\nkind = \"say\"\ntext = \"listo\"\n";
    let e = env(script, None).await;
    let waiting = e
        .runtime
        .create_agent(req(&e, "sin modelo", Execution::DecideLater))
        .await
        .unwrap();
    let err = e
        .runtime
        .continue_session(waiting.agent_id, "hola")
        .await
        .unwrap_err()
        .0;
    assert!(err.contains("todavía no tiene una sesión"), "{err}");

    let created = e
        .runtime
        .create_agent(req(&e, "con modelo", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let agent = created.agent_id;
    let state =
        |e: &Env| one::<String>(e, &format!("SELECT state FROM agents WHERE id = '{agent}'"));
    assert_eq!(state(&e), "COMPLETED");
    let first: String = one(
        &e,
        &format!("SELECT cli_session_id FROM agent_runs WHERE agent_id = '{agent}'"),
    );

    let run = e.runtime.continue_session(agent, "sigue").await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    assert_eq!(
        one::<i64>(
            &e,
            &format!("SELECT COUNT(*) FROM agent_runs WHERE agent_id = '{agent}'")
        ),
        2
    );
    let second: String = one(
        &e,
        &format!("SELECT cli_session_id FROM agent_runs WHERE id = '{run}'"),
    );
    assert_eq!(second, first, "el run nuevo debe continuar la misma sesión");
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT model_id FROM agent_runs WHERE id = '{run}'")
        ),
        "fake/fast"
    );
    assert_eq!(state(&e), "COMPLETED");
    assert_eq!(
        one::<String>(
            &e,
            &format!(
                "SELECT status FROM tasks WHERE id = (SELECT task_id FROM agents WHERE id = '{agent}')"
            )
        ),
        "DONE"
    );
    // Sin handoff ni cambio de executor: la conversación sigue en el CLI.
    assert_eq!(count(&e, "executor_changes"), 0);
    assert_eq!(
        one::<i64>(
            &e,
            &format!("SELECT COUNT(*) FROM handoffs WHERE to_run_id = '{run}'")
        ),
        0
    );
    let said: i64 = one(
        &e,
        &format!(
            "SELECT COUNT(*) FROM messages WHERE agent_id = '{agent}' AND role = 'USER' AND content = 'sigue'"
        ),
    );
    assert_eq!(said, 1);
    e.writer.shutdown();
}

/// Un CLI lento en imprimir su primera línea no es un cuelgue: la inactividad
/// se mide desde el arranque (antes moría como NO_HEARTBEAT en el primer tick).
#[tokio::test(flavor = "multi_thread")]
async fn slow_starting_executor_is_not_taken_for_hung() {
    let script = r#"
startup_delay_ms = 600
[[step]]
kind = "edit"
path = "slow.txt"
content = "ok\n"
[[step]]
kind = "say"
text = "listo"
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
            "arranque lento",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(
        one::<String>(&e, "SELECT end_reason FROM agent_runs"),
        "COMPLETED"
    );
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "COMPLETED");
    assert!(created.worktree.join("slow.txt").is_file());
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

/// Los cinco proveedores de v0.1 + P11, con sus ids reales.
const MATRIX: [&str; 5] = ["anthropic", "openai", "moonshot", "google", "github"];

const MATRIX_A: &str = r#"
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

const MATRIX_B: &str = r#"
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

/// P11.S5: forced kill cruzado (Test D) del proveedor `from` hacia cada uno de los otros cuatro.
/// A trabaja y se cuelga; el watchdog lo mata sin cleanup; B continúa solo con el handoff.
async fn handoff_matrix_from(from: &'static str) {
    let targets: Vec<&'static str> = MATRIX.iter().copied().filter(|p| *p != from).collect();
    let mut providers: Vec<(&'static str, &str)> = vec![(from, MATRIX_A)];
    providers.extend(targets.iter().map(|t| (*t, MATRIX_B)));
    let e = env_with_watchdog(
        &providers,
        None,
        Duration::from_millis(100),
        Duration::from_millis(1500),
    )
    .await;

    for to in targets {
        let pair = format!("{from} -> {to}");
        let title = format!("Sistema de usuarios {from}-{to}");
        let created = e
            .runtime
            .create_agent(req(&e, &title, Execution::Exact(format!("{from}/fast"))))
            .await
            .unwrap();
        e.runtime.wait_executors().await;
        e.writer.handle().flush().await.unwrap();
        let agent = created.agent_id;
        let q = |sql: &str| sql.replace("{agent}", &agent.to_string());

        assert_eq!(
            one::<String>(
                &e,
                &q("SELECT end_reason FROM agent_runs WHERE agent_id = '{agent}' AND seq = 1")
            ),
            "NO_HEARTBEAT",
            "{pair}: el run de origen debe morir por NO_HEARTBEAT"
        );
        assert_eq!(
            one::<String>(&e, &q("SELECT state FROM agents WHERE id = '{agent}'")),
            "FAILED",
            "{pair}"
        );

        e.runtime
            .switch(agent, &format!("{to}/fast"))
            .await
            .unwrap_or_else(|err| panic!("{pair}: el cambio falló: {}", err.0));
        e.runtime.wait_executors().await;
        e.writer.handle().flush().await.unwrap();

        assert_eq!(
            one::<String>(&e, &q("SELECT state FROM agents WHERE id = '{agent}'")),
            "COMPLETED",
            "{pair}: B debe terminar la tarea"
        );
        let conn = symphony_store::open_reader(&e.db).unwrap();
        let runs: Vec<(String, String, Option<String>)> = conn
            .prepare(&q(
                "SELECT provider_id, status, end_reason FROM agent_runs WHERE agent_id = '{agent}' ORDER BY seq",
            ))
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            runs,
            vec![
                (from.into(), "FAILED".into(), Some("NO_HEARTBEAT".into())),
                (to.into(), "EXITED".into(), Some("COMPLETED".into())),
            ],
            "{pair}"
        );
        assert_eq!(
            one::<i64>(
                &e,
                &q(
                    "SELECT COUNT(*) FROM handoffs WHERE outcome = 'CONTINUED' AND agent_id = '{agent}'"
                )
            ),
            1,
            "{pair}: un handoff continuado"
        );

        // Lo que B recibió: solo el handoff, con objetivo, el qué seguía y los archivos de A.
        let prompts: Vec<String> = conn
            .prepare(&q(
                "SELECT content FROM messages WHERE role = 'USER' AND agent_id = '{agent}' ORDER BY created_at",
            ))
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(prompts.len(), 2, "{pair}");
        let handoff = &prompts[1];
        assert!(handoff.contains(&title), "{pair}: falta el objetivo");
        assert!(
            handoff.contains("implementar UserStore en users.js y tests en users_test.js"),
            "{pair}: falta el qué seguía"
        );
        assert!(
            handoff.contains("email.js") && handoff.contains("password.js"),
            "{pair}: faltan los archivos de A"
        );
        for f in ["email.js", "password.js", "users.js", "users_test.js"] {
            assert!(created.worktree.join(f).is_file(), "{pair}: falta {f}");
        }
        assert_eq!(
            git(&created.worktree, &["branch", "--show-current"]),
            created.branch,
            "{pair}"
        );
    }
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn handoff_matrix_from_anthropic() {
    handoff_matrix_from("anthropic").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn handoff_matrix_from_openai() {
    handoff_matrix_from("openai").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn handoff_matrix_from_moonshot() {
    handoff_matrix_from("moonshot").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn handoff_matrix_from_google() {
    handoff_matrix_from("google").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn handoff_matrix_from_github() {
    handoff_matrix_from("github").await;
}

/// P10.S2: la salud del proveedor sigue a lo que pasa en sus runs.
fn health_of(e: &Env, provider: &str) -> (String, String, Option<f64>, Option<i64>, Option<i64>) {
    symphony_store::open_reader(&e.db)
        .unwrap()
        .query_row(
            "SELECT state, quota_certainty, quota_remaining, retry_after_at, reset_at
             FROM provider_health WHERE provider_id = ?1 AND model_id IS NULL",
            [provider],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[tokio::test(flavor = "multi_thread")]
async fn a_429_rate_limits_the_provider_and_never_exhausts_it() {
    // Un 429 y el CLI se cuelga: el watchdog lo mata y no hay éxito que limpie el estado.
    let script =
        "[[step]]\nkind = \"rate_limit\"\nretry_after_ms = 30\n[[step]]\nkind = \"hang\"\n";
    let e = env_with_watchdog(
        &[("alpha", script)],
        None,
        Duration::from_millis(100),
        Duration::from_millis(1500),
    )
    .await;
    e.runtime
        .create_agent(req(&e, "limitado", Execution::Exact("alpha/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    let (state, certainty, remaining, retry_after, reset) = health_of(&e, "alpha");
    assert_eq!(state, "RATE_LIMITED", "un 429 no es «agotado»");
    assert_eq!(
        (certainty.as_str(), remaining, reset),
        ("UNKNOWN", None, None)
    );
    assert!(retry_after.is_some(), "con la espera que pidió el CLI");
    // Queda anotado como fallo temporal, con la cuenta `default`.
    assert_eq!(
        one::<String>(
            &e,
            "SELECT failure_type || '|' || account_id FROM provider_failures"
        ),
        "TEMP_RATE_LIMIT|acct-alpha"
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn exhausted_quota_marks_the_provider_and_the_replacement_stays_healthy() {
    let reset_secs = now_secs() + 3600;
    let first = format!("[[step]]\nkind = \"quota_exhausted\"\nresets_at = {reset_secs}\n");
    let second = "[[step]]\nkind = \"say\"\ntext = \"listo\"\n";
    let e = env_with(&[("alpha", first.as_str()), ("beta", second)], None).await;
    e.runtime
        .create_agent(req(&e, "agotado", Execution::Exact("alpha/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    let (state, _, _, _, reset) = health_of(&e, "alpha");
    assert_eq!(state, "EXHAUSTED");
    assert_eq!(
        reset,
        Some(reset_secs * 1000),
        "la base guarda milisegundos"
    );
    assert_eq!(
        health_of(&e, "beta").0,
        "HEALTHY",
        "el reemplazo terminó bien"
    );
    assert_eq!(
        one::<String>(&e, "SELECT failure_type FROM provider_failures"),
        "DAILY_QUOTA"
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_known_quota_report_below_the_reserve_marks_quota_low_with_a_real_percentage() {
    let script = format!(
        "[[step]]\nkind = \"quota\"\nwindow = \"five_hour\"\nused_fraction = 0.3\nresets_at = {r}\n\
         [[step]]\nkind = \"quota\"\nwindow = \"seven_day\"\nused_fraction = 0.9\nresets_at = {r}\n\
         [[step]]\nkind = \"say\"\ntext = \"listo\"\n",
        r = now_secs() + 7200
    );
    let e = env_with(&[("alpha", script.as_str())], None).await;
    e.runtime
        .create_agent(req(&e, "cuota", Execution::Exact("alpha/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    let (state, certainty, remaining, _, _) = health_of(&e, "alpha");
    // Manda la ventana más apretada (0.9 usado → 0.1 restante ≤ reserva 0.20); un éxito no la borra.
    assert_eq!(state, "QUOTA_LOW");
    assert_eq!(certainty, "KNOWN");
    assert!((remaining.unwrap() - 0.1).abs() < 1e-9, "{remaining:?}");
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn usage_is_reported_when_the_cli_gives_it_and_estimated_when_it_does_not() {
    let reported =
        "[[step]]\nkind = \"usage\"\ntokens = 4321\n[[step]]\nkind = \"say\"\ntext = \"listo\"\n";
    let silent = "[[step]]\nkind = \"say\"\ntext = \"respuesta de unas cuantas palabras\"\n";
    let e = env_with(&[("alpha", reported), ("beta", silent)], None).await;
    for (title, model) in [("con uso", "alpha/fast"), ("sin uso", "beta/fast")] {
        e.runtime
            .create_agent(req(&e, title, Execution::Exact(model.into())))
            .await
            .unwrap();
        e.runtime.wait_executors().await;
    }
    e.writer.handle().flush().await.unwrap();

    let usage = |provider: &str| -> (String, i64) {
        symphony_store::open_reader(&e.db)
            .unwrap()
            .query_row(
                "SELECT source, COALESCE(tokens_in, 0) FROM usage_records WHERE provider_id = ?1",
                [provider],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    };
    assert_eq!(usage("alpha"), ("REPORTED".into(), 4321));
    let (source, tokens_in) = usage("beta");
    assert_eq!(source, "ESTIMATED");
    assert!(tokens_in > 0, "se estima a partir del prompt enviado");
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_token_budget_in_the_config_gives_an_estimated_quota_never_a_percentage() {
    let heavy =
        "[[step]]\nkind = \"usage\"\ntokens = 900\n[[step]]\nkind = \"say\"\ntext = \"listo\"\n";
    let e = env_with(&[("alpha", heavy), ("beta", heavy)], None).await;
    // Solo alpha tiene presupuesto: 1000 tokens por ventana de una hora, reserva 0.20.
    let mut cfg = symphony_daemon::health::HealthConfig::default();
    cfg.budgets.insert("alpha".into(), (1, 1000));
    e.bus.set_health_config(cfg);
    for (title, model) in [
        ("con presupuesto", "alpha/fast"),
        ("sin presupuesto", "beta/fast"),
    ] {
        e.runtime
            .create_agent(req(&e, title, Execution::Exact(model.into())))
            .await
            .unwrap();
        e.runtime.wait_executors().await;
    }
    e.writer.handle().flush().await.unwrap();

    let (state, certainty, remaining, _, _) = health_of(&e, "alpha");
    assert_eq!(certainty, "ESTIMATED");
    assert_eq!(
        remaining, None,
        "una estimación nunca se muestra como porcentaje"
    );
    assert_eq!(state, "QUOTA_LOW", "900 de 1000 con reserva 0.20");
    let (state, certainty, remaining, _, _) = health_of(&e, "beta");
    assert_eq!(
        (state.as_str(), certainty.as_str(), remaining),
        ("HEALTHY", "UNKNOWN", None)
    );
    e.writer.shutdown();
}

/// P10.S5: routing por profiles y failover con el router.
async fn set_health(
    e: &Env,
    provider: &str,
    state: &str,
    certainty: &str,
    remaining: Option<f64>,
    reset_in_ms: Option<i64>,
) {
    let (provider, state, certainty) = (
        provider.to_string(),
        state.to_string(),
        certainty.to_string(),
    );
    e.writer
        .handle()
        .write(Box::new(move |t| {
            let now = now_secs() * 1000;
            let h = symphony_core::Health {
                state: state.parse().unwrap(),
                certainty: certainty.parse().unwrap(),
                remaining,
                retry_after_at: None,
                reset_at: reset_in_ms.map(|ms| now + ms),
                evidence: Some("test".into()),
                updated_at: now,
            };
            symphony_store::health::put_health(t, &provider, None, &h)?;
            Ok(())
        }))
        .await
        .unwrap();
}

const SAY: &str = "[[step]]\nkind = \"say\"\ntext = \"listo\"\n";
const QUOTA: &str = "[[step]]\nkind = \"quota_exhausted\"\n";

fn rejections(e: &Env, agent: &str) -> Vec<(String, Option<String>)> {
    let conn = symphony_store::open_reader(&e.db).unwrap();
    conn.prepare(
        "SELECT c.model_id, c.reject_reason FROM routing_candidates c
         JOIN routing_decisions d ON d.id = c.decision_id
         WHERE d.agent_id = ?1 AND d.id = (SELECT id FROM routing_decisions WHERE agent_id = ?1 ORDER BY decided_at DESC, id DESC LIMIT 1)
         ORDER BY c.model_id",
    )
    .unwrap()
    .query_map([agent], |r| Ok((r.get(0)?, r.get(1)?)))
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_spawn_skips_the_exhausted_provider_and_records_the_decision() {
    let e = env_with(&[("alpha", SAY), ("beta", SAY)], None).await;
    set_health(&e, "alpha", "EXHAUSTED", "UNKNOWN", None, Some(3_600_000)).await;
    let created = e
        .runtime
        .create_agent(req(&e, "con profile", Execution::Profile("@code".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let agent = created.agent_id.to_string();

    // El modelo vive en el run; el agente solo recuerda el profile (AGENT ≠ MODEL).
    assert_eq!(
        one::<String>(
            &e,
            &format!(
                "SELECT execution_mode || '|' || requested_profile_id || '|' || COALESCE(requested_model_id, 'null') FROM agents WHERE id = '{agent}'"
            )
        ),
        "PROFILE|@code|null"
    );
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT provider_id FROM agent_runs WHERE agent_id = '{agent}'")
        ),
        "beta"
    );
    // La decisión queda guardada, atada al run, con el motivo de cada descarte.
    assert_eq!(
        one::<String>(
            &e,
            &format!(
                "SELECT d.trigger || '|' || d.profile_id FROM routing_decisions d JOIN agent_runs r ON r.routing_decision_id = d.id WHERE d.agent_id = '{agent}'"
            )
        ),
        "SPAWN|@code"
    );
    let rej = rejections(&e, &agent);
    assert!(
        rej.iter()
            .filter(|(m, _)| m.starts_with("alpha/"))
            .all(|(_, r)| r.as_deref() == Some("EXHAUSTED")),
        "{rej:?}"
    );
    assert!(
        rej.iter()
            .filter(|(m, _)| m.starts_with("beta/"))
            .any(|(_, r)| r.is_none()),
        "{rej:?}"
    );
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT account_id FROM agent_runs WHERE agent_id = '{agent}'")
        ),
        "acct-beta"
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_with_no_eligible_model_explains_why_and_keeps_the_task() {
    let e = env_with(&[("alpha", SAY)], None).await;
    set_health(&e, "alpha", "AUTH_ERROR", "UNKNOWN", None, None).await;
    let err = e
        .runtime
        .create_agent(req(&e, "sin nadie", Execution::Profile("@fast".into())))
        .await
        .unwrap_err();
    match err {
        CreateError::NoEligibleModel {
            profile,
            explanation,
            task,
        } => {
            assert_eq!(profile, "@fast");
            assert!(
                explanation.contains("descartado: sesión inválida o vencida"),
                "{explanation}"
            );
            assert!(explanation.contains("Ningún modelo es elegible ahora."));
            assert_eq!(task, "sin nadie", "la tarea no se pierde");
        }
        other => panic!("{other:?}"),
    }
    assert_nothing_created(&e);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_exact_model_is_obeyed_even_with_low_quota_and_the_choice_is_recorded() {
    let e = env_with(&[("alpha", SAY), ("beta", SAY)], None).await;
    // Cuota informada dentro de la reserva: un profile automático no la gastaría; el usuario sí.
    set_health(&e, "alpha", "QUOTA_LOW", "KNOWN", Some(0.1), None).await;
    let created = e
        .runtime
        .create_agent(req(&e, "exacto", Execution::Exact("alpha/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let agent = created.agent_id.to_string();
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT provider_id FROM agent_runs WHERE agent_id = '{agent}'")
        ),
        "alpha"
    );
    let explanation: String = one(
        &e,
        &format!("SELECT explanation FROM routing_decisions WHERE agent_id = '{agent}'"),
    );
    assert!(
        explanation.starts_with("Modelo exacto elegido por el usuario: alpha/fast"),
        "{explanation}"
    );
    assert_eq!(
        one::<Option<String>>(
            &e,
            &format!("SELECT profile_id FROM routing_decisions WHERE agent_id = '{agent}'")
        ),
        None
    );
    e.writer.shutdown();
}

async fn failover_from(
    first: &'static str,
    policy: symphony_core::FailoverPolicy,
    others: &[(&'static str, &'static str)],
) -> (Env, String) {
    let mut providers = vec![(first, QUOTA)];
    providers.extend_from_slice(others);
    let e = env_with(&providers, None).await;
    let mut r = req(&e, "failover", Execution::Exact(format!("{first}/fast")));
    r.failover = policy;
    let created = e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    (e, created.agent_id.to_string())
}

#[tokio::test(flavor = "multi_thread")]
async fn failover_any_goes_to_the_best_other_provider_and_records_why() {
    let (e, agent) = failover_from(
        "alpha",
        symphony_core::FailoverPolicy::Any,
        &[("beta", SAY), ("gamma", SAY)],
    )
    .await;
    let providers: String = one(
        &e,
        &format!(
            "SELECT group_concat(provider_id, '>') FROM (SELECT provider_id FROM agent_runs WHERE agent_id = '{agent}' ORDER BY seq)"
        ),
    );
    assert_eq!(
        providers, "alpha>beta",
        "gana el primero por puntaje y, a igualdad, por id"
    );
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT state FROM agents WHERE id = '{agent}'")
        ),
        "COMPLETED"
    );
    // La decisión de failover queda atada al run nuevo, con todos los modelos evaluados.
    assert_eq!(
        one::<String>(
            &e,
            &format!(
                "SELECT d.trigger FROM routing_decisions d JOIN agent_runs r ON r.routing_decision_id = d.id WHERE r.agent_id = '{agent}' AND r.seq = 2"
            )
        ),
        "FAILOVER"
    );
    let rej = rejections(&e, &agent);
    // El modelo que se agotó queda descartado como agotado; el proveedor agotado, por salud.
    assert!(
        rej.iter()
            .any(|(m, r)| m == "alpha/fast" && r.as_deref() == Some("EXHAUSTED")),
        "{rej:?}"
    );
    assert!(
        rej.iter()
            .any(|(m, r)| m.starts_with("gamma/") && r.is_none()),
        "gamma sigue elegible: {rej:?}"
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn failover_none_leaves_the_agent_waiting_and_asks_the_user() {
    let (e, agent) = failover_from(
        "alpha",
        symphony_core::FailoverPolicy::None,
        &[("beta", SAY)],
    )
    .await;
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT state FROM agents WHERE id = '{agent}'")
        ),
        "WAITING_PROVIDER"
    );
    assert!(
        one::<String>(
            &e,
            &format!("SELECT state_reason FROM agents WHERE id = '{agent}'")
        )
        .contains("el failover está desactivado")
    );
    assert_eq!(
        one::<i64>(
            &e,
            &format!("SELECT COUNT(*) FROM agent_runs WHERE agent_id = '{agent}'")
        ),
        1
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn failover_same_provider_does_not_leave_a_provider_that_is_exhausted() {
    // Quedarse en el proveedor no sirve si lo que se agotó fue su cuota: espera al usuario.
    let (e, agent) = failover_from(
        "alpha",
        symphony_core::FailoverPolicy::SameProvider,
        &[("beta", SAY)],
    )
    .await;
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT state FROM agents WHERE id = '{agent}'")
        ),
        "WAITING_PROVIDER"
    );
    assert_eq!(
        one::<i64>(
            &e,
            &format!("SELECT COUNT(*) FROM agent_runs WHERE agent_id = '{agent}'")
        ),
        1
    );
    // Aun así queda la decisión: todo lo de otros proveedores quedó fuera por la política.
    assert_eq!(
        one::<String>(
            &e,
            &format!(
                "SELECT d.trigger || '|' || COALESCE(d.selected_model_id, 'ninguno') FROM routing_decisions d WHERE d.agent_id = '{agent}' AND d.trigger = 'FAILOVER'"
            )
        ),
        "FAILOVER|ninguno"
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_automatic_failover_never_spends_the_reserve_of_another_provider() {
    let e = env_with(&[("alpha", QUOTA), ("beta", SAY), ("gamma", SAY)], None).await;
    // beta (que ganaría por id) tiene la cuota informada dentro de la reserva: se salta.
    set_health(&e, "beta", "QUOTA_LOW", "KNOWN", Some(0.1), None).await;
    let created = e
        .runtime
        .create_agent(req(&e, "reserva", Execution::Exact("alpha/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let agent = created.agent_id.to_string();
    let providers: String = one(
        &e,
        &format!(
            "SELECT group_concat(provider_id, '>') FROM (SELECT provider_id FROM agent_runs WHERE agent_id = '{agent}' ORDER BY seq)"
        ),
    );
    assert_eq!(providers, "alpha>gamma", "se salta la reserva de beta");
    let rej = rejections(&e, &agent);
    assert!(
        rej.iter()
            .any(|(m, r)| m.starts_with("beta/") && r.as_deref() == Some("RESERVE")),
        "{rej:?}"
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_manual_switch_is_recorded_as_an_exact_choice() {
    // alpha sigue trabajando cuando el usuario cambia de modelo.
    let slow = "[[step]]\nkind = \"say\"\ntext = \"voy\"\n[[step]]\nkind = \"sleep\"\nms = 30000\n";
    let e = env_with(&[("alpha", slow), ("beta", SAY)], None).await;
    let created = e
        .runtime
        .create_agent(req(&e, "cambio", Execution::Exact("alpha/fast".into())))
        .await
        .unwrap();
    e.runtime
        .switch(created.agent_id, "beta/smart")
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let agent = created.agent_id.to_string();
    assert_eq!(
        one::<String>(
            &e,
            &format!(
                "SELECT trigger || '|' || selected_model_id FROM routing_decisions WHERE agent_id = '{agent}' AND trigger = 'SWITCH'"
            )
        ),
        "SWITCH|beta/smart"
    );
    let explain = symphony_daemon::routing::explain(
        &symphony_store::open_reader(&e.db).unwrap(),
        created.agent_id,
        5,
    )
    .unwrap();
    let decisions = explain.as_array().unwrap();
    assert_eq!(decisions.len(), 2, "el spawn y el cambio");
    assert_eq!(decisions[0]["trigger"], "SWITCH");
    assert!(decisions[0]["candidates"].as_array().unwrap().len() >= 4);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_by_profile_lets_the_router_choose_among_the_usable_models() {
    let slow = "[[step]]\nkind = \"say\"\ntext = \"voy\"\n[[step]]\nkind = \"sleep\"\nms = 30000\n";
    let e = env_with(&[("alpha", slow), ("beta", SAY), ("gamma", SAY)], None).await;
    set_health(&e, "beta", "EXHAUSTED", "UNKNOWN", None, Some(3_600_000)).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "por profile",
            Execution::Exact("alpha/fast".into()),
        ))
        .await
        .unwrap();
    let (_, model) = e
        .runtime
        .switch_to_profile(created.agent_id, "@fast", None)
        .await
        .unwrap();
    assert!(model.starts_with("gamma/"), "beta está agotado: {model}");
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let agent = created.agent_id.to_string();
    assert_eq!(
        one::<String>(
            &e,
            &format!(
                "SELECT trigger || '|' || profile_id FROM routing_decisions WHERE agent_id = '{agent}' AND trigger = 'SWITCH'"
            )
        ),
        "SWITCH|@fast"
    );
    // Un profile que no existe se rechaza sin tocar al agente.
    let err = e
        .runtime
        .switch_to_profile(created.agent_id, "@nada", None)
        .await
        .unwrap_err();
    assert!(err.0.contains("no existe"), "{}", err.0);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn long_messages_and_diffs_become_searchable_context_objects() {
    // Un mensaje de más de 4 KB va al object store; un archivo nuevo deja un diff en el checkpoint.
    let long = format!(
        "{} palabraunicadelmensaje {}",
        "relleno ".repeat(700),
        "fin ".repeat(100)
    );
    let script = format!(
        "[[step]]\nkind = \"edit\"\npath = \"README.md\"\ncontent = \"identificadorraro actualizado\"\n[[step]]\nkind = \"say\"\ntext = \"{long}\"\n"
    );
    let e = env(&script, None).await;
    e.runtime
        .create_agent(req(&e, "indexar", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let project: String = one(&e, "SELECT id FROM projects");

    let conn = symphony_store::open_reader(&e.db).unwrap();
    let find = |word: &str| {
        let q = symphony_context::chunk::fts_query(word).unwrap();
        symphony_store::context::search(&conn, &project, &q, None, 10).unwrap()
    };
    let dbg: (i64, i64, String, i64) = (
        one(&e, "SELECT COUNT(*) FROM context_objects"),
        one(&e, "SELECT COUNT(*) FROM context_chunks"),
        one(&e, "SELECT state || COALESCE(state_reason, '') FROM agents"),
        one(&e, "SELECT MAX(LENGTH(content)) FROM messages"),
    );
    let msg = find("palabraunicadelmensaje");
    assert_eq!(msg.len(), 1, "{msg:?} objetos/chunks={dbg:?}");
    assert!(msg[0].uri.starts_with("ctx://message/"), "{}", msg[0].uri);
    // El original completo sigue recuperable por su dirección.
    let obj = symphony_store::context::object_by_uri(&conn, &msg[0].uri)
        .unwrap()
        .unwrap();
    let bytes = symphony_object_store::ObjectStore::new(e.home.join("objects"))
        .get(&obj.blob_hash)
        .unwrap();
    assert!(
        String::from_utf8(bytes)
            .unwrap()
            .contains("palabraunicadelmensaje")
    );
    let diff = find("identificadorraro");
    assert!(
        !diff.is_empty() && diff[0].uri.starts_with("ctx://diff/"),
        "{diff:?}"
    );
    e.writer.shutdown();
}

/// Un CLI que falla (login, cuota) antes de leer su prompt deja un `EPIPE` al escribirlo: no es un
/// fallo de arranque; su salida y su código de salida mandan y el failover sigue su curso.
#[tokio::test(flavor = "multi_thread")]
async fn cli_that_quits_before_reading_its_prompt_still_fails_over() {
    let first = "[[step]]\nkind = \"quota_exhausted\"\n";
    let second = "[[step]]\nkind = \"edit\"\npath = \"b.txt\"\ncontent = \"b\n\"\n";
    let e = env_with(&[("alpha", first), ("beta", second)], None).await;
    let mut r = req(
        &e,
        "tarea con prompt enorme",
        Execution::Exact("alpha/fast".into()),
    );
    // Mayor que el búfer de la tubería: la escritura sigue en curso cuando alpha ya salió.
    r.description = Some("x".repeat(1_000_000));
    e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    assert_eq!(
        one::<String>(&e, "SELECT end_reason FROM agent_runs WHERE seq = 1"),
        "QUOTA_EXHAUSTED"
    );
    assert_eq!(count(&e, "provider_failures"), 1);
    assert_eq!(count(&e, "agent_runs"), 2);
    e.writer.shutdown();
}
