mod client;
mod hook;
mod mcp;

use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use miette::IntoDiagnostic;
use serde_json::{Value, json};
use symphony_core::SymphonyHome;

/// Symphony: coordina tus CLIs de IA para código desde una sola terminal.
#[derive(Parser)]
#[command(name = "symphony", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Estado del daemon (lo arranca si no está vivo).
    Status,
    /// Controla el daemon `symphonyd`.
    Daemon {
        #[command(subcommand)]
        action: DaemonCmd,
    },
    /// CLIs de IA detectados (Claude Code, Codex, …).
    Providers {
        #[command(subcommand)]
        action: Option<ProvidersCmd>,
    },
    /// Uso de tokens por proveedor y modelo: lo que informa cada CLI frente a lo que se estima.
    Usage {
        /// Cuántos días hacia atrás (por defecto 7).
        #[arg(long, default_value_t = 7)]
        days: i64,
    },
    /// Crea un agente nuevo con su tarea y workspace aislado.
    Spawn {
        /// Tarea u objetivo del agente.
        task: String,
        /// Modelo canónico exacto (ej. `claude/sonnet`, `codex/gpt-5.x`).
        #[arg(short, long)]
        model: Option<String>,
        /// Profile de modelo (ej. `@code`, `@fast`).
        #[arg(long)]
        profile: Option<String>,
        /// Política de failover (any, same-provider, none).
        #[arg(long, default_value = "any")]
        failover: String,
        /// Modo de contexto (raw, safe, balanced, aggressive).
        #[arg(long, default_value = "balanced")]
        context_mode: String,
        /// Prioridad (número entero).
        #[arg(long, default_value = "0")]
        priority: i64,
        /// Descripción adicional o contexto.
        #[arg(short, long)]
        description: Option<String>,
        /// Directorio raíz del proyecto (default: actual).
        #[arg(long)]
        cwd: Option<PathBuf>,
    },
    /// Lista los agentes del proyecto.
    Agents {
        /// Incluye agentes completados y cancelados.
        #[arg(short, long)]
        all: bool,
    },
    /// Envía un mensaje o instrucción al executor vivo del agente.
    Send {
        /// ID o número del agente (ej. `1`, `#1`, `01J...`).
        agent: String,
        /// Mensaje a enviar.
        message: String,
    },
    /// Pausa (suspende) el executor de un agente.
    Pause {
        /// ID o número del agente.
        agent: String,
    },
    /// Reanuda un agente pausado.
    Resume {
        /// ID o número del agente.
        agent: String,
    },
    /// Detiene un agente y su executor.
    Stop {
        /// ID o número del agente.
        agent: String,
    },
    /// Termina forzosamente el executor y cancela el agente.
    Kill {
        /// ID o número del agente.
        agent: String,
    },
    /// Cambia el modelo del agente a uno exacto y reinicia el executor.
    Switch {
        /// ID o número del agente.
        agent: String,
        /// Modelo exacto al que cambiar (ej. `claude/sonnet`, `codex/gpt-5.x`).
        #[arg(short, long)]
        model: Option<String>,
        /// Modelo posicional si no se usa `--model`.
        target_model: Option<String>,
        /// En vez de un modelo, un profile (`@code`, `@fast`…): Symphony elige entre los utilizables.
        #[arg(short, long, conflicts_with_all = ["model", "target_model"])]
        profile: Option<String>,
    },
    /// Por qué se eligió el modelo de un agente: decisiones de routing y candidatos evaluados.
    ExplainRoute {
        /// ID o número del agente.
        agent: String,
    },
    /// Context engine: qué entró al último handoff, estadísticas y el original de un objeto.
    Context {
        #[command(subcommand)]
        action: ContextCmd,
    },
    /// Hechos consolidados del proyecto (arquitectura, decisiones, restricciones, convenciones).
    Fact {
        #[command(subcommand)]
        action: FactCmd,
    },
    /// Pide a un review agent que revise el trabajo de un agente (solo recomienda; no integra).
    Review {
        /// ID o número del agente a revisar.
        agent: String,
        /// Modelo exacto del revisor (ej. `claude/sonnet`).
        #[arg(long)]
        model: Option<String>,
        /// Profile del revisor (por defecto `@review`).
        #[arg(long, conflicts_with = "model")]
        profile: Option<String>,
    },
    /// El chat general del proyecto (la rama `symphony/chat`).
    Chat {
        #[command(subcommand)]
        action: ChatCmd,
    },
    /// Muestra el git diff del worktree del agente contra su commit base.
    Diff {
        /// ID o número del agente.
        agent: String,
    },
    /// Muestra el historial de mensajes/conversación del agente.
    Logs {
        /// ID o número del agente.
        agent: String,
        /// Cantidad máxima de mensajes a mostrar.
        #[arg(short, long)]
        limit: Option<usize>,
    },
    /// Abre la sesión del agente en el CLI oficial (Claude Code, Codex) en una terminal nueva.
    Attach {
        /// ID o número del agente.
        agent: String,
        /// Solo imprime el comando, sin abrir una terminal.
        #[arg(long)]
        print: bool,
    },
    /// Inspecciona en detalle el estado, tarea, worktree, checkpoints y runs del agente.
    Inspect {
        /// ID o número del agente.
        agent: String,
    },
    /// Skills compartidas entre proveedores (`~/.symphony/skills`, `.symphony/skills`).
    Skills {
        #[command(subcommand)]
        action: Option<ToggleCmd>,
    },
    /// Servidores MCP compartidos (`mcp.toml`) y el broker de contexto.
    Mcp {
        #[command(subcommand)]
        action: McpCmd,
    },
    /// Uso interno: lo ejecutan los hooks de los CLIs.
    #[command(hide = true)]
    Hook {
        #[command(subcommand)]
        action: HookCmd,
    },
}

