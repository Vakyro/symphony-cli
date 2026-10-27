//! Las herramientas del harness de Symphony (ADR-0019): las mismas para cualquier modelo, sea cual
//! sea el CLI que lo lleva. Funciones puras sobre el sistema de archivos y el shell, sin MCP: el
//! broker (`mcp.rs`) solo las expone.
//!
//! Acceso local total (decisión de Leo, 2026-10-23): las rutas relativas cuelgan del directorio
//! del agente (su worktree), pero nada impide ir más allá. Todos los errores son texto para el
//! modelo: dicen qué pasó y cómo seguir.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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
/// Tope duro de `max_results`: la llamada no puede pedir una respuesta sin límite (P13.S5).
const GREP_MAX: usize = 1000;
const GLOB_RESULTS: usize = 500;
/// Lo que devuelve `shell` por flujo antes de recortar (cabeza y cola).
const SHELL_OUT_MAX: usize = 30_000;
pub const SHELL_TIMEOUT: Duration = Duration::from_secs(120);
/// Un `cargo test` en frío con otros agentes compilando pasa de 10 min (dogfooding 2026-10-24).
pub const SHELL_TIMEOUT_MAX: Duration = Duration::from_secs(1800);

fn resolve(root: &Path, p: &str) -> PathBuf {
    let p = Path::new(p);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}

