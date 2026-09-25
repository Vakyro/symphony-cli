mod client;
mod hook;

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
            println!("La TUI de Symphony llega en la versión 0.1 (P07).");
            println!("Mientras tanto: `symphony status` o `symphony --help`.");
        }
        Some(Cmd::Hook {
            action: HookCmd::Emit,
        }) => hook::emit(),
        Some(Cmd::Providers { action }) => {
            let method = if matches!(action, Some(ProvidersCmd::Refresh)) {
                "providers.refresh"
            } else {
                "providers.list"
            };
            let mut conn = client::connect_or_start(home).await?;
            print_providers(&client::call(&mut conn, method, json!({})).await?);
        }
        Some(Cmd::Status) => {
            let mut conn = client::connect_or_start(home).await?;
            print_status(&client::call(&mut conn, "status", json!({})).await?);
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

fn print_status(status: &Value) {
    let uptime_s = status["uptime_ms"].as_u64().unwrap_or(0) / 1000;
    println!("daemon: corriendo");
    println!("  pid:      {}", status["pid"]);
    println!("  versión:  {}", status["version"].as_str().unwrap_or("?"));
    println!("  activo:   {}", human_duration(uptime_s));
    println!("  home:     {}", status["home"].as_str().unwrap_or("?"));
}

fn print_providers(v: &Value) {
    let empty = Vec::new();
    let rows = v["providers"].as_array().unwrap_or(&empty);
    if rows.is_empty() {
        println!("No hay proveedores registrados.");
        return;
    }
    println!(
        "{:<10} {:<10} {:<10} {:>7}  RUTA",
        "PROVEEDOR", "ESTADO", "VERSIÓN", "MODELOS"
    );
    for p in rows {
        let text = |k: &str| p[k].as_str().unwrap_or("—").to_string();
        println!(
            "{:<10} {:<10} {:<10} {:>7}  {}",
            text("display_name"),
            text("setup_state"),
            text("cli_version"),
            p["models"],
            text("cli_path")
        );
    }
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
    fn cli_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