#[derive(Subcommand)]
enum ProvidersCmd {
    /// Vuelve a detectar los CLIs instalados.
    Refresh,
}

#[derive(Subcommand)]
enum ChatCmd {
    /// Integra el chat en `main`: sin opciones muestra el preflight; `--review` pide la revisión;
    /// `--yes` hace el merge (nunca sin tu confirmación).
    Merge {
        /// Pide a un review agent que revise el chat (solo recomienda).
        #[arg(long, conflicts_with = "yes")]
        review: bool,
        /// Confirma e integra el chat en `main` con un commit de merge.
        #[arg(long)]
        yes: bool,
        /// Con `--yes`: integra aunque el revisor haya fallado o no haya revisión.
        #[arg(long, requires = "yes")]
        skip_review: bool,
        /// Corre la validación del proyecto en el preflight (puede tardar).
        #[arg(long, conflicts_with_all = ["review", "yes"])]
        validate: bool,
        /// Con `--review`: modelo exacto del revisor (ej. `claude/sonnet`).
        #[arg(long, requires = "review")]
        model: Option<String>,
        /// Con `--review`: profile del revisor (por defecto `@review`).
        #[arg(long, requires = "review", conflicts_with = "model")]
        profile: Option<String>,
    },
}

#[derive(Subcommand)]
enum FactCmd {
    /// Registra un hecho vigente (`CURRENT`) sobre el proyecto.
    Set {
        /// Clave única del hecho (ej. `auth_storage`, `db_engine`).
        key: String,
        /// Valor o descripción del hecho.
        value: String,
        /// Tipo de hecho: ARCHITECTURE, DECISION, CONSTRAINT, CONVENTION (default: DECISION).
        #[arg(short, long, default_value = "DECISION")]
        kind: String,
    },
    /// Lista los hechos registrados para el proyecto.
    List {
        /// Incluye hechos reemplazados (SUPERSEDED) y en conflicto (CONFLICT).
        #[arg(short, long)]
        all: bool,
    },
}

#[derive(Subcommand)]
enum ContextCmd {
    /// Qué entró al último handoff de un agente y con qué fidelidad.
    Inspect {
        /// ID o número del agente.
        agent: String,
    },
    /// Compresión, coste de los handoffs y recuperaciones.
    Stats,
    /// El original completo de un objeto de contexto (`ctx://…`).
    Raw {
        /// Dirección `ctx://`.
        uri: String,
    },
}

#[derive(Subcommand)]
enum McpCmd {
    /// Sirve MCP por stdio: `context_search`, `context_retrieve` y `context_lines`.
    Serve,
    /// Lista los servidores de `mcp.toml` (usuario y proyecto) con su estado.
    List,
    /// Enciende un servidor para los agentes nuevos.
    Enable {
        /// Nombre en `mcp.toml`.
        name: String,
    },
    /// Apaga un servidor para los agentes nuevos.
    Disable {
        /// Nombre en `mcp.toml`.
        name: String,
    },
}

#[derive(Subcommand)]
enum ToggleCmd {
    /// Lista las skills encontradas.
    List,
    /// Enciende una skill para los agentes nuevos.
    Enable {
        /// Nombre de la carpeta de la skill.
        name: String,
    },
    /// Apaga una skill para los agentes nuevos.
    Disable {
        /// Nombre de la carpeta de la skill.
        name: String,
    },
}

#[derive(Subcommand)]
enum HookCmd {
    /// Envía al daemon el JSON del hook que llega por stdin.
    Emit,
}

#[derive(Subcommand)]
enum DaemonCmd {
    /// Arranca el daemon si no está corriendo.
    Start,
    /// Detiene el daemon.
    Stop,
    /// Dice si el daemon está corriendo, sin arrancarlo.
    Status,
}

