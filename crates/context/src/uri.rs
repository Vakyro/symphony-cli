//! Direcciones `ctx://` (IDEA §5.6, DB §3.G, STACK §47): cómo se nombra algo recuperable.
//!
//! ```text
//! ctx://file/src/auth.ts            el archivo en el worktree del agente
//! ctx://file/src/auth.ts@a1b2c3d    el archivo en ese commit o rama
//! ctx://diff/<agente>/<hash>        un diff guardado en un checkpoint
//! ctx://message/<id>                un mensaje largo de la conversación
//! ctx://run/<id>                    lo que dejó un run
//! ctx://tool/<id>                   la salida de una herramienta
//! ```
//!
//! El parser es **estricto**: una dirección nunca puede salir de su raíz. Solo se decodifican
//! cuatro escapes, en mayúsculas, y solo en la ruta de un archivo: `%25` (`%`), `%40` (`@`),
//! `%3F` (`?`) y `%23` (`#`), para que `packages/@scope/x.ts` tenga dirección. Cualquier otro
//! `%` se rechaza (nunca `%2e` ni `%2f`), igual que `.` y `..`, rutas absolutas, `\`, letras de
//! unidad y caracteres de control. Lo que parsea, al formatearse vuelve a dar la misma dirección.

use std::fmt;
use std::path::{Component, Path, PathBuf};

/// Largo máximo de una dirección (en bytes).
pub const MAX_URI_LEN: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CtxUri {
    /// Archivo del proyecto. `rev`: commit, rama o etiqueta; sin él, el worktree vivo.
    File {
        path: String,
        rev: Option<String>,
    },
    Diff {
        agent: String,
        hash: String,
    },
    Message {
        id: String,
    },
    Run {
        id: String,
    },
    Tool {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UriError {
    #[error("la dirección está vacía o es demasiado larga")]
    Length,
    #[error("no empieza con `ctx://`")]
    Scheme,
    #[error("tipo de dirección desconocido: `{0}`")]
    Kind(String),
    #[error(
        "la dirección no puede contener `\\`, caracteres de control, `?`/`#` ni un `%` que no sea `%25`, `%40`, `%3F` o `%23`"
    )]
    ForbiddenChar,
    #[error("segmento de ruta inválido (`..`, `.`, vacío o con `:`): `{0}`")]
    Segment(String),
    #[error("identificador inválido: `{0}`")]
    Id(String),
    #[error("revisión inválida: `{0}`")]
    Rev(String),
}

fn ident_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Un segmento de ruta de archivo: ni `.` ni `..`, sin `:` (letra de unidad), sin espacios finales.
fn segment_ok(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && !s.contains(':')
        && !s.ends_with(' ')
        && !s.ends_with('.')
}

/// Los únicos escapes que acepta la ruta de un archivo (en mayúsculas, para que cada ruta tenga
/// una sola dirección).
const ESCAPES: [(char, &str); 4] = [('%', "%25"), ('@', "%40"), ('?', "%3F"), ('#', "%23")];

/// `a%40b` → `a@b`. `None` si hay un `%` que no es uno de [`ESCAPES`].
fn decode_path(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let (c, _) = ESCAPES.iter().find(|(_, e)| tail.starts_with(e))?;
        out.push(*c);
        rest = &tail[3..];
    }
    out.push_str(rest);
    Some(out)
}

/// Lo inverso de [`decode_path`]: la ruta tal como va en la dirección.
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        match ESCAPES.iter().find(|(e, _)| *e == c) {
            Some((_, esc)) => out.push_str(esc),
            None => out.push(c),
        }
    }
    out
}

fn rev_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && !s.starts_with('-')
        && !s.contains("..")
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
}

