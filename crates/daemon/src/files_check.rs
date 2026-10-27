//! Check `files` (ADR-0021): en proyectos `document` y `media`, cada archivo cambiado existe y
//! su formato se abre. Por extensión, sin dependencias nuevas: texto UTF-8, JSON y TOML con
//! lo que ya está en el workspace, binarios por su firma. Lo que no se reconoce no se juzga.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Texto más grande que esto no se lee entero (se juzga solo la cabeza).
const TEXT_MAX: u64 = 32 * 1024 * 1024;
/// Dónde se busca el `%%EOF` de un PDF.
const PDF_TAIL: u64 = 2048;

const TEXT: &[&str] = &[
    "md", "markdown", "txt", "csv", "tsv", "html", "htm", "xml", "yaml", "yml", "rst", "tex",
    "srt", "vtt", "css", "adoc",
];
/// Cabeza, cola (solo PDF) y tamaño de un archivo leído para juzgarlo.
type Sample = (Vec<u8>, Vec<u8>, u64);

const ZIP: &[&str] = &["zip", "docx", "xlsx", "pptx", "odt", "ods", "odp", "epub"];

fn ext_of(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Si la extensión es de las que se juzgan.
pub fn judged(path: &str) -> bool {
    let e = ext_of(path);
    TEXT.contains(&e.as_str())
        || ZIP.contains(&e.as_str())
        || matches!(
            e.as_str(),
            "svg"
                | "json"
                | "toml"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "pdf"
                | "mp3"
                | "wav"
                | "mp4"
                | "mov"
                | "m4a"
                | "m4v"
                | "webm"
                | "mkv"
                | "ogg"
                | "oga"
                | "ogv"
                | "opus"
                | "flac"
        )
}

/// Juzga un archivo por su contenido. `head`: los primeros bytes (o todo, si es texto);
/// `tail`: los últimos (solo PDF); `len`: el tamaño real. `Err` con el motivo.
pub fn judge(path: &str, head: &[u8], tail: &[u8], len: u64) -> Result<(), String> {
    let e = ext_of(path);
    let e = e.as_str();
    let text = TEXT.contains(&e) || matches!(e, "svg" | "json" | "toml");
    if len == 0 {
        return if text {
            Ok(())
        } else {
            Err("está vacío".into())
        };
    }
    let starts = |sig: &[u8]| head.starts_with(sig);
    let at = |i: usize, sig: &[u8]| head.get(i..i + sig.len()) == Some(sig);
    let ok = match e {
        _ if TEXT.contains(&e) => {
            return std::str::from_utf8(head)
                .map(|_| ())
                .map_err(|_| "no es texto UTF-8 válido".into());
        }
        "svg" => {
            let s = std::str::from_utf8(head).map_err(|_| "no es texto UTF-8 válido")?;
            return if s.contains("<svg") {
                Ok(())
            } else {
                Err("no contiene un elemento <svg>".into())
            };
        }
        "json" => {
            return serde_json::from_slice::<serde_json::Value>(head)
                .map(|_| ())
                .map_err(|e| format!("JSON inválido: {e}"));
        }
        "toml" => {
            let s = std::str::from_utf8(head).map_err(|_| "no es texto UTF-8 válido")?;
            return s
                .parse::<toml_edit::DocumentMut>()
                .map(|_| ())
                .map_err(|e| format!("TOML inválido: {}", e.message()));
        }
        "png" => starts(b"\x89PNG\r\n\x1a\n"),
        "jpg" | "jpeg" => starts(b"\xff\xd8\xff"),
        "gif" => starts(b"GIF87a") || starts(b"GIF89a"),
        "webp" => starts(b"RIFF") && at(8, b"WEBP"),
        "pdf" => {
            if !starts(b"%PDF-") {
                return Err("no empieza con %PDF-".into());
            }
            return if tail.windows(5).any(|w| w == b"%%EOF") {
                Ok(())
            } else {
                Err("está truncado (sin %%EOF al final)".into())
            };
        }
        _ if ZIP.contains(&e) => starts(b"PK\x03\x04"),
        "mp3" => starts(b"ID3") || (head.len() > 1 && head[0] == 0xff && head[1] & 0xe0 == 0xe0),
        "wav" => starts(b"RIFF") && at(8, b"WAVE"),
        "mp4" | "mov" | "m4a" | "m4v" => at(4, b"ftyp"),
        "webm" | "mkv" => starts(b"\x1a\x45\xdf\xa3"),
        "ogg" | "oga" | "ogv" | "opus" => starts(b"OggS"),
        "flac" => starts(b"fLaC"),
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        Err(format!("no tiene la firma de un .{e}"))
    }
}

/// Lee lo necesario de `full` y lo juzga. `None` si la extensión no se juzga.
pub fn judge_file(rel: &str, full: &Path) -> Option<Result<(), String>> {
    if !judged(rel) {
        return None;
    }
    let e = ext_of(rel);
    let text = TEXT.contains(&e.as_str()) || matches!(e.as_str(), "svg" | "json" | "toml");
    let read = || -> std::io::Result<Option<Sample>> {
        let mut f = std::fs::File::open(full)?;
        let len = f.metadata()?.len();
        // ponytail: un texto de más de 32 MB no se juzga; leerlo a trozos si hace falta.
        if text && len > TEXT_MAX {
            return Ok(None);
        }
        let want = if text { len } else { len.min(64) };
        let mut head = Vec::new();
        f.by_ref().take(want).read_to_end(&mut head)?;
        let mut tail = Vec::new();
        if e == "pdf" {
            f.seek(SeekFrom::Start(len.saturating_sub(PDF_TAIL)))?;
            f.read_to_end(&mut tail)?;
        }
        Ok(Some((head, tail, len)))
    };
    Some(match read() {
        Ok(Some((head, tail, len))) => judge(rel, &head, &tail, len),
        Ok(None) => Ok(()),
        Err(e) => Err(format!("no se puede abrir: {e}")),
    })
}

/// Archivos que el agente cambió respecto de `base` (commiteados o no, incluidos los nuevos
/// sin seguimiento: el gate corre antes del commit).
pub fn changed_files(worktree: &Path, base: &str) -> Vec<String> {
    let r = symphony_git::Repo::at(worktree);
    let mut files: Vec<String> = r
        .diff_numstat(base)
        .map(|v| v.into_iter().map(|n| n.path).collect())
        .unwrap_or_default();
    for entry in r.status().unwrap_or_default() {
        if let symphony_git::FileStatus::Untracked { path } = entry {
            files.push(path);
        }
    }
    files.sort();
    files.dedup();
    files
}

/// Juzga los archivos cambiados (`rels`, relativos a `root`). Devuelve los fallos
/// (`archivo: motivo`) y cuántos se juzgaron. Un archivo que ya no existe (borrado) no cuenta.
pub fn check(root: &Path, rels: &[String]) -> (Vec<String>, u32) {
    let mut failures = Vec::new();
    let mut judged_count = 0u32;
    for rel in rels {
        let full = root.join(rel);
        if !full.is_file() {
            continue;
        }
        if let Some(r) = judge_file(rel, &full) {
            judged_count += 1;
            if let Err(why) = r {
                failures.push(format!("{rel}: {why}"));
            }
        }
    }
    (failures, judged_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_and_text_are_judged_by_extension() {
        let ok = |p: &str, b: &[u8]| judge(p, b, b, b.len() as u64);
        assert!(ok("a.png", b"\x89PNG\r\n\x1a\n....").is_ok());
        assert!(ok("a.png", b"GIF89a").is_err());
        assert!(ok("a.jpg", b"\xff\xd8\xff\xe0").is_ok());
        assert!(ok("a.gif", b"GIF89a....").is_ok());
        assert!(ok("a.webp", b"RIFF\0\0\0\0WEBPVP8 ").is_ok());
        assert!(ok("a.wav", b"RIFF\0\0\0\0WEBPVP8 ").is_err());
        assert!(ok("a.docx", b"PK\x03\x04rest").is_ok());
        assert!(ok("a.mp4", b"\0\0\0\x18ftypmp42").is_ok());
        assert!(ok("a.mp3", b"ID3\x04").is_ok());
        assert!(ok("a.mp3", b"\xff\xfb\x90").is_ok());
        assert!(ok("a.ogg", b"OggS\0").is_ok());
        assert!(ok("a.flac", b"fLaC").is_ok());
        assert!(ok("A.PNG", b"\x89PNG\r\n\x1a\n").is_ok(), "mayúsculas");
        assert!(ok("a.md", "# título ñ\n".as_bytes()).is_ok());
        assert!(ok("a.md", b"\xff\xfe\xfd").is_err());
        assert!(ok("a.svg", b"<?xml?><svg></svg>").is_ok());
        assert!(ok("a.svg", b"<html></html>").is_err());
        assert!(ok("a.json", br#"{"a": [1, 2]}"#).is_ok());
        assert!(ok("a.json", br#"{"a": "#).is_err());
        assert!(ok("a.toml", b"a = 1\n[b]\nc = 'x'\n").is_ok());
        assert!(ok("a.toml", b"a = = 1").is_err());
        assert!(ok("a.desconocida", b"\0\0").is_ok(), "no se juzga");
    }

    #[test]
    fn empty_files_and_truncated_pdfs() {
        assert!(judge("a.png", b"", b"", 0).is_err());
        assert!(
            judge("a.md", b"", b"", 0).is_ok(),
            "un texto vacío es válido"
        );
        let pdf = b"%PDF-1.7\n...\n%%EOF\n";
        assert!(judge("a.pdf", pdf, pdf, pdf.len() as u64).is_ok());
        let cut = b"%PDF-1.7\n...";
        assert!(
            judge("a.pdf", cut, cut, cut.len() as u64)
                .unwrap_err()
                .contains("truncado")
        );
    }

    #[test]
    fn check_reads_files_skips_deleted_and_unknown() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("bien.png"), b"\x89PNG\r\n\x1a\nresto").unwrap();
        std::fs::write(d.path().join("roto.pdf"), b"%PDF-1.4 sin final").unwrap();
        std::fs::write(d.path().join("datos.bin"), b"\0\x01").unwrap();
        let rels: Vec<String> = ["bien.png", "roto.pdf", "datos.bin", "borrado.md"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (failures, judged) = check(d.path(), &rels);
        assert_eq!(judged, 2, "el .bin no se juzga y el borrado no existe");
        assert_eq!(failures.len(), 1);
        assert!(failures[0].starts_with("roto.pdf"), "{failures:?}");
    }
}
