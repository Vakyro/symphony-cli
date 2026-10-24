//! Las herramientas del harness de Symphony (ADR-0019): las mismas para cualquier modelo, sea cual
//! sea el CLI que lo lleva. Funciones puras sobre el sistema de archivos y el shell, sin MCP: el
//! broker (`mcp.rs`) solo las expone.
//!
//! Acceso local total (decisión de Leo, 2026-10-23): las rutas relativas cuelgan del directorio
//! del agente (su worktree), pero nada impide ir más allá. Todos los errores son texto para el
//! modelo: dicen qué pasó y cómo seguir.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use regex::RegexBuilder;

/// Líneas que devuelve `read` si no se pide otra cosa.
const READ_LINES: usize = 2000;
/// Una línea más larga se recorta (minificados, logs de una sola línea).
const LINE_MAX: usize = 2000;
/// Archivos de más de esto no se recorren en `grep`.
const GREP_FILE_MAX: u64 = 2 * 1024 * 1024;
const GREP_RESULTS: usize = 100;
const GLOB_RESULTS: usize = 500;
/// Lo que devuelve `shell` por flujo antes de recortar (cabeza y cola).
const SHELL_OUT_MAX: usize = 30_000;
pub const SHELL_TIMEOUT: Duration = Duration::from_secs(120);
pub const SHELL_TIMEOUT_MAX: Duration = Duration::from_secs(600);

fn resolve(root: &Path, p: &str) -> PathBuf {
    let p = Path::new(p);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}

fn cut_line(l: &str) -> String {
    if l.chars().count() <= LINE_MAX {
        return l.to_string();
    }
    let head: String = l.chars().take(LINE_MAX).collect();
    format!("{head}… [línea recortada]")
}

/// El archivo con sus líneas numeradas (`    12\ttexto`); `offset` es la primera línea (desde 1).
pub fn read(
    root: &Path,
    path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<String, String> {
    let file = resolve(root, path);
    let bytes =
        std::fs::read(&file).map_err(|e| format!("no pude leer {}: {e}", file.display()))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("{} no es texto UTF-8 (¿binario?)", file.display()))?;
    let total = text.lines().count();
    let first = offset.unwrap_or(1).max(1);
    let limit = limit.unwrap_or(READ_LINES).max(1);
    let shown: Vec<String> = text
        .lines()
        .enumerate()
        .skip(first - 1)
        .take(limit)
        .map(|(i, l)| format!("{:>6}\t{}", i + 1, cut_line(l)))
        .collect();
    if shown.is_empty() {
        return Ok(format!(
            "(sin líneas desde la {first}: el archivo tiene {total})"
        ));
    }
    let last = first - 1 + shown.len();
    let mut out = shown.join("\n");
    if last < total {
        out.push_str(&format!(
            "\n[mostradas {first}-{last} de {total}; pide el resto con `offset`]"
        ));
    }
    Ok(out)
}

/// Crea o reemplaza un archivo (con sus carpetas).
pub fn write(root: &Path, path: &str, content: &str) -> Result<String, String> {
    let file = resolve(root, path);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("no pude crear {}: {e}", dir.display()))?;
    }
    std::fs::write(&file, content)
        .map_err(|e| format!("no pude escribir {}: {e}", file.display()))?;
    Ok(format!(
        "escrito {} ({} bytes)",
        file.display(),
        content.len()
    ))
}

/// Cambia `old` por `new` en un archivo. `old` tiene que aparecer una sola vez, salvo con
/// `replace_all`. Si el archivo usa CRLF y `old` no, se reintenta con CRLF.
pub fn edit(
    root: &Path,
    path: &str,
    old: &str,
    new: &str,
    replace_all: bool,
) -> Result<String, String> {
    let file = resolve(root, path);
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("no pude leer {}: {e}", file.display()))?;
    if old.is_empty() {
        return Err("`old` está vacío: para crear un archivo usa `write`".into());
    }
    if old == new {
        return Err("`old` y `new` son iguales: no hay nada que cambiar".into());
    }
    let crlf = text.contains("\r\n") && !old.contains("\r\n");
    let (old, new) = if crlf && !text.contains(old) {
        (old.replace('\n', "\r\n"), new.replace('\n', "\r\n"))
    } else {
        (old.to_string(), new.to_string())
    };
    let count = text.matches(old.as_str()).count();
    match (count, replace_all) {
        (0, _) => Err(format!(
            "no encontré ese texto en {}: copia `old` tal cual está en el archivo (con `read`)",
            file.display()
        )),
        (n, false) if n > 1 => Err(format!(
            "ese texto aparece {n} veces en {}: añade líneas alrededor para que sea único, o usa `replace_all`",
            file.display()
        )),
        _ => {
            let changed = if replace_all {
                text.replace(old.as_str(), &new)
            } else {
                text.replacen(old.as_str(), &new, 1)
            };
            std::fs::write(&file, changed)
                .map_err(|e| format!("no pude escribir {}: {e}", file.display()))?;
            Ok(format!("editado {} ({count} cambio(s))", file.display()))
        }
    }
}