impl CtxUri {
    /// Parsea y valida. Solo decodifica los escapes de [`ESCAPES`], y solo en la ruta de un archivo.
    pub fn parse(input: &str) -> Result<Self, UriError> {
        if input.is_empty() || input.len() > MAX_URI_LEN {
            return Err(UriError::Length);
        }
        let rest = input.strip_prefix("ctx://").ok_or(UriError::Scheme)?;
        let (kind, tail) = rest.split_once('/').unwrap_or((rest, ""));
        // En un archivo, `%` solo puede ser uno de ESCAPES (se comprueba al decodificar la ruta).
        let reserved: &[char] = if kind == "file" {
            &['\\', '?', '#']
        } else {
            &['%', '\\', '?', '#']
        };
        if rest
            .chars()
            .any(|c| c.is_control() || reserved.contains(&c))
        {
            return Err(UriError::ForbiddenChar);
        }
        match kind {
            "file" => {
                let (path, rev) = match tail.split_once('@') {
                    Some((p, r)) => (p, Some(r)),
                    None => (tail, None),
                };
                if path.is_empty() {
                    return Err(UriError::Segment(String::new()));
                }
                let path = decode_path(path).ok_or(UriError::ForbiddenChar)?;
                if path.chars().any(char::is_control) {
                    return Err(UriError::ForbiddenChar);
                }
                for seg in path.split('/') {
                    if !segment_ok(seg) {
                        return Err(UriError::Segment(seg.to_string()));
                    }
                }
                let rev = match rev {
                    Some(r) if rev_ok(r) => Some(r.to_string()),
                    Some(r) => return Err(UriError::Rev(r.to_string())),
                    None => None,
                };
                Ok(Self::File { path, rev })
            }
            "diff" => {
                let (agent, hash) = tail
                    .split_once('/')
                    .ok_or_else(|| UriError::Id(tail.to_string()))?;
                for id in [agent, hash] {
                    if !ident_ok(id) {
                        return Err(UriError::Id(id.to_string()));
                    }
                }
                Ok(Self::Diff {
                    agent: agent.into(),
                    hash: hash.into(),
                })
            }
            "message" | "run" | "tool" => {
                if !ident_ok(tail) {
                    return Err(UriError::Id(tail.to_string()));
                }
                let id = tail.to_string();
                Ok(match kind {
                    "message" => Self::Message { id },
                    "run" => Self::Run { id },
                    _ => Self::Tool { id },
                })
            }
            other => Err(UriError::Kind(other.to_string())),
        }
    }

    /// Resuelve la ruta de un `File` bajo `root` **sin salir de ella**. `None` si la dirección
    /// no es un archivo. El llamador, antes de leer, debe comprobar además los enlaces
    /// simbólicos (`canonicalize` y volver a comprobar que sigue bajo `root`).
    pub fn file_under(&self, root: &Path) -> Option<PathBuf> {
        let Self::File { path, .. } = self else {
            return None;
        };
        let rel = Path::new(path);
        // El parser ya lo garantiza; se vuelve a comprobar porque es la barrera de seguridad.
        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            return None;
        }
        Some(root.join(rel))
    }
}