/// `symphony skills|mcp list|enable|disable`: llama al daemon con el proyecto del directorio actual.
async fn shared_command(
    home: &std::path::Path,
    method: &str,
    toggle: Option<(String, bool)>,
    what: &str,
) -> miette::Result<()> {
    let cwd = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .display()
        .to_string();
    let mut conn = client::connect_or_start(home).await?;
    if let Some((name, enabled)) = toggle {
        client::call(
            &mut conn,
            method,
            json!({ "project_root": cwd, "name": name, "enabled": enabled }),
        )
        .await?;
        println!(
            "{what} `{name}` {} (vale para los agentes que se creen o continúen desde ahora)",
            if enabled { "encendido" } else { "apagado" }
        );
        return Ok(());
    }
    let res = client::call(&mut conn, method, json!({ "project_root": cwd })).await?;
    print_shared(&res);
    Ok(())
}

fn print_shared(res: &Value) {
    let on = |v: &Value| {
        if v["enabled"].as_bool() == Some(true) {
            "on "
        } else {
            "off"
        }
    };
    if let Some(skills) = res["skills"].as_array() {
        if skills.is_empty() {
            println!(
                "no hay skills compartidas: crea `~/.symphony/skills/<nombre>/SKILL.md` o `.symphony/skills/<nombre>/SKILL.md`"
            );
        }
        for s in skills {
            let extra = if s["shadowed"].as_bool() == Some(true) {
                " (tapada por la del proyecto)"
            } else {
                ""
            };
            println!(
                "{} {:<24} {:<8} {}{extra}",
                on(s),
                s["name"].as_str().unwrap_or(""),
                s["scope"].as_str().unwrap_or(""),
                s["description"].as_str().unwrap_or("")
            );
        }
    }
    if let Some(servers) = res["servers"].as_array() {
        if servers.is_empty() {
            println!(
                "no hay servidores MCP: define `[servers.<nombre>]` en `~/.symphony/mcp.toml` o `.symphony/mcp.toml`"
            );
        }
        for s in servers {
            let target = s["command"]
                .as_str()
                .or_else(|| s["url"].as_str())
                .unwrap_or("");
            let note = s["note"]
                .as_str()
                .map(|n| format!(" — {n}"))
                .unwrap_or_default();
            println!(
                "{} {:<24} {:<8} {:<6} {target}{note}",
                on(s),
                s["name"].as_str().unwrap_or(""),
                s["scope"].as_str().unwrap_or(""),
                s["transport"].as_str().unwrap_or("")
            );
        }
        for w in res["warnings"].as_array().into_iter().flatten() {
            eprintln!("aviso: {}", w.as_str().unwrap_or(""));
        }
    }
}

fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    // Antes que nada: un hook nunca puede fallar por el home, el runtime o el daemon.
    if let Some(Cmd::Hook {
        action: HookCmd::Emit,
    }) = cli.command
    {
        hook::emit();
        return Ok(());
    }
    let home = SymphonyHome::resolve().into_diagnostic()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .into_diagnostic()?;
    runtime.block_on(run(cli, home))
}