fn walker(base: &Path, glob: Option<&str>) -> Result<ignore::Walk, String> {
    let mut b = WalkBuilder::new(base);
    // `.gitignore` vale aunque la carpeta no sea un repo (proyectos de documentos, ADR-0020).
    b.require_git(false);
    if let Some(g) = glob {
        let mut o = OverrideBuilder::new(base);
        o.add(g)
            .map_err(|e| format!("patrón inválido `{g}`: {e}"))?;
        b.overrides(
            o.build()
                .map_err(|e| format!("patrón inválido `{g}`: {e}"))?,
        );
    }
    Ok(b.build())
}

fn shown(base: &Path, p: &Path) -> String {
    p.strip_prefix(base)
        .unwrap_or(p)
        .display()
        .to_string()
        .replace('\\', "/")
}

/// Archivos que cumplen un patrón (`**/*.rs`, `src/*.ts`), sin lo que git ignora.
pub fn glob(root: &Path, pattern: &str, path: Option<&str>) -> Result<String, String> {
    let base = path.map_or_else(|| root.to_path_buf(), |p| resolve(root, p));
    let mut found: Vec<String> = Vec::new();
    for entry in walker(&base, Some(pattern))?.flatten() {
        if entry.file_type().is_some_and(|t| t.is_file()) {
            found.push(shown(&base, entry.path()));
        }
    }
    found.sort();
    if found.is_empty() {
        return Ok("Sin resultados.".into());
    }
    let total = found.len();
    found.truncate(GLOB_RESULTS);
    let mut out = found.join("\n");
    if total > GLOB_RESULTS {
        out.push_str(&format!("\n[{total} en total; afina el patrón]"));
    }
    Ok(out)
}

/// Líneas que cumplen una expresión regular, como `ruta:línea:texto`.
pub fn grep(
    root: &Path,
    pattern: &str,
    path: Option<&str>,
    glob: Option<&str>,
    ignore_case: bool,
    max: Option<usize>,
) -> Result<String, String> {
    let re = RegexBuilder::new(pattern)
        .case_insensitive(ignore_case)
        .build()
        .map_err(|e| format!("expresión regular inválida: {e}"))?;
    let base = path.map_or_else(|| root.to_path_buf(), |p| resolve(root, p));
    let max = max.unwrap_or(GREP_RESULTS).max(1);
    let mut hits: Vec<String> = Vec::new();
    let mut total = 0;
    let mut files: Vec<PathBuf> = Vec::new();
    if base.is_file() {
        files.push(base.clone());
    } else {
        for entry in walker(&base, glob)?.flatten() {
            if entry.file_type().is_some_and(|t| t.is_file()) {
                files.push(entry.into_path());
            }
        }
    }
    files.sort();
    let rel_base = if base.is_file() {
        base.parent().unwrap_or(&base).to_path_buf()
    } else {
        base
    };
    for file in files {
        if std::fs::metadata(&file).is_ok_and(|m| m.len() > GREP_FILE_MAX) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue; // binario o no UTF-8
        };
        for (i, line) in text.lines().enumerate() {
            if re.is_match(line) {
                total += 1;
                if hits.len() < max {
                    hits.push(format!(
                        "{}:{}:{}",
                        shown(&rel_base, &file),
                        i + 1,
                        cut_line(line.trim_end())
                    ));
                }
            }
        }
    }
    if hits.is_empty() {
        return Ok("Sin resultados.".into());
    }
    let mut out = hits.join("\n");
    if total > hits.len() {
        out.push_str(&format!(
            "\n[{total} coincidencias; muestro {}]",
            hits.len()
        ));
    }
    Ok(out)
}

