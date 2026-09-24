mod client;

use std::time::{Duration, Instant};

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
        Some(Cmd::Status) => {
            let mut conn = client::connect_or_start(home).await?;
            print_status(&client::call(&mut conn, "status", json!({})).await?);
        }
        Some(Cmd::Daemon {
            action: DaemonCmd::Status,
        }) => match client::try_connect(home).await {
            Some(mut conn) => print_status(&client::call(&mut conn, "status", json!({})).await?),
            None => println!("daemon: detenido"),
        },
        Some(Cmd::Daemon {
            action: DaemonCmd::Start,
        }) => match client::try_connect(home).await {
            Some(mut conn) => {
                let status = client::call(&mut conn, "status", json!({})).await?;
                println!("daemon: ya estaba corriendo (pid {})", status["pid"]);
            }
            None => {
                let pid = client::start_daemon(home).await?;
                println!("daemon: iniciado (pid {pid})");
            }
        },
        Some(Cmd::Daemon {
            action: DaemonCmd::Stop,
        }) => match client::try_connect(home).await {
            None => println!("daemon: no estaba corriendo"),
            Some(mut conn) => {
                client::call(&mut conn, "shutdown", json!({})).await?;
                drop(conn);
                let deadline = Instant::now() + Duration::from_secs(10);
                while client::try_connect(home).await.is_some() {
                    if Instant::now() > deadline {
                        miette::bail!(
                            "el daemon recibió la orden de apagarse pero sigue respondiendo"
                        );
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
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