async fn run(cli: Cli, home: SymphonyHome) -> miette::Result<()> {
    let home = home.root();
    match cli.command {
        None => {
            use std::io::IsTerminal;
            if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
                println!(
                    "`symphony` sin argumentos abre la TUI y necesita una terminal interactiva."
                );
                println!("Sin terminal: `symphony status`, `symphony agents` o `symphony --help`.");
                return Ok(());
            }
            let cwd = std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .display()
                .to_string();
            let requests = client::connect_or_start(home).await?;
            let events = client::connect_or_start(home).await?;
            symphony_tui::run(requests, events, cwd)
                .await
                .into_diagnostic()?;
        }
        Some(Cmd::Hook {
            action: HookCmd::Emit,
        }) => hook::emit(),
        Some(Cmd::Mcp {
            action: McpCmd::Serve,
        }) => mcp::serve(home.to_path_buf()).await?,
        Some(Cmd::Mcp { action }) => {
            let (method, toggle) = match action {
                McpCmd::List | McpCmd::Serve => ("mcp.list", None),
                McpCmd::Enable { name } => ("mcp.set_enabled", Some((name, true))),
                McpCmd::Disable { name } => ("mcp.set_enabled", Some((name, false))),
            };
            shared_command(home, method, toggle, "servidor").await?;
        }
        Some(Cmd::Skills { action }) => {
            let (method, toggle) = match action {
                None | Some(ToggleCmd::List) => ("skills.shared", None),
                Some(ToggleCmd::Enable { name }) => ("skills.set_enabled", Some((name, true))),
                Some(ToggleCmd::Disable { name }) => ("skills.set_enabled", Some((name, false))),
            };
            shared_command(home, method, toggle, "skill").await?;
        }
        Some(Cmd::Providers { action }) => {
            let method = if matches!(action, Some(ProvidersCmd::Refresh)) {
                "providers.refresh"
            } else {
                "providers.list"
            };
            let mut conn = client::connect_or_start(home).await?;
            print_providers(&client::call(&mut conn, method, json!({})).await?);
        }
        Some(Cmd::Usage { days }) => {
            let mut conn = client::connect_or_start(home).await?;
            print_usage(&client::call(&mut conn, "usage.get", json!({ "days": days })).await?);
        }
        Some(Cmd::Status) => {
            let mut conn = client::connect_or_start(home).await?;
            print_status(&client::call(&mut conn, "status", json!({})).await?);
        }
        Some(Cmd::Spawn {
            task,
            model,
            profile,
            failover,
            context_mode,
            priority,
            description,
            cwd,
        }) => {
            let cwd_str = cwd
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
                .display()
                .to_string();
            let mut conn = client::connect_or_start(home).await?;
            let res = client::call(
                &mut conn,
                "agent.create",
                json!({
                    "title": task,
                    "description": description,
                    "project_root": cwd_str,
                    "model": model,
                    "profile": profile,
                    "failover": db_value(&failover),
                    "context_mode": db_value(&context_mode),
                    "priority": priority,
                }),
            )
            .await?;
            print_spawn(&res);
        }
        Some(Cmd::Agents { all }) => {
            let cwd_str = std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .display()
                .to_string();
            let mut conn = client::connect_or_start(home).await?;
            let res = client::call(
                &mut conn,
                "agent.list",
                json!({ "project_root": cwd_str, "all": all }),
            )
            .await?;
            print_agents(&res);
        }
        Some(Cmd::Send { agent, message }) => {
            let mut conn = client::connect_or_start(home).await?;
            let _ = client::call(
                &mut conn,
                "agent.send",
                json!({ "agent": agent, "text": message }),
            )
            .await?;
            println!("mensaje enviado al agente {agent}");
        }
        Some(Cmd::Pause { agent }) => {
            let mut conn = client::connect_or_start(home).await?;
            let _ = client::call(&mut conn, "agent.pause", json!({ "agent": agent })).await?;
            println!("agente {agent} pausado");
        }
        Some(Cmd::Resume { agent }) => {
            let mut conn = client::connect_or_start(home).await?;
            let _ = client::call(&mut conn, "agent.resume", json!({ "agent": agent })).await?;
            println!("agente {agent} reanudado");
        }
        Some(Cmd::Stop { agent }) => {
            let mut conn = client::connect_or_start(home).await?;
            let _ = client::call(&mut conn, "agent.stop", json!({ "agent": agent })).await?;
            println!("agente {agent} detenido");
        }
        Some(Cmd::Kill { agent }) => {
            let mut conn = client::connect_or_start(home).await?;
            let _ = client::call(&mut conn, "agent.kill", json!({ "agent": agent })).await?;
            println!("agente {agent} terminado");
        }
        Some(Cmd::Switch {
            agent,
            model,
            target_model,
            profile,
        }) => {
            let mut conn = client::connect_or_start(home).await?;
            let (params, target) = match (&profile, model.or(target_model)) {
                (Some(p), _) => (json!({ "agent": agent, "profile": p }), p.clone()),
                (None, Some(m)) => (json!({ "agent": agent, "model": m }), m),
                (None, None) => {
                    return Err(miette::miette!(
                        "falta especificar el modelo (ej. `claude/sonnet`) o un `--profile`"
                    ));
                }
            };
            let res = client::call(&mut conn, "agent.switch", params).await?;
            println!(
                "agente {agent} cambiado a {} (run {})",
                res["model"].as_str().unwrap_or(&target),
                res["run_id"].as_str().unwrap_or("?")
            );
        }
        Some(Cmd::ExplainRoute { agent }) => {
            let mut conn = client::connect_or_start(home).await?;
            let res = client::call(&mut conn, "route.explain", json!({ "agent": agent })).await?;
            print_route(&res);
        }
        Some(Cmd::Context { action }) => {
            let mut conn = client::connect_or_start(home).await?;
            match action {
                ContextCmd::Inspect { agent } => {
                    let res = client::call(&mut conn, "context.inspect", json!({ "agent": agent }))
                        .await?;
                    print_context_inspect(&res);
                }
                ContextCmd::Stats => {
                    let res = client::call(&mut conn, "context.stats", json!({})).await?;
                    print_context_stats(&res);
                }
                ContextCmd::Raw { uri } => {
                    let cwd = std::env::current_dir()
                        .unwrap_or_else(|_| PathBuf::from("."))
                        .display()
                        .to_string();
                    let res = client::call(
                        &mut conn,
                        "context.raw",
                        json!({ "uri": uri, "project_root": cwd }),
                    )
                    .await?;
                    print!("{}", res["text"].as_str().unwrap_or(""));
                }
            }
        }
        Some(Cmd::Chat {
            action:
                ChatCmd::Merge {
                    review,
                    yes,
                    skip_review,
                    validate,
                    model,
                    profile,
                },
        }) => {
            let cwd = std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .display()
                .to_string();
            let mut conn = client::connect_or_start(home).await?;
            if yes {
                let res = client::call(
                    &mut conn,
                    "chat.merge.integrate",
                    json!({ "project_root": cwd, "confirm": true, "skip_review": skip_review }),
                )
                .await?;
                println!(
                    "Chat integrado en la rama principal: {}",
                    res["commit"].as_str().unwrap_or("?")
                );
            } else if review {
                let res = client::call(
                    &mut conn,
                    "chat.merge.review",
                    json!({ "project_root": cwd, "model": model, "profile": profile }),
                )
                .await?;
                println!(
                    "Revisión pedida al agente {}. Cuando termine, corre `symphony chat merge` para ver su veredicto.",
                    res["reviewer_agent_id"].as_str().unwrap_or("?")
                );
            } else {
                let res = client::call(
                    &mut conn,
                    "chat.merge.prepare",
                    json!({ "project_root": cwd, "validate": validate }),
                )
                .await?;
                print_chat_merge(&res);
            }
        }
        Some(Cmd::Review {
            agent,
            model,
            profile,
        }) => {
            let mut conn = client::connect_or_start(home).await?;
            let res = client::call(
                &mut conn,
                "review.start",
                json!({ "agent": agent, "model": model, "profile": profile }),
            )
            .await?;
            println!(
                "Revisión pedida al agente {}. Su veredicto queda guardado al terminar \
                 (aparece en la vista del agente y en `symphony chat merge`).",
                res["reviewer_agent_id"].as_str().unwrap_or("?")
            );
        }
        Some(Cmd::Fact { action }) => {
            let cwd_str = std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .display()
                .to_string();
            let mut conn = client::connect_or_start(home).await?;
            match action {
                FactCmd::Set { key, value, kind } => {
                    let res = client::call(
                        &mut conn,
                        "fact.set",
                        json!({
                            "project_root": cwd_str,
                            "key": key,
                            "value": value,
                            "kind": kind,
                        }),
                    )
                    .await?;
                    println!(
                        "hecho registrado: [{}] {} = {} ({})",
                        res["kind"].as_str().unwrap_or(""),
                        res["key"].as_str().unwrap_or(""),
                        res["value"].as_str().unwrap_or(""),
                        res["status"].as_str().unwrap_or("CURRENT")
                    );
                }
                FactCmd::List { all } => {
                    let res = client::call(
                        &mut conn,
                        "fact.list",
                        json!({
                            "project_root": cwd_str,
                            "all": all,
                        }),
                    )
                    .await?;
                    print_facts(&res);
                }
            }
        }
        Some(Cmd::Diff { agent }) => {
            let mut conn = client::connect_or_start(home).await?;
            let res = client::call(&mut conn, "agent.diff", json!({ "agent": agent })).await?;
            let diff = res["diff"].as_str().unwrap_or("");
            if diff.trim().is_empty() {
                println!("Sin cambios sin commitear.");
            } else {
                print!("{diff}");
                if !diff.ends_with('\n') {
                    println!();
                }
            }
        }
        Some(Cmd::Logs { agent, limit }) => {
            let mut conn = client::connect_or_start(home).await?;
            let res = client::call(
                &mut conn,
                "agent.logs",
                json!({ "agent": agent, "limit": limit }),
            )
            .await?;
            let empty = Vec::new();
            let messages = res["messages"].as_array().unwrap_or(&empty);
            if messages.is_empty() {
                println!("Sin mensajes.");
            } else {
                for m in messages {
                    let role = m["role"].as_str().unwrap_or("MSG");
                    let content = m["content"].as_str().unwrap_or("");
                    println!("[{role}] {content}");
                }
            }
        }
        Some(Cmd::Attach { agent, print }) => {
            let mut conn = client::connect_or_start(home).await?;
            let res = client::call(
                &mut conn,
                "agent.attach",
                json!({ "agent": agent, "open": !print }),
            )
            .await?;
            let command = res["command"].as_str().unwrap_or("");
            if print {
                println!("{command}");
            } else if res["opened"].as_bool().unwrap_or(false) {
                println!(
                    "sesión del agente {agent} abierta en {} (terminal nueva)",
                    res["cli"].as_str().unwrap_or("el CLI")
                );
            } else {
                println!(
                    "no se pudo abrir una terminal ({}); córrelo a mano:",
                    res["error"].as_str().unwrap_or("?")
                );
                println!("  {command}");
            }
        }
        Some(Cmd::Inspect { agent }) => {
            let mut conn = client::connect_or_start(home).await?;
            let res = client::call(&mut conn, "agent.inspect", json!({ "agent": agent })).await?;
            print_inspect(&res);
        }
        Some(Cmd::Daemon {
            action: DaemonCmd::Status,
        }) => {
            let status = match client::try_connect(home).await {
                Some(mut conn) => client::call(&mut conn, "status", json!({})).await.map(Some),
                None => Ok(None),
            };
            match status {
                Ok(Some(s)) => print_status(&s),
                Ok(None) => println!("daemon: detenido"),
                // Murió justo mientras preguntábamos: si soltó el lock, está detenido.
                Err(e)
                    if e.is_connection_lost()
                        && !symphony_protocol::transport::daemon_lock_held(home) =>
                {
                    println!("daemon: detenido")
                }
                Err(e) => return Err(e.into()),
            }
        }
        Some(Cmd::Daemon {
            action: DaemonCmd::Start,
        }) => match client::ensure_running(home).await? {
            client::Ensured::Running(mut conn) => {
                let status = client::call(&mut conn, "status", json!({})).await?;
                println!("daemon: ya estaba corriendo (pid {})", status["pid"]);
            }
            client::Ensured::Started(pid, _) => println!("daemon: iniciado (pid {pid})"),
        },
        Some(Cmd::Daemon {
            action: DaemonCmd::Stop,
        }) => match client::try_connect(home).await {
            None => println!("daemon: no estaba corriendo"),
            Some(mut conn) => {
                client::call(&mut conn, "shutdown", json!({})).await?;
                drop(conn);
                if !client::wait_stopped(home, Duration::from_secs(10)).await {
                    miette::bail!("el daemon recibió la orden de apagarse pero sigue corriendo");
                }
                println!("daemon: detenido");
            }
        },
    }
    Ok(())
}