/// Cabeza y cola de una salida larga.
fn clip(s: &str) -> String {
    if s.len() <= SHELL_OUT_MAX {
        return s.to_string();
    }
    let half = SHELL_OUT_MAX / 2;
    let head_end = (0..=half)
        .rev()
        .find(|i| s.is_char_boundary(*i))
        .unwrap_or(0);
    let tail_start = (s.len() - half..=s.len())
        .find(|i| s.is_char_boundary(*i))
        .unwrap_or(s.len());
    format!(
        "{}\n[… {} bytes omitidos …]\n{}",
        &s[..head_end],
        tail_start - head_end,
        &s[tail_start..]
    )
}

/// El shell de la máquina: Git Bash en Windows si está (`bash.exe` del PATH suele ser el lanzador
/// de WSL, que no es lo que se quiere), si no PowerShell; `sh` en el resto.
fn shell_command(command: &str) -> tokio::process::Command {
    #[cfg(windows)]
    {
        let git_bash = ["ProgramFiles", "ProgramFiles(x86)", "LocalAppData"]
            .iter()
            .filter_map(std::env::var_os)
            .map(|d| PathBuf::from(d).join("Git").join("bin").join("bash.exe"))
            .find(|p| p.is_file());
        if let Some(bash) = git_bash {
            let mut c = tokio::process::Command::new(bash);
            c.arg("-c").arg(command);
            return c;
        }
        let mut c = tokio::process::Command::new("powershell.exe");
        c.args(["-NoProfile", "-NonInteractive", "-Command", command]);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(command);
        c
    }
}

/// Corre un comando en `cwd` (o en `root`) y devuelve su código y su salida. Pasado el tiempo se
/// mata el árbol de procesos.
pub async fn shell(
    root: &Path,
    command: &str,
    cwd: Option<&str>,
    timeout: Option<Duration>,
) -> Result<String, String> {
    let dir = cwd.map_or_else(|| root.to_path_buf(), |c| resolve(root, c));
    let timeout = timeout.unwrap_or(SHELL_TIMEOUT).min(SHELL_TIMEOUT_MAX);
    let mut cmd = shell_command(command);
    cmd.current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("no pude lanzar el shell en {}: {e}", dir.display()))?;
    let pid = child.id();
    // Se lee la salida aparte: así, al vencer el tiempo, el árbol se mata antes de soltar el hijo.
    let (out_task, err_task) = (drain(child.stdout.take()), drain(child.stderr.take()));
    match tokio::time::timeout(timeout, child.wait()).await {
        Err(_) => {
            kill_tree(pid);
            let _ = child.kill().await;
            out_task.abort();
            err_task.abort();
            Err(format!(
                "el comando pasó de {} s y se detuvo; lánzalo en segundo plano o acótalo",
                timeout.as_secs()
            ))
        }
        Ok(Err(e)) => Err(format!("falló la ejecución: {e}")),
        Ok(Ok(status)) => {
            // Un proceso que dejó corriendo en segundo plano puede mantener abierta la salida.
            let grab = |t: tokio::task::JoinHandle<Vec<u8>>| async move {
                tokio::time::timeout(Duration::from_secs(2), t)
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .unwrap_or_default()
            };
            let (stdout, stderr) = (grab(out_task).await, grab(err_task).await);
            let stdout = String::from_utf8_lossy(&stdout);
            let stderr = String::from_utf8_lossy(&stderr);
            let code = status
                .code()
                .map_or_else(|| "señal".to_string(), |c| c.to_string());
            let mut text = format!("código de salida: {code}");
            if !stdout.trim().is_empty() {
                text.push_str(&format!(
                    "
--- stdout ---
{}",
                    clip(stdout.trim_end())
                ));
            }
            if !stderr.trim().is_empty() {
                text.push_str(&format!(
                    "
--- stderr ---
{}",
                    clip(stderr.trim_end())
                ));
            }
            Ok(text)
        }
    }
}

/// Lee un flujo hasta que se cierra, en su propia tarea.
fn drain(
    s: Option<impl tokio::io::AsyncRead + Unpin + Send + 'static>,
) -> tokio::task::JoinHandle<Vec<u8>> {
    tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(mut s) = s {
            let _ = tokio::io::AsyncReadExt::read_to_end(&mut s, &mut buf).await;
        }
        buf
    })
}

