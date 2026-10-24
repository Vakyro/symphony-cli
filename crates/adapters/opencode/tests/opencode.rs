//! ADR-0012: adapter de OpenCode contra fixtures reales (OpenCode 1.18.34, modelos gratuitos de
//! su proveedor propio) y la suite de contrato.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::{Value, json};
use symphony_adapter_common::contract::{self, Fixtures};
use symphony_adapter_common::{
    AgentEvent, HookCommand, ProviderAdapter, ResumeRequest, SpawnRequest, ToolKind,
};
use symphony_adapter_opencode::{
    OpenCodeAdapter, canonical_id, fixtures_dir, mcp_config, parse_models, run_config,
};
use symphony_core::FailureType;

fn adapter() -> OpenCodeAdapter {
    OpenCodeAdapter {
        binary: Some(std::env::current_exe().unwrap()),
        models_cache: Some(std::env::temp_dir().join("symphony-test-opencode-models-unused.txt")),
    }
}

fn lines(file: &str) -> Vec<String> {
    std::fs::read_to_string(fixtures_dir().join(file))
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}

fn request(model: &str) -> SpawnRequest {
    SpawnRequest {
        worktree: std::env::temp_dir(),
        model: model.into(),
        prompt: "hola".into(),
        session_id: Some("00000000-0000-4000-8000-000000000000".into()),
        hook: None,
        env: vec![("SYMPHONY_AGENT_ID".into(), "a1".into())],
    }
}

fn args(spec: &symphony_adapter_common::ProcessSpec) -> Vec<String> {
    spec.args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

fn names(events: &[AgentEvent]) -> Vec<&'static str> {
    events.iter().map(AgentEvent::type_name).collect()
}

fn fixtures() -> Fixtures {
    let mut stream = Vec::new();
    // Un turno sin herramientas: sesión, respuesta, uso y fin de turno.
    let want = [
        vec!["AgentStarted"],
        vec!["AssistantText"],
        vec!["TurnUsage", "TurnFinished"],
    ];
    for (l, w) in lines("stream.jsonl").into_iter().zip(want) {
        stream.push((l, w));
    }
    // write + bash en un paso, y la respuesta en otro.
    let want = [
        vec!["AgentStarted"],
        vec!["ToolRequested", "FileModified", "ToolFinished"],
        vec!["CommandRequested", "CommandFinished"],
        vec!["TurnUsage"],
        vec!["AgentStarted"],
        vec!["AssistantText"],
        vec!["TurnUsage", "TurnFinished"],
    ];
    for (l, w) in lines("tools.jsonl").into_iter().zip(want) {
        stream.push((l, w));
    }
    // glob, read, edit, bash que sale con 3, y un read que falla.
    let want = [
        vec!["ToolRequested", "ToolFinished"],
        vec!["ToolRequested", "ToolFinished"],
        vec!["ToolRequested", "FileModified", "ToolFinished"],
        vec!["CommandRequested", "CommandFinished"],
        vec!["ToolRequested", "ToolFinished"],
    ];
    for (l, w) in lines("tools-error.jsonl").into_iter().zip(want) {
        stream.push((l, w));
    }
    stream.push((lines("error-modelo.jsonl").remove(0), vec!["ProviderError"]));
    Fixtures {
        stream,
        hooks: vec![],
        errors: vec![
            // Sintéticos: no se pudo provocar cuota ni auth con los modelos gratuitos.
            (
                "ProviderModelNotFoundError: model not found".into(),
                FailureType::ModelUnavailable,
            ),
            (
                "APIError: rate limit exceeded statusCode=429".into(),
                FailureType::TempRateLimit,
            ),
            (
                "APIError: Insufficient balance, add credits".into(),
                FailureType::DailyQuota,
            ),
            (
                "ProviderAuthError: 401 Unauthorized (Authorization: Bearer sk-AAAAAAAAAAAAAAAAAAAAAAAA)"
                    .into(),
                FailureType::Auth,
            ),
        ],
    }
}

#[test]
fn passes_the_contract_suite_with_real_fixtures() {
    let f = fixtures();
    assert!(
        f.stream.len() >= 15,
        "fixtures incompletos: {}",
        f.stream.len()
    );
    let failures = contract::check(&adapter(), &f);
    assert!(
        failures.is_empty(),
        "fallas del contrato:\n{}",
        failures.join("\n")
    );
}