fn print_spawn(res: &Value) {
    println!("agente #{} creado", res["number"]);
    println!("  tarea:    {}", res["task_code"].as_str().unwrap_or(""));
    println!("  worktree: {}", res["worktree"].as_str().unwrap_or(""));
    println!("  rama:     {}", res["branch"].as_str().unwrap_or(""));
    if let Some(run) = res.get("run").and_then(Value::as_object) {
        println!(
            "  estado:   {} ({})",
            res["state"].as_str().unwrap_or(""),
            run["model"].as_str().unwrap_or("")
        );
    } else {
        println!("  estado:   {}", res["state"].as_str().unwrap_or(""));
    }
}

fn print_agents(res: &Value) {
    let empty = Vec::new();
    let rows = res["agents"].as_array().unwrap_or(&empty);
    if rows.is_empty() {
        println!("No hay agentes activos.");
        return;
    }
    println!("{:<4} {:<15} {:<20} TAREA", "#", "ESTADO", "MODELO");
    for a in rows {
        let num = a["number"].as_i64().unwrap_or(0);
        let state = a["state"].as_str().unwrap_or("—");
        let model = a["model_id"].as_str().unwrap_or("—");
        let task = a["task_title"].as_str().unwrap_or("—");
        println!(
            "{:<4} {:<15} {:<20} {}",
            format!("#{num}"),
            state,
            model,
            task
        );
    }
}