impl fmt::Display for CtxUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File { path, rev: None } => write!(f, "ctx://file/{}", encode_path(path)),
            Self::File {
                path,
                rev: Some(rev),
            } => write!(f, "ctx://file/{}@{rev}", encode_path(path)),
            Self::Diff { agent, hash } => write!(f, "ctx://diff/{agent}/{hash}"),
            Self::Message { id } => write!(f, "ctx://message/{id}"),
            Self::Run { id } => write!(f, "ctx://run/{id}"),
            Self::Tool { id } => write!(f, "ctx://tool/{id}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn well_formed_addresses_roundtrip() {
        for s in [
            "ctx://file/src/auth.ts",
            "ctx://file/src/auth.ts@a1b2c3d",
            "ctx://file/README.md@main",
            "ctx://file/dir/sub dir/a b.txt",
            "ctx://file/packages/%40scope/x.ts",
            "ctx://file/src/%40types.ts@main",
            "ctx://file/a%3Fb%23c%25d.txt",
            "ctx://diff/01M3AGENT/9f8e7d6c",
            "ctx://message/01M3MSG",
            "ctx://run/921",
            "ctx://tool/01M3TOOL",
        ] {
            let uri = CtxUri::parse(s).unwrap_or_else(|e| panic!("{s}: {e}"));
            assert_eq!(uri.to_string(), s);
        }
    }

    #[test]
    fn traversal_and_escape_attempts_are_rejected() {
        for s in [
            "ctx://file/../../etc/passwd",
            "ctx://file/a/../../b",
            "ctx://file/./a",
            "ctx://file//etc/passwd",
            "ctx://file/a//b",
            "ctx://file/",
            "ctx://file",
            "ctx://file/%2e%2e/%2e%2e/etc/passwd",
            "ctx://file/a%2fb",
            "ctx://file/a%2Fb",
            "ctx://file/a%5Cb",
            "ctx://file/a%40b%",
            "ctx://file/a%4",
            "ctx://file/a%3fb",
            "ctx://file/a%00b",
            "ctx://file/a%0Ab",
            "ctx://file/%252e%252e/%2e",
            "ctx://diff/a%40b/c",
            "ctx://message/a%40b",
            "ctx://file/..%2f..%2fx",
            "ctx://file/a\\..\\b",
            "ctx://file/C:/Windows/system.ini",
            "ctx://file/C:\\Windows",
            "ctx://file/a/b/",
            "ctx://file/a/..",
            "ctx://file/...",
            "ctx://file/a@../../b",
            "ctx://file/a@-rf",
            "ctx://file/a@",
            "ctx://file/a\0b",
            "ctx://file/a\nb",
            "ctx://file/a?x=1",
            "ctx://file/a#frag",
            "ctx://diff/../x/y",
            "ctx://diff/a",
            "ctx://diff/a/b/c",
            "ctx://message/../x",
            "ctx://message/",
            "ctx://run/a b",
            "ctx://secreto/x",
            "http://file/a",
            "ctx:/file/a",
            "CTX://file/a",
            "",
        ] {
            assert!(CtxUri::parse(s).is_err(), "debía rechazarse: {s:?}");
        }
        let long = format!("ctx://file/{}", "a/".repeat(600));
        assert_eq!(CtxUri::parse(&long), Err(UriError::Length));
    }

    #[test]
    fn a_file_address_never_resolves_outside_the_root() {
        let root = Path::new("/proyecto/worktree");
        let uri = CtxUri::parse("ctx://file/src/a.rs").unwrap();
        assert_eq!(uri.file_under(root), Some(root.join("src").join("a.rs")));
        assert_eq!(CtxUri::parse("ctx://run/1").unwrap().file_under(root), None);
        // Aunque alguien construya la variante a mano, la barrera final la frena.
        let forged = CtxUri::File {
            path: "../fuera".into(),
            rev: None,
        };
        assert_eq!(forged.file_under(root), None);
        let absolute = CtxUri::File {
            path: "/etc/passwd".into(),
            rev: None,
        };
        assert_eq!(absolute.file_under(root), None);
    }

    #[test]
    fn escaped_reserved_characters_roundtrip_to_the_real_path() {
        let uri = CtxUri::parse("ctx://file/packages/%40scope/x%23y.ts@main").unwrap();
        assert_eq!(
            uri,
            CtxUri::File {
                path: "packages/@scope/x#y.ts".into(),
                rev: Some("main".into()),
            }
        );
        // El `@` escapado no se confunde con el separador de revisión.
        assert_eq!(
            uri.to_string(),
            "ctx://file/packages/%40scope/x%23y.ts@main"
        );
        let root = Path::new("/raiz");
        assert_eq!(
            uri.file_under(root),
            Some(root.join("packages").join("@scope").join("x#y.ts"))
        );
        // Un nombre que parece un escape queda literal: `%252e` es el archivo `%2e`.
        let lit = CtxUri::parse("ctx://file/%252e").unwrap();
        assert_eq!(
            lit,
            CtxUri::File {
                path: "%2e".into(),
                rev: None
            }
        );
    }

    proptest! {
        /// Cualquier ruta sin segmentos que escapen sobrevive a formatear y volver a parsear.
        #[test]
        fn any_safe_path_roundtrips(segs in proptest::collection::vec("[A-Za-z0-9 @?#%_-]{1,8}", 1..5)) {
            let path = segs.join("/");
            let uri = CtxUri::File { path: path.clone(), rev: None };
            let s = uri.to_string();
            match CtxUri::parse(&s) {
                Ok(back) => prop_assert_eq!(back, uri),
                // Solo se rechazan segmentos con espacio final (regla de `segment_ok`).
                Err(UriError::Segment(seg)) => prop_assert!(seg.ends_with(' ') || seg.ends_with('.'), "{s}"),
                Err(e) => prop_assert!(false, "{s}: {e}"),
            }
        }

        /// Con cualquier texto, el parser no entra en pánico; lo que acepta es canónico y
        /// nunca contiene un segmento que escape.
        #[test]
        fn parsing_never_panics_and_accepted_addresses_are_safe(s in "\\PC{0,200}") {
            if let Ok(uri) = CtxUri::parse(&s) {
                prop_assert_eq!(uri.to_string(), s.clone());
                prop_assert!(!s.contains(".."), "{s}");
                prop_assert!(!s.contains('\\') && !s.contains('%'));
                if let Some(p) = uri.file_under(Path::new("/raiz")) {
                    prop_assert!(p.starts_with("/raiz"));
                }
            }
        }

        /// Cualquier mezcla de trozos típicos de un ataque se rechaza o queda dentro de la raíz.
        #[test]
        fn hostile_paths_stay_inside_or_fail(parts in proptest::collection::vec(
            prop_oneof![Just("..".to_string()), Just(".".to_string()), Just("a".to_string()),
                        Just("%2e%2e".to_string()), Just("C:".to_string()), Just("".to_string()),
                        Just("b c".to_string()), Just("\\".to_string())], 1..7)) {
            let s = format!("ctx://file/{}", parts.join("/"));
            if let Ok(uri) = CtxUri::parse(&s) {
                let p = uri.file_under(Path::new("/raiz"));
                prop_assert!(p.is_some_and(|p| p.starts_with("/raiz")));
            }
        }
    }
}
