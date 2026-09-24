//! Logs del daemon (STACK §21): `tracing` a un archivo rotativo diario en
//! `~/.symphony/logs/`, pasando cada línea por el redactor central (STACK §22).
//! Sin telemetría: nada sale de la máquina.
//!
//! Spans recomendados: `project_id`, `session_id`, `agent_id`, `run_id`,
//! `provider`, `model`, `tool_call_id`.

use std::io::{self, Write};
use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

/// Días de logs que se conservan.
const MAX_LOG_FILES: usize = 14;

/// Envuelve un writer y redacta secretos antes de escribir. El formateador
/// de `tracing-subscriber` escribe cada evento completo en una sola llamada.
pub struct RedactingWriter<W>(pub W);

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        self.0.write_all(symphony_core::redact(&text).as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[derive(Clone)]
pub struct RedactingMakeWriter<M>(pub M);

impl<'a, M: MakeWriter<'a>> MakeWriter<'a> for RedactingMakeWriter<M> {
    type Writer = RedactingWriter<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter(self.0.make_writer())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    #[error("no se pudo crear el archivo de log en {dir}: {source}")]
    Appender {
        dir: String,
        source: tracing_appender::rolling::InitError,
    },
    #[error("filtro de log inválido `{level}`: {source}")]
    Filter {
        level: String,
        source: tracing_subscriber::filter::ParseError,
    },
    #[error("ya había un logger global instalado")]
    AlreadyInstalled,
}

/// Instala el logger global del daemon. Hay que conservar el `WorkerGuard`
/// mientras viva el proceso: al soltarlo se vacía el buffer al archivo.
pub fn init(logs_dir: &Path, level: &str) -> Result<WorkerGuard, LoggingError> {
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("symphonyd")
        .filename_suffix("log")
        .max_log_files(MAX_LOG_FILES)
        .build(logs_dir)
        .map_err(|source| LoggingError::Appender {
            dir: logs_dir.display().to_string(),
            source,
        })?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_new(level).map_err(|source| LoggingError::Filter {
        level: level.to_string(),
        source,
    })?;
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(RedactingMakeWriter(writer))
        .try_init()
        .map_err(|_| LoggingError::AlreadyInstalled)?;
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .map_err(|_| io::Error::other("lock"))?
                .extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn spans_are_kept_and_secrets_redacted() {
        let capture = Capture::default();
        let sink = capture.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(RedactingMakeWriter(move || sink.clone()))
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("run", agent_id = "01J9AGENT", provider = "anthropic");
            let _e = span.enter();
            tracing::info!(
                header = "Authorization: Bearer sk-ant-api03-SECRETSECRETSECRET",
                "llamada fallida"
            );
            tracing::warn!("env OPENAI_API_KEY=sk-proj-abcdefghijklmnopqrstuvwx");
        });
        let out = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        assert!(out.contains("agent_id=\"01J9AGENT\""), "{out}");
        assert!(out.contains("provider=\"anthropic\""), "{out}");
        assert!(!out.contains("SECRETSECRETSECRET"), "{out}");
        assert!(!out.contains("abcdefghijklmnopqrstuvwx"), "{out}");
        assert!(out.contains("[REDACTED]"), "{out}");
    }

    #[test]
    fn rolling_file_is_created_in_logs_dir() {
        let dir = tempfile::tempdir().unwrap();
        let appender = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix("symphonyd")
            .filename_suffix("log")
            .build(dir.path())
            .unwrap();
        let mut w = RedactingWriter(appender);
        w.write_all(b"GITHUB_TOKEN=ghp_AbCdEfGhIjKlMnOpQrStUvWxYz0123\n")
            .unwrap();
        w.flush().unwrap();
        let files: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().collect();
        assert_eq!(files.len(), 1);
        let name = files[0].file_name().to_string_lossy().into_owned();
        assert!(
            name.starts_with("symphonyd") && name.ends_with(".log"),
            "{name}"
        );
        let text = std::fs::read_to_string(files[0].path()).unwrap();
        assert_eq!(text, "GITHUB_TOKEN=[REDACTED]\n");
    }
}