#[test]
fn the_session_id_comes_from_the_first_event_and_the_turn_ends_on_stop() {
    let a = adapter();
    let first = lines("stream.jsonl").remove(0);
    assert!(matches!(
        &a.parse_stream_line(&first)[..],
        [AgentEvent::SessionStarted { cli_session_id: Some(id), .. }] if id.starts_with("ses_")
    ));
    let finish = lines("stream.jsonl").remove(2);
    let ev = a.parse_stream_line(&finish);
    assert!(matches!(
        ev[0],
        AgentEvent::TurnUsage {
            context_tokens: 28_409
        }
    ));
    assert!(matches!(ev[1], AgentEvent::TurnFinished { .. }));
    // Un paso que sigue con herramientas no cierra el turno.
    let mid = lines("tools.jsonl").remove(3);
    assert_eq!(names(&a.parse_stream_line(&mid)), ["TurnUsage"]);
}

#[test]
fn tool_events_carry_command_path_and_the_real_exit_code() {
    let a = adapter();
    let ok = lines("tools.jsonl");
    let write = a.parse_stream_line(&ok[1]);
    assert!(matches!(
        &write[1],
        AgentEvent::FileModified { path: Some(p), .. } if p.ends_with("hello.txt")
    ));
    let bash = a.parse_stream_line(&ok[2]);
    assert!(matches!(
        &bash[0],
        AgentEvent::ToolRequested { command: Some(c), kind: ToolKind::Command, .. } if c == "echo done"
    ));
    assert!(matches!(
        bash[1],
        AgentEvent::ToolFinished {
            ok: true,
            exit_code: Some(0),
            ..
        }
    ));

    let bad = lines("tools-error.jsonl");
    // `bash` termina como `completed` aunque el comando falle: manda el código de salida.
    assert!(matches!(
        a.parse_stream_line(&bad[3])[1],
        AgentEvent::ToolFinished {
            ok: false,
            exit_code: Some(3),
            ..
        }
    ));
    // Una herramienta en `error`.
    assert!(matches!(
        a.parse_stream_line(&bad[4])[1],
        AgentEvent::ToolFinished { ok: false, .. }
    ));
    // Y no hay `FileModified` de una edición que falló.
    let failed_edit = json!({
        "type": "tool_use", "sessionID": "ses_x",
        "part": { "type": "tool", "tool": "write", "callID": "c1",
                  "state": { "status": "error", "input": { "filePath": "a.txt" }, "error": "denied" } }
    });
    assert_eq!(
        names(&a.parse_stream_line(&failed_edit.to_string())),
        ["ToolRequested", "ToolFinished"]
    );
}

#[test]
fn error_events_are_never_lost() {
    let a = adapter();
    let ev = a.parse_stream_line(&lines("error-modelo.jsonl").remove(0));
    assert!(matches!(
        &ev[..],
        [AgentEvent::ProviderError(e)] if e.failure_type == FailureType::ProviderError
            && e.raw_code.as_deref() == Some("UnknownError")
    ));
    // Un error reconocible se clasifica y puede pedir failover.
    let quota = json!({
        "type": "error", "sessionID": "ses_x",
        "error": { "name": "APIError", "data": { "message": "You exceeded your usage limit", "statusCode": 429 } }
    });
    let ev = a.parse_stream_line(&quota.to_string());
    assert!(matches!(
        &ev[..],
        [AgentEvent::ProviderError(e)] if e.failure_type == FailureType::DailyQuota && !e.transient
    ));
}

#[test]
fn an_upstream_429_is_a_transient_rate_limit_not_a_quota() {
    let ev = adapter().parse_stream_line(&lines("error-upstream.jsonl").remove(0));
    assert!(matches!(
        &ev[..],
        [AgentEvent::ProviderError(e)] if e.failure_type == FailureType::TempRateLimit && e.transient
    ));
}

#[test]
fn spawn_and_resume_use_run_json_and_the_session_flag() {
    let a = adapter();
    let spawn = args(&a.spawn_spec(&request("opencode/big-pickle")).unwrap());
    assert_eq!(
        spawn,
        ["run", "--format", "json", "-m", "opencode/big-pickle"],
        "el id de sesión lo asigna OpenCode: no se pasa"
    );
    let resume = args(
        &a.resume_spec(&ResumeRequest {
            spawn: request("opencode/big-pickle"),
            cli_session_id: "ses_abc".into(),
        })
        .unwrap(),
    );
    assert_eq!(resume[resume.len() - 2..], ["-s", "ses_abc"]);
    assert!(a.close_stdin_after_prompt());
    assert!(a.encode_user_message("x").is_none());
}

#[test]
fn the_prompt_names_the_worktree_so_the_agent_stays_isolated() {
    let a = adapter();
    let wt = std::env::temp_dir().join("agent-001");
    let framed = a.frame_prompt("Arregla el login", &wt);
    assert!(framed.contains(&wt.display().to_string()), "{framed}");
    assert!(framed.contains("SOLO dentro"), "{framed}");
    assert!(framed.ends_with("Arregla el login"), "{framed}");
    // El resto de los adapters no cambian el prompt.
    let kimi = symphony_adapter_kimi::KimiAdapter::default();
    assert_eq!(kimi.frame_prompt("x", &wt), "x");
}