fn print_inspect(res: &Value) {
    let a = &res["agent"];
    let t = &res["task"];
    let w = &res["worktree"];
    let r = &res["current_run"];
    let c = &res["latest_checkpoint"];
    println!(
        "AGENTE #{}: {}",
        a["number"],
        a["id"].as_str().unwrap_or("")
    );
    let reason = a["state_reason"]
        .as_str()
        .map(|r| format!(" ({r})"))
        .unwrap_or_default();
    println!(
        "  estado:     {}{}",
        a["state"].as_str().unwrap_or(""),
        reason
    );
    println!(
        "  tarea:      {} · {}",
        t["code"].as_str().unwrap_or(""),
        t["title"].as_str().unwrap_or("")
    );
    if let Some(desc) = t["description"].as_str() {
        println!("  detalle:    {desc}");
    }
    if let Some(m) = r["model_id"].as_str() {
        println!("  modelo:     {m} (run #{})", r["seq"]);
    } else if let Some(req_m) = a["requested_model_id"].as_str() {
        println!("  modelo req: {req_m}");
    }
    if w.is_object() {
        println!(
            "  worktree:   {} (rama {})",
            w["path"].as_str().unwrap_or(""),
            w["branch"].as_str().unwrap_or("")
        );
    }
    println!(
        "  failover:   {}",
        a["failover_policy"].as_str().unwrap_or("")
    );
    println!("  contexto:   {}", a["context_mode"].as_str().unwrap_or(""));
    if c.is_object() {
        println!(
            "  checkpoint: #{} (commit {})",
            c["seq"],
            c["head_commit"].as_str().unwrap_or("—")
        );
        if let Some(next) = c["next_step"].as_str() {
            println!("  siguiente:  {next}");
        }
    }
}

