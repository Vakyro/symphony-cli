//! Redactor central de secretos (STACK §22). Se aplica antes de persistir
//! stdout/stderr, guardar errores, mostrar el audit log y escribir tracing.
//! Mejor redactar de más que filtrar un token.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;

pub const REDACTED: &str = "[REDACTED]";

/// Nombres que delatan una credencial en `NOMBRE=valor` o `"nombre": "valor"`.
/// Después de la palabra clave, el nombre solo sigue con `_`/`-` o termina:
/// `GITHUB_TOKEN` y `auth-token` sí; `tokens_used` y `tokenizer` no.
const SECRET_NAME: &str = r"(?i:[a-z0-9_-]*(?:api[_-]?key|apikey|token|secret|passw(?:or)?d|credentials?|private[_-]?key|access[_-]?key|session[_-]?key|authorization|cookie|auth)(?:[_-][a-z0-9_-]*)?)";

static RULES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    let rules: [(String, &'static str); 7] = [
        // Encabezados HTTP: se borra todo el valor, cualquiera sea el esquema.
        (
            r"(?im)^(\s*(?:proxy-)?authorization\s*:\s*).+$".into(),
            "${1}[REDACTED]",
        ),
        (
            r"(?im)^(\s*(?:set-)?cookie\s*:\s*).+$".into(),
            "${1}[REDACTED]",
        ),
        // `Bearer <token>` en cualquier parte de una línea.
        (
            r"(?i)\b(bearer\s+)[A-Za-z0-9._~+/=-]{8,}".into(),
            "${1}[REDACTED]",
        ),
        // Campos JSON con nombre de credencial.
        (
            format!(r#"("{SECRET_NAME}"\s*:\s*)"(?:[^"\\]|\\.)*""#),
            "${1}\"[REDACTED]\"",
        ),
        // Variables de entorno / argumentos `NOMBRE=valor` o `NOMBRE: valor`.
        (
            format!(r#"\b({SECRET_NAME}\s*[=:]\s*)(?:"[^"]*"|'[^']*'|[^\s"',;]+)"#),
            "${1}[REDACTED]",
        ),
        // Formatos conocidos de llaves, aunque aparezcan sueltas.
        (
            [
                r"sk-ant-[A-Za-z0-9_-]{16,}",
                r"sk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{20,}",
                r"(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{20,}",
                r"github_pat_[A-Za-z0-9_]{20,}",
                r"AIza[0-9A-Za-z_-]{35}",
                r"(?:AKIA|ASIA)[0-9A-Z]{16}",
                r"xox[abprs]-[A-Za-z0-9-]{10,}",
            ]
            .join("|"),
            REDACTED,
        ),
        // JWT (también cubre tokens OAuth en ese formato).
        (
            r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}".into(),
            REDACTED,
        ),
    ];
    rules
        .into_iter()
        // Las reglas son constantes: si una no compila, lo detectan los tests de este módulo.
        .filter_map(|(re, rep)| Regex::new(&re).ok().map(|r| (r, rep)))
        .collect()
});

/// Devuelve el texto con los secretos reemplazados por `[REDACTED]`.
/// Si no hay nada que redactar, no copia.
pub fn redact(input: &str) -> Cow<'_, str> {
    let mut out = Cow::Borrowed(input);
    for (re, rep) in RULES.iter() {
        if let Cow::Owned(s) = re.replace_all(&out, *rep) {
            out = Cow::Owned(s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_rules_compile() {
        assert_eq!(RULES.len(), 7);
    }

    fn assert_clean(input: &str, secret: &str) {
        let out = redact(input);
        assert!(!out.contains(secret), "no redactó `{secret}` en: {out}");
        assert!(out.contains(REDACTED), "falta el marcador en: {out}");
    }

    #[test]
    fn authorization_headers() {
        assert_clean(
            "Authorization: Bearer abcdefghijklmnop123",
            "abcdefghijklmnop123",
        );
        assert_clean(
            "authorization: Basic dXNlcjpwYXNzd29yZA==",
            "dXNlcjpwYXNzd29yZA==",
        );
        assert_clean("  Proxy-Authorization: Token s3cr3tvalue", "s3cr3tvalue");
        assert_clean(
            "curl -H 'x' -> sent Bearer eyJhbGciOiJIUzI1.x.y and more",
            "eyJhbGciOiJIUzI1",
        );
    }

    #[test]
    fn known_key_formats() {
        for key in [
            "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUv",
            "sk-proj-AbCdEfGhIjKlMnOpQrStUvWxYz",
            "ghp_AbCdEfGhIjKlMnOpQrStUvWxYz0123",
            "gho_AbCdEfGhIjKlMnOpQrStUvWxYz0123",
            "github_pat_11ABCDEFG0123456789_abcdefghijk",
            "AIzaSyA1234567890abcdefghijklmnopqrstuv",
            "AKIAIOSFODNN7EXAMPLE",
            "xoxb-1234567890-abcdefghij",
        ] {
            assert_clean(&format!("error calling api with key {key} (401)"), key);
        }
    }

    #[test]
    fn jwt() {
        let jwt = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.c2lnbmF0dXJlX2hlcmU";
        assert_clean(&format!("token={jwt}"), "c2lnbmF0dXJlX2hlcmU");
        assert_clean(&format!("got {jwt} from refresh"), "c2lnbmF0dXJlX2hlcmU");
    }

    #[test]
    fn cookies() {
        assert_clean("Cookie: session=abc123def456; theme=dark", "abc123def456");
        assert_clean(
            "Set-Cookie: __Secure-token=zzz999yyy; HttpOnly",
            "zzz999yyy",
        );
    }

    #[test]
    fn credential_env_vars_and_args() {
        assert_clean("ANTHROPIC_API_KEY=abc123xyz789", "abc123xyz789");
        assert_clean("export OPENAI_API_KEY=\"q1w2e3r4t5\"", "q1w2e3r4t5");
        assert_clean("GITHUB_TOKEN: plainvalue42", "plainvalue42");
        assert_clean("set CODEX_API_KEY='single-quoted-1'", "single-quoted-1");
        assert_clean(
            "AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG",
            "wJalrXUtnFEMI/K7MDENG",
        );
        assert_clean("DB_PASSWORD=hunter2hunter2", "hunter2hunter2");
        assert_clean("--auth-token=cli-flag-secret", "cli-flag-secret");
    }

    #[test]
    fn json_fields() {
        assert_clean(
            r#"{"access_token":"a.b.c-opaque","expires_in":3600}"#,
            "a.b.c-opaque",
        );
        assert_clean(r#"{"apiKey": "k-123456", "model": "sonnet"}"#, "k-123456");
        assert_clean(r#"{"refresh_token":"with \"escaped\" quote"}"#, "escaped");
        let out = redact(r#"{"access_token":"x-1","expires_in":3600}"#);
        assert!(out.contains(r#""expires_in":3600"#), "borró de más: {out}");
    }

    #[test]
    fn leaves_normal_text_alone() {
        for s in [
            "cargo nextest run -p symphony-core (24 passed)",
            "Agent #3 WAITING_RESOURCE: esperando el test suite de Agent #2",
            "token count: 1234 input tokens",
            "tokens_used=1234",
            "the author wrote a skeleton",
            r#"{"type":"turn.completed","usage":{"input_tokens":24763}}"#,
        ] {
            assert_eq!(redact(s), s, "cambió texto normal");
        }
        assert!(matches!(redact("sin secretos"), Cow::Borrowed(_)));
    }
}