#[test]
fn default_model_does_not_pass_dash_m() {
    let spec = a_spec("default");
    assert!(!args(&spec).contains(&"-m".to_string()));
    let attach = adapter()
        .attach_spec("ses_abc", "default", &std::env::temp_dir())
        .unwrap();
    assert_eq!(args(&attach), ["-s", "ses_abc"]);
    let attach = adapter()
        .attach_spec("ses_abc", "opencode/big-pickle", &std::env::temp_dir())
        .unwrap();
    assert_eq!(
        args(&attach),
        ["-s", "ses_abc", "-m", "opencode/big-pickle"]
    );
}

fn a_spec(model: &str) -> symphony_adapter_common::ProcessSpec {
    adapter().spawn_spec(&request(model)).unwrap()
}

#[test]
fn canonical_ids_never_carry_two_slashes() {
    assert_eq!(canonical_id("opencode/big-pickle"), "opencode/big-pickle");
    assert_eq!(
        canonical_id("github-copilot/gpt-5"),
        "opencode/github-copilot:gpt-5"
    );
    assert_eq!(canonical_id("solo"), "opencode/solo");
    for id in ["opencode/big-pickle", "anthropic/claude-sonnet-4"] {
        assert_eq!(canonical_id(id).matches('/').count(), 1, "{id}");
    }
}

#[test]
fn models_output_is_parsed_and_the_list_always_has_the_default() {
    let text = "opencode/big-pickle\nopencode/ling-3.1-flash-free\n\n  anthropic/claude-sonnet-4  \nruido sin barra\nopencode/big-pickle\nrara ruta/ con espacio\n";
    assert_eq!(
        parse_models(text),
        [
            "opencode/big-pickle",
            "opencode/ling-3.1-flash-free",
            "anthropic/claude-sonnet-4"
        ]
    );
    // Sin ejecutable ni lista guardada no se rompe: queda el modelo configurado.
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache/opencode-models.txt");
    let missing = OpenCodeAdapter {
        binary: Some("no-existe-opencode".into()),
        models_cache: Some(cache.clone()),
    };
    let models = missing.list_models();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "opencode/default");
    assert_eq!(models[0].cli_model_id, "default");

    // Con una lista guardada vigente se usa sin lanzar nada (el arranque del daemon no espera).
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(&cache, text).unwrap();
    let models = missing.list_models();
    let ids: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "opencode/default",
            "opencode/big-pickle",
            "opencode/ling-3.1-flash-free",
            "opencode/anthropic:claude-sonnet-4"
        ]
    );
    assert_eq!(models[3].cli_model_id, "anthropic/claude-sonnet-4");
}

#[test]
fn the_version_comes_from_the_npm_package_without_running_the_cli() {
    use symphony_adapter_opencode::version_from_package;
    let d = tempfile::tempdir().unwrap();
    let pkg = d.path().join("node_modules/opencode-ai");
    std::fs::create_dir_all(pkg.join("bin")).unwrap();
    let exe = pkg.join("bin/opencode.exe");
    std::fs::write(&exe, b"x").unwrap();
    assert_eq!(version_from_package(&exe), None);
    std::fs::write(
        pkg.join("package.json"),
        r#"{"name":"opencode-ai","version":"1.18.34"}"#,
    )
    .unwrap();
    assert_eq!(version_from_package(&exe).as_deref(), Some("1.18.34"));
    // El `package.json` de otro paquete no cuenta.
    std::fs::write(
        pkg.join("package.json"),
        r#"{"name":"otro","version":"9.9.9"}"#,
    )
    .unwrap();
    assert_eq!(version_from_package(&exe), None);
}

