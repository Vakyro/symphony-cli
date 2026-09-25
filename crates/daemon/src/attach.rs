//! «Abrir en el CLI» (attach, ADR-0005): la sesión del agente en el CLI oficial,
//! interactivo, en una terminal nueva del sistema. Symphony no la maneja: solo
//! la abre. Si no puede abrir una terminal, devuelve el comando para copiarlo.

use std::ffi::OsStr;
use std::process::{Command, Stdio};

use symphony_process::ProcessSpec;

/// Argumento para PowerShell/cmd: comillas si hace falta, `"` escapadas.
fn win_quote(arg: &OsStr) -> String {
    let s = arg.to_string_lossy();
    if !s.is_empty() && !s.contains([' ', '\t', '"', '&', '|', '<', '>', '^']) {
        return s.into_owned();
    }
    format!("\"{}\"", s.replace('"', "\\\""))
}

/// Argumento para `sh`: siempre entre comillas simples.
fn sh_quote(arg: &OsStr) -> String {
    format!("'{}'", arg.to_string_lossy().replace('\'', r"'\''"))
}

fn program_and_args(spec: &ProcessSpec, quote: fn(&OsStr) -> String) -> String {
    std::iter::once(quote(spec.program.as_os_str()))
        .chain(spec.args.iter().map(|a| quote(a)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Comando para correrlo a mano (PowerShell en Windows, `sh` en el resto).
pub fn command_line(spec: &ProcessSpec) -> String {
    let cwd = spec.cwd.clone().unwrap_or_default();
    if cfg!(windows) {
        format!(
            "Set-Location {}; & {}",
            win_quote(cwd.as_os_str()),
            program_and_args(spec, win_quote)
        )
    } else {
        format!(
            "cd {} && {}",
            sh_quote(cwd.as_os_str()),
            program_and_args(spec, sh_quote)
        )
    }
}

/// Abre una terminal nueva con el CLI. No espera a que el usuario la cierre.
pub fn open_terminal(spec: &ProcessSpec, title: &str) -> Result<(), String> {
    let mut cmd = terminal_command(spec, title);
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("no se pudo abrir una terminal: {e}"))?;
    // Se recoge en otro hilo: algunas terminales (Linux) no vuelven hasta cerrarse.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(windows)]
fn terminal_command(spec: &ProcessSpec, title: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let cwd = spec.cwd.clone().unwrap_or_default();
    // `start` abre una consola visible para el CLI; el primer argumento entre comillas es el título.
    let line = format!(
        "/c start \"{}\" /D {} {}",
        title.replace('"', "'"),
        win_quote(cwd.as_os_str()),
        program_and_args(spec, win_quote)
    );
    let mut cmd = Command::new("cmd");
    cmd.raw_arg(line);
    cmd
}

#[cfg(target_os = "macos")]
fn terminal_command(spec: &ProcessSpec, _title: &str) -> Command {
    let script = command_line(spec)
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let mut cmd = Command::new("osascript");
    cmd.args([
        "-e",
        &format!("tell application \"Terminal\" to do script \"{script}\""),
        "-e",
        "tell application \"Terminal\" to activate",
    ]);
    cmd
}

// ponytail: solo `x-terminal-emulator` (Debian/Ubuntu); sin él, el usuario copia el comando.
#[cfg(all(unix, not(target_os = "macos")))]
fn terminal_command(spec: &ProcessSpec, title: &str) -> Command {
    let mut cmd = Command::new("x-terminal-emulator");
    cmd.args(["-T", title, "-e", "sh", "-c", &command_line(spec)]);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ProcessSpec {
        ProcessSpec::new("/opt/my tools/claude")
            .args([
                "--resume",
                "abc-123",
                "-c",
                "sandbox_mode=\"workspace-write\"",
            ])
            .cwd("/home/leo/.symphony/worktrees/p/agent-001")
    }

    #[test]
    fn command_line_quotes_paths_with_spaces_and_quotes() {
        let line = command_line(&spec());
        if cfg!(windows) {
            assert!(line.starts_with("Set-Location "), "{line}");
            assert!(line.contains("& \""), "{line}");
            assert!(
                line.contains(r#""sandbox_mode=\"workspace-write\"""#),
                "{line}"
            );
            assert!(line.contains(" --resume abc-123 "), "{line}");
        } else {
            assert_eq!(
                line,
                "cd '/home/leo/.symphony/worktrees/p/agent-001' && '/opt/my tools/claude' \
                 '--resume' 'abc-123' '-c' 'sandbox_mode=\"workspace-write\"'"
            );
        }
    }

    #[test]
    fn sh_quote_escapes_single_quotes() {
        assert_eq!(sh_quote(OsStr::new("it's")), r"'it'\''s'");
    }
}