fn print_status(status: &Value) {
    let uptime_s = status["uptime_ms"].as_u64().unwrap_or(0) / 1000;
    println!("daemon: corriendo");
    println!("  pid:      {}", status["pid"]);
    println!("  versión:  {}", status["version"].as_str().unwrap_or("?"));
    println!("  activo:   {}", human_duration(uptime_s));
    println!("  home:     {}", status["home"].as_str().unwrap_or("?"));
}

/// Cuota con su certeza: nunca un porcentaje que el CLI no informó (FLOW §12.3).
fn quota_label(health: &Value) -> String {
    match health["certainty"].as_str() {
        Some("KNOWN") => health["remaining"]
            .as_f64()
            .map_or("desconocida".into(), |r| {
                format!("{:.0} % restante", r * 100.0)
            }),
        Some("ESTIMATED") => "estimada".into(),
        _ => "desconocida".into(),
    }
}

fn print_providers(v: &Value) {
    let empty = Vec::new();
    let rows = v["providers"].as_array().unwrap_or(&empty);
    if rows.is_empty() {
        println!("No hay proveedores registrados.");
        return;
    }
    println!(
        "{:<10} {:<10} {:<13} {:<14} {:<10} {:>7}  RUTA",
        "PROVEEDOR", "ESTADO", "SALUD", "CUOTA", "VERSIÓN", "MODELOS"
    );
    for p in rows {
        let text = |k: &str| p[k].as_str().unwrap_or("—").to_string();
        println!(
            "{:<10} {:<10} {:<13} {:<14} {:<10} {:>7}  {}",
            text("display_name"),
            text("setup_state"),
            p["health"]["state"].as_str().unwrap_or("UNKNOWN"),
            quota_label(&p["health"]),
            text("cli_version"),
            p["models"],
            text("cli_path")
        );
    }
}

fn print_chat_merge(v: &Value) {
    println!(
        "Integrar `{}` en `{}` — {} commit(s) por integrar
",
        v["source"].as_str().unwrap_or("?"),
        v["target"].as_str().unwrap_or("?"),
        v["ahead"].as_u64().unwrap_or(0)
    );
    let empty = Vec::new();
    for c in v["checks"].as_array().unwrap_or(&empty) {
        let mark = match c["status"].as_str().unwrap_or("") {
            "PASS" => "✓",
            "WARN" => "!",
            _ => "✗",
        };
        println!(
            "  {mark} {:<17} {}",
            c["name"].as_str().unwrap_or("?"),
            c["detail"].as_str().unwrap_or("")
        );
    }
    println!();
    if v["ready"].as_bool().unwrap_or(false) {
        println!(
            "Listo. Pide la revisión con `symphony chat merge --review` e integra con `symphony chat merge --yes`."
        );
    } else {
        println!("Hay comprobaciones en ✗: corrígelas y vuelve a correr `symphony chat merge`.");
    }
}

fn print_route(v: &Value) {
    let empty = Vec::new();
    let decisions = v["decisions"].as_array().unwrap_or(&empty);
    let Some(d) = decisions.first() else {
        println!("Este agente todavía no tiene decisiones de routing registradas.");
        return;
    };
    println!(
        "Decisión más reciente: {} · {}\n",
        d["trigger"].as_str().unwrap_or("?"),
        d["profile"].as_str().unwrap_or("modelo exacto")
    );
    println!("{}\n", d["explanation"].as_str().unwrap_or(""));
    println!("{:<30} {:<11} {:>7}  MOTIVO", "MODELO", "ESTADO", "PUNTAJE");
    for c in d["candidates"].as_array().unwrap_or(&empty) {
        let eligible = c["eligible"].as_bool().unwrap_or(false);
        println!(
            "{:<30} {:<11} {:>7}  {}",
            c["model_id"].as_str().unwrap_or("?"),
            if eligible { "elegible" } else { "descartado" },
            c["score"]
                .as_f64()
                .map_or("—".into(), |s| format!("{s:.2}")),
            c["reject_reason"].as_str().unwrap_or("")
        );
    }
    if decisions.len() > 1 {
        println!("\nDecisiones anteriores:");
        for p in &decisions[1..] {
            println!(
                "  {} → {}",
                p["trigger"].as_str().unwrap_or("?"),
                p["selected"].as_str().unwrap_or("ninguno")
            );
        }
    }
}