/// Sufijo único para los temporales de `write_atomic`.
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Escribe el archivo de una vez: temporal en la misma carpeta, `sync_all` y `rename` encima.
/// Un fallo a mitad deja intacta la versión anterior; nunca se trunca el original (P13.S5).
fn write_atomic(file: &Path, content: &[u8]) -> Result<(), String> {
    // A través de un symlink se escribe su destino, y el archivo conserva sus permisos
    // (el bit ejecutable de un script), como con una escritura normal (revisión de P13).
    let target = std::fs::canonicalize(file).ok();
    let file = target.as_deref().unwrap_or(file);
    let perms = std::fs::metadata(file).ok().map(|m| m.permissions());
    let dir = file.parent().unwrap_or_else(|| Path::new("."));
    let name = file.file_name().map_or_else(
        || "archivo".to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.{}.{seq}.symphony-tmp", std::process::id()));
    let result = (|| -> Result<(), String> {
        let mut tmp_file = std::fs::File::create(&tmp)
            .map_err(|e| format!("no pude crear {}: {e}", tmp.display()))?;
        std::io::Write::write_all(&mut tmp_file, content)
            .map_err(|e| format!("no pude escribir {}: {e}", tmp.display()))?;
        tmp_file
            .sync_all()
            .map_err(|e| format!("no pude sincronizar {}: {e}", tmp.display()))?;
        drop(tmp_file);
        if let Some(p) = &perms {
            let _ = std::fs::set_permissions(&tmp, p.clone());
        }
        std::fs::rename(&tmp, file)
            .map_err(|e| format!("no pude reemplazar {}: {e}", file.display()))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
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
    use std::io::BufRead as _;
    let file = resolve(root, path);
    let opened =
        std::fs::File::open(&file).map_err(|e| format!("no pude leer {}: {e}", file.display()))?;
    let first = offset.unwrap_or(1).max(1);
    let limit = limit.unwrap_or(READ_LINES).max(1);
    // Línea a línea y solo hasta completar la página: un archivo de gigas no se recorre entero
    // para devolver un rango corto (revisión de seguridad P13.S1 y P13.S5).
    let mut reader = std::io::BufReader::new(opened);
    let (mut total, mut shown) = (0usize, Vec::new());
    let mut buf = String::new();
    loop {
        buf.clear();
        let n = reader.read_line(&mut buf).map_err(|e| match e.kind() {
            std::io::ErrorKind::InvalidData => {
                format!("{} no es texto UTF-8 (¿binario?)", file.display())
            }
            _ => format!("no pude leer {}: {e}", file.display()),
        })?;
        if n == 0 {
            break;
        }
        total += 1;
        if total >= first {
            shown.push(format!(
                "{total:>6}\t{}",
                cut_line(buf.trim_end_matches(['\r', '\n']))
            ));
            if shown.len() >= limit {
                break;
            }
        }
    }
    if shown.is_empty() {
        return Ok(format!(
            "(sin líneas desde la {first}: el archivo tiene {total})"
        ));
    }
    let last = first - 1 + shown.len();
    let mut out = shown.join("\n");
    // Para saber si quedan líneas basta con mirar el búfer: no se lee ni se valida el resto.
    if reader.fill_buf().is_ok_and(|pending| !pending.is_empty()) {
        out.push_str(&format!(
            "\n[mostradas {first}-{last}; hay más líneas, pide el resto con `offset`]"
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
    write_atomic(&file, content.as_bytes())
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
            write_atomic(&file, changed.as_bytes())
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
    // Se corta al llegar al límite: no se acumulan ni se ordenan millones de rutas (P13.S5).
    let mut found: Vec<String> = Vec::new();
    let mut partial = false;
    for entry in walker(&base, Some(pattern))?.flatten() {
        if entry.file_type().is_some_and(|t| t.is_file()) {
            if found.len() >= GLOB_RESULTS {
                partial = true;
                break;
            }
            found.push(shown(&base, entry.path()));
        }
    }
    if found.is_empty() {
        return Ok("Sin resultados.".into());
    }
    found.sort();
    let mut out = found.join("\n");
    if partial {
        out.push_str(&format!(
            "\n[búsqueda parcial: hay más de {GLOB_RESULTS} archivos; afina el patrón]"
        ));
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
    let max = max.unwrap_or(GREP_RESULTS).clamp(1, GREP_MAX);
    let mut hits: Vec<String> = Vec::new();
    let mut partial = false;
    // En flujo: no se guardan todas las rutas ni se recorren todos los archivos si ya hay
    // suficientes coincidencias (P13.S5).
    let files: Box<dyn Iterator<Item = PathBuf>> = if base.is_file() {
        Box::new(std::iter::once(base.clone()))
    } else {
        Box::new(
            walker(&base, glob)?
                .flatten()
                .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
                .map(ignore::DirEntry::into_path),
        )
    };
    let rel_base = if base.is_file() {
        base.parent().unwrap_or(&base).to_path_buf()
    } else {
        base
    };
    'archivos: for file in files {
        if std::fs::metadata(&file).is_ok_and(|m| m.len() > GREP_FILE_MAX) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue; // binario o no UTF-8
        };
        for (i, line) in text.lines().enumerate() {
            if re.is_match(line) {
                if hits.len() >= max {
                    partial = true;
                    break 'archivos;
                }
                hits.push(format!(
                    "{}:{}:{}",
                    shown(&rel_base, &file),
                    i + 1,
                    cut_line(line.trim_end())
                ));
            }
        }
    }
    if hits.is_empty() {
        return Ok("Sin resultados.".into());
    }
    let mut out = hits.join("\n");
    if partial {
        out.push_str(&format!(
            "\n[hay más coincidencias; muestro {} (búsqueda parcial)]",
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

/// El `bash.exe` de Git para Windows, si está (el `bash.exe` del PATH suele ser el lanzador de
/// WSL, que no es lo que se quiere).
#[cfg(windows)]
fn git_bash() -> Option<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)", "LocalAppData"]
        .iter()
        .filter_map(std::env::var_os)
        .flat_map(|d| {
            let d = PathBuf::from(d);
            [
                d.join("Git").join("bin").join("bash.exe"),
                d.join("Programs").join("Git").join("bin").join("bash.exe"),
            ]
        })
        .find(|p| p.is_file())
}

/// ¿El shell que se va a usar entiende la sintaxis de `sh`? Los tests lo consultan para no
/// lanzar comandos de bash cuando toca PowerShell.
#[cfg(test)]
fn posix_shell() -> bool {
    #[cfg(windows)]
    {
        git_bash().is_some()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// El shell de la máquina: Git Bash en Windows si está, si no PowerShell; `sh` en el resto.
fn shell_command(command: &str) -> tokio::process::Command {
    #[cfg(windows)]
    {
        if let Some(bash) = git_bash() {
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
    let timeout = timeout
        .filter(|t| !t.is_zero())
        .unwrap_or(SHELL_TIMEOUT)
        .min(SHELL_TIMEOUT_MAX);
    let mut cmd = shell_command(command);
    cmd.current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    // Su propio grupo de procesos: al vencer el tiempo se mata el grupo entero, no solo el shell.
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("no pude lanzar el shell en {}: {e}", dir.display()))?;
    let pid = child.id();
    // Se lee la salida aparte: así, al vencer el tiempo, el árbol se mata antes de soltar el hijo.
    let (out_buf, out_task) = drain(child.stdout.take());
    let (err_buf, err_task) = drain(child.stderr.take());
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
            // Un proceso que dejó algo corriendo en segundo plano puede mantener abierta la
            // salida: se espera un poco y, si no cierra, se devuelve lo ya leído.
            let (stdout, stderr) = tokio::join!(grab(out_buf, out_task), grab(err_buf, err_task));
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

/// Lo retenido de un flujo: cabeza, cola y cuántos bytes se omitieron.
#[derive(Default)]
struct Captured {
    head: Vec<u8>,
    tail: std::collections::VecDeque<u8>,
    skipped: usize,
}

impl Captured {
    fn push(&mut self, data: &[u8]) {
        // Cabeza + marca + cola caben en SHELL_OUT_MAX: `clip` no vuelve a cortar.
        const HALF: usize = SHELL_OUT_MAX / 2 - 64;
        let mut data = data;
        let take = HALF.saturating_sub(self.head.len()).min(data.len());
        self.head.extend_from_slice(&data[..take]);
        data = &data[take..];
        self.tail.extend(data);
        let excess = self.tail.len().saturating_sub(HALF);
        self.tail.drain(..excess);
        self.skipped += excess;
    }

    /// Lo leído hasta ahora, con el marcador de los bytes omitidos.
    fn finish(&mut self) -> Vec<u8> {
        let mut out = std::mem::take(&mut self.head);
        if self.skipped > 0 {
            out.extend_from_slice(format!("\n[… {} bytes omitidos …]\n", self.skipped).as_bytes());
        }
        out.extend(self.tail.drain(..));
        out
    }
}

type Shared = std::sync::Arc<std::sync::Mutex<Captured>>;

/// El búfer aunque otra tarea hubiera muerto con el cerrojo tomado.
fn retained(buf: &Shared) -> std::sync::MutexGuard<'_, Captured> {
    match buf.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Lee un flujo hasta que se cierra, en su propia tarea, publicando lo leído en un búfer
/// compartido: si un descendiente mantiene el pipe abierto, lo ya leído no se pierde (P13.S5).
fn drain(
    s: Option<impl tokio::io::AsyncRead + Unpin + Send + 'static>,
) -> (Shared, tokio::task::JoinHandle<()>) {
    let buf: Shared = std::sync::Arc::default();
    let writer = std::sync::Arc::clone(&buf);
    let task = tokio::spawn(async move {
        let mut chunk = [0u8; 8192];
        if let Some(mut s) = s {
            while let Ok(n @ 1..) = tokio::io::AsyncReadExt::read(&mut s, &mut chunk).await {
                retained(&writer).push(&chunk[..n]);
            }
        }
    });
    (buf, task)
}

/// Espera a que el flujo cierre; si un descendiente lo mantiene abierto, corta la lectura y
/// devuelve lo que ya estaba retenido, en vez de descartarlo.
async fn grab(buf: Shared, mut task: tokio::task::JoinHandle<()>) -> Vec<u8> {
    let _ = tokio::time::timeout(Duration::from_secs(2), &mut task).await;
    task.abort();
    retained(&buf).finish()
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
        let _ = std::process::Command::new("kill")
            .args(["-KILL", "--", &format!("-{pid}")])
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
        assert!(page.contains("mostradas 2-2; hay más líneas"), "{page}");
        assert!(page.contains("offset"), "{page}");
        assert!(
            read(&root, "no-existe", None, None)
                .unwrap_err()
                .contains("no pude leer")
        );
    }

    #[test]
    fn replacing_a_file_does_not_truncate_the_previous_version() {
        use std::io::Read as _;
        let (_d, root) = tree();
        let path = root.join("src/b.txt");
        let original = std::fs::read_to_string(&path).unwrap();
        let mut opened = std::fs::File::open(&path).unwrap();
        write(&root, "src/b.txt", "nuevo").unwrap();
        let mut previous = String::new();
        opened.read_to_string(&mut previous).unwrap();
        assert_eq!(previous, original);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "nuevo");

        let mut opened = std::fs::File::open(&path).unwrap();
        edit(&root, "src/b.txt", "nuevo", "editado", false).unwrap();
        previous.clear();
        opened.read_to_string(&mut previous).unwrap();
        assert_eq!(previous, "nuevo");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "editado");
    }

    /// Revisión de P13: escribir de forma atómica no le quita el bit ejecutable a un script.
    #[cfg(unix)]
    #[test]
    fn editing_a_script_keeps_it_executable() {
        use std::os::unix::fs::PermissionsExt as _;
        let (_d, root) = tree();
        let path = root.join("src/b.txt");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        write(&root, "src/b.txt", "#!/bin/sh\n").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
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

    #[tokio::test]
    async fn a_huge_output_keeps_only_head_and_tail_in_memory() {
        let big: Vec<u8> = (0..5_000_000u32)
            .map(|i| if i % 80 == 79 { b'\n' } else { b'x' })
            .chain(*b"FIN")
            .collect();
        let (buf, task) = drain(Some(std::io::Cursor::new(big)));
        task.await.unwrap();
        let text = String::from_utf8(retained(&buf).finish()).unwrap();
        assert!(text.len() <= SHELL_OUT_MAX, "{}", text.len());
        assert!(text.contains("bytes omitidos") && text.ends_with("FIN"));
        assert_eq!(clip(&text), text, "no se corta dos veces");
    }

    #[test]
    fn read_does_not_parse_beyond_the_requested_page() {
        let (_d, root) = tree();
        std::fs::write(root.join("pagina.txt"), b"primera\n\xff").unwrap();
        let page = read(&root, "pagina.txt", None, Some(1)).unwrap();
        assert!(
            page.contains("primera") && page.contains("offset"),
            "{page}"
        );
    }

    #[test]
    fn grep_reports_a_partial_search_at_the_requested_limit() {
        let (_d, root) = tree();
        std::fs::write(root.join("muchas.txt"), "hallazgo\n".repeat(300)).unwrap();
        let hits = grep(&root, "hallazgo", Some("muchas.txt"), None, false, Some(2)).unwrap();
        assert!(hits.contains("búsqueda parcial"), "{hits}");
        assert_eq!(
            hits.lines()
                .filter(|line| line.starts_with("muchas.txt:"))
                .count(),
            2
        );
    }

    #[test]
    fn grep_caps_the_requested_number_of_results() {
        let (_d, root) = tree();
        std::fs::write(root.join("muchas.txt"), "hallazgo\n".repeat(1200)).unwrap();
        let hits = grep(
            &root,
            "hallazgo",
            Some("muchas.txt"),
            None,
            false,
            Some(usize::MAX),
        )
        .unwrap();
        assert!(
            hits.lines()
                .filter(|line| line.starts_with("muchas.txt:"))
                .count()
                <= 1000
        );
    }

    #[test]
    fn glob_reports_a_partial_search_at_its_limit() {
        let (_d, root) = tree();
        for i in 0..501 {
            std::fs::write(root.join(format!("{i}.log")), "").unwrap();
        }
        let found = glob(&root, "*.log", None).unwrap();
        assert!(found.contains("búsqueda parcial"), "{found}");
        assert_eq!(
            found.lines().filter(|line| line.ends_with(".log")).count(),
            500
        );
    }

    #[tokio::test]
    async fn shell_keeps_output_when_a_descendant_holds_the_pipe_open() {
        // El `&` es sintaxis de sh: con el PowerShell de reserva no se monta el escenario.
        if !posix_shell() {
            return;
        }
        let (_d, root) = tree();
        let started = std::time::Instant::now();
        let output = shell(&root, "sleep 6 & echo resultado-retenido", None, None)
            .await
            .unwrap();
        assert!(output.contains("resultado-retenido"), "{output}");
        // No espera al nieto: corta la lectura a los dos segundos y devuelve lo leído.
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn read_streams_lines_and_still_rejects_binaries() {
        let (_d, root) = tree();
        std::fs::write(root.join("bin.dat"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
        assert!(
            read(&root, "bin.dat", None, None)
                .unwrap_err()
                .contains("UTF-8")
        );
        std::fs::write(root.join("crlf.txt"), "uno\r\ndos\r\n").unwrap();
        assert_eq!(
            read(&root, "crlf.txt", None, None).unwrap(),
            "     1\tuno\n     2\tdos"
        );
    }
}
