use std::process::ExitCode;

use miette::{IntoDiagnostic, WrapErr};
use symphony_core::SymphonyHome;
use tokio_util::sync::CancellationToken;

/// El daemon no compite con los CLIs: pocos workers (STACK §39).
const WORKER_THREADS: usize = 2;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("symphonyd {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        None => match run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(report) => {
                eprintln!("{report:?}");
                ExitCode::FAILURE
            }
        },
        Some(_) => {
            eprintln!("uso: symphonyd [--version]\nNormalmente lo arranca `symphony`.");
            ExitCode::from(2)
        }
    }
}

fn run() -> miette::Result<()> {
    let home = SymphonyHome::resolve().into_diagnostic()?;
    let config = symphony_core::load_or_create(&home).into_diagnostic()?;
    let _log_guard = symphony_daemon::logging::init(&home.logs_dir(), &config.logging.level)
        .into_diagnostic()
        .wrap_err("no se pudo iniciar el log")?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(WORKER_THREADS)
        .enable_all()
        .build()
        .into_diagnostic()?;
    runtime
        .block_on(async {
            let shutdown = CancellationToken::new();
            let on_signal = shutdown.clone();
            tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    tracing::info!("señal de interrupción recibida");
                    on_signal.cancel();
                }
            });
            symphony_daemon::server::serve(home.root(), shutdown).await
        })
        .map_err(|e| {
            tracing::error!(error = %e, "el daemon terminó con error");
            e
        })
        .into_diagnostic()
}