fn print_context_inspect(v: &Value) {
    let h = &v["handoff"];
    if h.is_null() {
        println!("El agente no tiene handoffs.");
        return;
    }
    println!(
        "Handoff {} · modo {} · {} tokens enviados de {} en RAW · {} ms",
        h["id"].as_str().unwrap_or("?"),
        h["mode"].as_str().unwrap_or("?"),
        h["tokens_sent"],
        h["tokens_raw_estimate"],
        h["build_ms"]
    );
    let items = v["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        println!("(primer arranque: solo el objetivo, sin detalle por sección)");
        return;
    }
    println!(
        "{:<11} {:<10} {:>8}  ORIGEN",
        "SECCIÓN", "FIDELIDAD", "TOKENS"
    );
    for i in items {
        let fidelity = match i["fidelity"].as_i64() {
            Some(5) => "completo",
            Some(4) => "recortado",
            Some(3) => "resumen",
            Some(2) => "esqueleto",
            Some(1) => "referencia",
            Some(0) => "omitido",
            _ => "?",
        };
        println!(
            "{:<11} {:<10} {:>8}  {}",
            i["section"].as_str().unwrap_or("?"),
            fidelity,
            i["tokens"],
            i["path"].as_str().unwrap_or("")
        );
    }
}

fn print_context_stats(v: &Value) {
    println!(
        "{} objetos de contexto, {} chunks buscables.",
        v["objects"], v["chunks"]
    );
    let empty = Vec::new();
    let comp = v["compression"].as_array().unwrap_or(&empty);
    if comp.is_empty() {
        println!("Sin objetos comprimidos todavía.");
    }
    for c in comp {
        println!(
            "  {:<13} {:>5} objetos · {} → {} tokens",
            c["compressor"].as_str().unwrap_or("?"),
            c["objects"],
            c["tokens_original"],
            c["tokens_compressed"]
        );
    }
    for h in v["handoffs"].as_array().unwrap_or(&empty) {
        println!(
            "  handoffs {:<10} {:>4} · {} tokens enviados de {} en RAW · {} necesitaron recuperar · {} no continuaron",
            h["mode"].as_str().unwrap_or("?"),
            h["handoffs"],
            h["tokens_sent"],
            h["tokens_raw"],
            h["needed_retrieval"],
            h["failed_to_continue"]
        );
    }
    let r = &v["retrievals"];
    println!(
        "  recuperaciones: {} ({} sin resultado) · {} tokens devueltos",
        r["total"], r["misses"], r["tokens_returned"]
    );
}

fn print_facts(res: &Value) {
    let empty = Vec::new();
    let rows = res["facts"].as_array().unwrap_or(&empty);
    if rows.is_empty() {
        println!("No hay hechos registrados para este proyecto.");
        return;
    }
    println!("{:<14} {:<20} {:<12} VALOR", "TIPO", "CLAVE", "ESTADO");
    for f in rows {
        let text = |k: &str| f[k].as_str().unwrap_or("—").to_string();
        println!(
            "{:<14} {:<20} {:<12} {}",
            text("kind"),
            text("key"),
            text("status"),
            text("value")
        );
    }
}

fn print_usage(v: &Value) {
    let empty = Vec::new();
    let rows = v["usage"].as_array().unwrap_or(&empty);
    if rows.is_empty() {
        println!("Sin uso registrado en los últimos {} días.", v["days"]);
        return;
    }
    println!(
        "{:<12} {:<26} {:<10} {:>5} {:>12} {:>12}",
        "PROVEEDOR", "MODELO", "ORIGEN", "RUNS", "ENTRADA", "SALIDA"
    );
    for u in rows {
        let text = |k: &str| u[k].as_str().unwrap_or("—").to_string();
        println!(
            "{:<12} {:<26} {:<10} {:>5} {:>12} {:>12}",
            text("provider_id"),
            text("model_id"),
            if u["source"] == "REPORTED" {
                "informado"
            } else {
                "estimado"
            },
            u["runs"],
            u["tokens_in"],
            u["tokens_out"]
        );
    }
    println!(
        "«estimado»: el CLI no informó tokens; es una aproximación (~4 caracteres por token)."
    );
}

/// `same-provider` → `SAME_PROVIDER`: los flags aceptan la forma de CLI, la DB la suya.
fn db_value(flag: &str) -> String {
    flag.trim().to_ascii_uppercase().replace('-', "_")
}

fn human_duration(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs} s"),
        60..3600 => format!("{} min {} s", secs / 60, secs % 60),
        _ => format!("{} h {} min", secs / 3600, (secs % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_durations() {
        assert_eq!(human_duration(0), "0 s");
        assert_eq!(human_duration(59), "59 s");
        assert_eq!(human_duration(61), "1 min 1 s");
        assert_eq!(human_duration(3_725), "1 h 2 min");
    }

    #[test]
    fn flags_map_to_db_values() {
        use symphony_core::{ContextMode, FailoverPolicy};
        for (flag, want) in [
            ("any", FailoverPolicy::Any),
            ("same-provider", FailoverPolicy::SameProvider),
            ("none", FailoverPolicy::None),
        ] {
            assert_eq!(db_value(flag).parse::<FailoverPolicy>().ok(), Some(want));
        }
        assert_eq!(
            db_value("balanced").parse::<ContextMode>().ok(),
            Some(ContextMode::Balanced)
        );
    }

    #[test]
    fn cli_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