#[test]
fn mcp_servers_travel_in_the_config_env_per_invocation() {
    let mut req = request("opencode/big-pickle");
    req.hook = Some(HookCommand {
        program: "/usr/bin/symphony".into(),
        args: vec!["hook".into(), "emit".into()],
    });
    req.env.push((
        "SYMPHONY_MCP_EXTRA".into(),
        json!([
            { "name": "fs", "transport": "stdio", "command": "npx", "args": ["-y", "server-fs"], "env": { "RAIZ": "." } },
            { "name": "remoto", "transport": "http", "url": "https://example.test/mcp" }
        ])
        .to_string(),
    ));
    let cfg = mcp_config(&req).unwrap();
    let mcp = &cfg["mcp"];
    assert_eq!(
        mcp["symphony-context"]["command"],
        json!(["/usr/bin/symphony", "mcp", "serve"])
    );
    assert_eq!(mcp["symphony-context"]["type"], "local");
    assert_eq!(
        mcp["symphony-context"]["environment"]["SYMPHONY_AGENT_ID"],
        "a1"
    );
    assert_eq!(mcp["fs"]["command"], json!(["npx", "-y", "server-fs"]));
    assert_eq!(mcp["fs"]["environment"]["RAIZ"], ".");
    assert_eq!(mcp["remoto"]["type"], "remote");
    assert_eq!(mcp["remoto"]["url"], "https://example.test/mcp");

    let spec = adapter().spawn_spec(&req).unwrap();
    let cfg = run_config(&req);
    let env = spec
        .env
        .iter()
        .find(|(k, _)| k == "OPENCODE_CONFIG_CONTENT")
        .map(|(_, v)| v.to_string_lossy().into_owned())
        .expect("OPENCODE_CONFIG_CONTENT");
    let parsed: Value = serde_json::from_str(&env).unwrap();
    assert_eq!(parsed, cfg);

    assert_eq!(parsed["mcp"], mcp_config(&req).unwrap()["mcp"]);

    // Sin hook ni servidores del usuario solo viaja el permiso del worktree, sin `mcp`.
    let plain = run_config(&request("default"));
    assert!(plain.get("mcp").is_none());
    assert!(plain["permission"]["external_directory"].is_object());
}

#[test]
fn only_the_worktree_subtree_is_allowed_as_an_external_directory() {
    let mut req = request("default");
    req.worktree = r"C:\Users\Leo\.symphony\worktrees\p1\agent-001".into();
    let cfg = run_config(&req);
    let ext = cfg["permission"]["external_directory"].as_object().unwrap();
    assert_eq!(ext.len(), 2);
    assert_eq!(
        ext["C:/Users/Leo/.symphony/worktrees/p1/agent-001/**"],
        "allow"
    );
    assert_eq!(
        ext[r"C:\Users\Leo\.symphony\worktrees\p1\agent-001\**"],
        "allow"
    );
    // Ningún comodín que abra más que el worktree.
    assert!(ext.keys().all(|k| k.contains("agent-001")));
}

#[test]
fn a_finished_background_query_is_promoted_and_a_broken_one_is_dropped() {
    let d = tempfile::tempdir().unwrap();
    let cache = d.path().join("opencode-models.txt");
    let tmp = cache.with_extension("tmp");
    let a = OpenCodeAdapter {
        binary: Some("no-existe-opencode".into()),
        models_cache: Some(cache.clone()),
    };
    let age = |p: &std::path::Path, secs: u64| {
        let f = std::fs::OpenOptions::new().write(true).open(p).unwrap();
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs))
            .unwrap();
    };
    // Una consulta reciente todavía puede estar escribiendo: no se toca.
    std::fs::write(&tmp, "opencode/big-pickle\n").unwrap();
    assert_eq!(a.list_models().len(), 1);
    assert!(tmp.exists() && !cache.exists());
    // Ya terminó: se promueve a la lista vigente.
    age(&tmp, 120);
    let ids: Vec<_> = a.list_models().into_iter().map(|m| m.id).collect();
    assert_eq!(ids, ["opencode/default", "opencode/big-pickle"]);
    assert!(cache.exists() && !tmp.exists());
    // Una consulta que no dejó modelos válidos se descarta sin pisar la lista buena.
    std::fs::write(&tmp, "error: algo falló\n").unwrap();
    age(&tmp, 120);
    assert_eq!(a.list_models().len(), 2);
    assert!(!tmp.exists());
}

/// ADR-0019: en el harness de Symphony OpenCode niega sus herramientas propias; sin la marca no.
#[test]
fn the_symphony_harness_denies_opencodes_own_tools() {
    let mut req = request("default");
    req.hook = Some(symphony_adapter_common::HookCommand {
        program: "symphony".into(),
        args: vec!["hook".into(), "emit".into()],
    });
    req.env.push(("SYMPHONY_HARNESS".into(), "1".into()));
    let cfg = run_config(&req);
    for tool in [
        "bash", "edit", "read", "glob", "grep", "list", "webfetch", "task",
    ] {
        assert_eq!(cfg["permission"][tool], "deny", "{tool}");
    }
    // El permiso del worktree sigue ahí.
    assert!(cfg["permission"]["external_directory"].is_object());

    let plain = run_config(&request("default"));
    assert!(plain["permission"].get("bash").is_none());
}

/// Sin broker no se niegan las herramientas de OpenCode: el modelo se quedaría sin ninguna.
#[test]
fn the_harness_keeps_opencodes_tools_when_there_is_no_broker() {
    let mut req = request("default");
    req.hook = None;
    req.env.push(("SYMPHONY_HARNESS".into(), "1".into()));
    assert!(run_config(&req)["permission"].get("bash").is_none());
}