/// `kill_on_drop` solo mata al shell; los hijos que dejó hay que matarlos por el árbol.
fn kill_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("pkill")
            .args(["-KILL", "-P", &pid.to_string()])
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().to_path_buf();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/a.rs"),
            "fn uno() {}\nfn dos() {}\nfn tres() {}\n",
        )
        .unwrap();
        std::fs::write(root.join("src/b.txt"), "nada\nUNO aquí\n").unwrap();
        std::fs::write(root.join(".gitignore"), "ignorado/\n").unwrap();
        std::fs::create_dir_all(root.join("ignorado")).unwrap();
        std::fs::write(root.join("ignorado/c.rs"), "fn uno() {}\n").unwrap();
        (d, root)
    }

    #[test]
    fn read_numbers_lines_and_pages() {
        let (_d, root) = tree();
        let all = read(&root, "src/a.rs", None, None).unwrap();
        assert!(all.contains("     2\tfn dos() {}"), "{all}");
        let page = read(&root, "src/a.rs", Some(2), Some(1)).unwrap();
        assert!(
            page.contains("fn dos") && !page.contains("fn uno"),
            "{page}"
        );
        assert!(page.contains("mostradas 2-2 de 3"), "{page}");
        assert!(
            read(&root, "no-existe", None, None)
                .unwrap_err()
                .contains("no pude leer")
        );
    }

    #[test]
    fn write_then_edit_with_the_uniqueness_rules() {
        let (_d, root) = tree();
        write(&root, "nuevo/x.txt", "uno\ndos\nuno\n").unwrap();
        // Dos apariciones: pide contexto o replace_all.
        let err = edit(&root, "nuevo/x.txt", "uno", "UNO", false).unwrap_err();
        assert!(err.contains("2 veces"), "{err}");
        edit(&root, "nuevo/x.txt", "dos\nuno", "dos\nUNO", false).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("nuevo/x.txt")).unwrap(),
            "uno\ndos\nUNO\n"
        );
        edit(&root, "nuevo/x.txt", "uno", "x", true).unwrap();
        assert!(
            edit(&root, "nuevo/x.txt", "zzz", "y", false)
                .unwrap_err()
                .contains("no encontré")
        );
        assert!(edit(&root, "nuevo/x.txt", "x", "x", false).is_err());
    }

    #[test]
    fn edit_matches_crlf_files_with_lf_text() {
        let (_d, root) = tree();
        std::fs::write(root.join("w.txt"), "a\r\nb\r\nc\r\n").unwrap();
        edit(&root, "w.txt", "a\nb", "A\nB", false).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("w.txt")).unwrap(),
            "A\r\nB\r\nc\r\n"
        );
    }

    #[test]
    fn glob_and_grep_respect_gitignore() {
        let (_d, root) = tree();
        assert_eq!(glob(&root, "**/*.rs", None).unwrap(), "src/a.rs");
        let hits = grep(&root, "uno", None, None, true, None).unwrap();
        assert!(hits.contains("src/a.rs:1:fn uno"), "{hits}");
        assert!(hits.contains("src/b.txt:2:UNO"), "{hits}");
        assert!(!hits.contains("ignorado"), "{hits}");
        let only_rs = grep(&root, "uno", None, Some("*.rs"), false, None).unwrap();
        assert!(!only_rs.contains("b.txt"), "{only_rs}");
        assert_eq!(
            grep(&root, "inexistente", None, None, false, None).unwrap(),
            "Sin resultados."
        );
        assert!(
            grep(&root, "(", None, None, false, None)
                .unwrap_err()
                .contains("inválida")
        );
    }

    #[tokio::test]
    async fn shell_reports_exit_code_output_and_times_out() {
        let (_d, root) = tree();
        let ok = shell(&root, "echo hola", None, None).await.unwrap();
        assert!(
            ok.starts_with("código de salida: 0") && ok.contains("hola"),
            "{ok}"
        );
        let bad = shell(&root, "exit 3", None, None).await.unwrap();
        assert!(bad.starts_with("código de salida: 3"), "{bad}");
        let started = std::time::Instant::now();
        let slow = shell(&root, "sleep 5", None, Some(Duration::from_millis(300)))
            .await
            .unwrap_err();
        assert!(slow.contains("se detuvo"), "{slow}");
        // El timeout corta de verdad: no espera a que el comando termine solo.
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
    }
}
