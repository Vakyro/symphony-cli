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
        // Variables de entorno / argumentos `NOMBRE=valor` o `NOMBRE: valor`. Un valor ya
        // redactado es un valor entero: si no, lo que lo sigue sin separador (`[REDACTED]}x`)
        // se volvería a comer en otra pasada (idempotencia, caso de cargo-fuzz).
        (
            format!(r#"\b({SECRET_NAME}\s*[=:]\s*)(?:\[REDACTED\]|"[^"]*"|'[^']*'|[^\s"',;]+)"#),
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
    let mut out = pass(input);
    // Dos coincidencias vecinas pueden compartir una comilla: la primera se la come y la
    // segunda solo aparece en otra pasada (caso de cargo-fuzz). Se repite hasta que no cambia;
    // cada pasada que cambia cambia texto por el marcador, así que termina (el tope es seguro).
    for _ in 0..16 {
        let Cow::Owned(s) = &out else { break };
        let changed = match pass(s) {
            Cow::Owned(n) => Some(n),
            Cow::Borrowed(_) => None,
        };
        match changed {
            Some(n) => out = Cow::Owned(n),
            None => break,
        }
    }
    out
}

/// Una pasada de todas las reglas, en orden.
fn pass(input: &str) -> Cow<'_, str> {
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
    use proptest::prelude::*;

    proptest! {
        /// Cualquier texto (comandos, JSON, cabeceras rotas) se redacta sin panic
        /// y redactar de nuevo no cambia nada: el marcador no crea ni borra reglas.
        #[test]
        fn redact_never_panics_and_is_idempotent(s in "\\PC{0,512}") {
            let once = redact(&s).into_owned();
            let twice = redact(&once).into_owned();
            prop_assert_eq!(twice, once);
        }

        /// Cualquier byte válido como texto (incluidos los inválidos como UTF-8,
        /// perdidos en la conversión) tampoco puede romper el redactor.
        #[test]
        fn redact_handles_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
            let s = String::from_utf8_lossy(&bytes);
            let _ = redact(&s);
        }
    }

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
    /// Encontrado por cargo-fuzz (P13.S2, `fuzz/artifacts/redact/crash-55b3…`): redactar dos
    /// veces tiene que dar lo mismo que una.
    #[test]
    fn fuzz_case_is_idempotent() {
        let bytes: &[u8] = &[
            65, 117, 116, 104, 110, 99, 105, 97, 108, 45, 102, 105, 99, 116, 105, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 110, 58, 1, 0, 123, 91, 121, 118, 164, 0, 32, 66, 101, 97, 114, 101, 105, 111,
            10, 123, 34, 116, 111, 107, 101, 110, 58, 34, 118, 97, 108, 111, 114, 45, 156, 135, 73,
            99, 105, 105, 99, 105, 111, 34, 125, 107, 101, 110, 34, 58, 34, 118, 97, 108, 111, 114,
            45, 102, 105, 99, 116, 105, 111, 10, 111, 34, 126,
        ];
        let s = String::from_utf8_lossy(bytes);
        let once = redact(&s).into_owned();
        assert_eq!(redact(&once), once, "una vez: {once:?}");
    }
    /// Segundo caso de cargo-fuzz (`crash-973c…`, tras 40 min sobre el primer arreglo).
    #[test]
    fn fuzz_case_2_is_idempotent() {
        let bytes: &[u8] = &[
            6, 99, 105, 97, 10, 65, 80, 73, 95, 75, 69, 89, 65, 117, 116, 104, 111, 114, 105, 122,
            97, 116, 105, 111, 110, 252, 61, 118, 160, 184, 108, 6, 0, 58, 32, 66, 105, 93, 105,
            111, 110, 58, 32, 66, 105, 93, 97, 10, 111, 10, 136, 91, 34, 116, 111, 107, 101, 110,
            34, 58, 34, 118, 97, 108, 111, 114, 118, 97, 108, 111, 114, 41, 45, 102, 105, 99, 116,
            105, 99, 101, 97, 114, 101, 114, 32, 99, 116, 3, 101, 110, 34, 0, 0, 105, 99, 107, 125,
            105, 34, 111, 101, 110, 34, 62, 34, 118, 3, 101, 110, 34, 58, 34, 118, 0, 0, 91, 34,
            116, 111, 107, 101, 110, 34, 58, 34, 116, 105, 111, 110, 58, 32, 66, 101, 97, 114, 101,
            0, 0, 138, 39, 0, 65, 0, 0, 0, 0, 0, 0, 0, 50, 111, 10, 123, 34, 0, 116, 111, 107, 101,
            110, 58, 34, 114, 32, 99, 114, 101, 100, 101, 110, 99, 105, 97, 108, 45, 97, 10, 111,
            10, 136, 91, 34, 116, 111, 107, 101, 110, 34, 58, 34, 118, 97, 108, 111, 114, 118, 97,
            108, 111, 114, 41, 45, 102, 105, 99, 116, 105, 99, 101, 97, 114, 101, 114, 32, 99, 0,
            0, 91, 34, 116, 111, 107, 101, 110, 34, 58, 34, 116, 105, 111, 110, 58, 32, 66, 101,
            97, 114, 101, 114, 32, 99, 114, 101, 100, 101, 110, 99, 105, 97, 108, 45, 102, 99, 105,
            93, 97, 10, 111, 10, 136, 91, 34, 116, 111, 107, 101, 110, 34, 58, 34, 118, 97, 108,
            111, 114, 118, 97, 108, 111, 114, 118, 97, 108, 111, 25, 41, 45, 102, 105, 99, 116,
            105, 99, 105, 111, 34, 125,
        ];
        let s = String::from_utf8_lossy(bytes);
        let once = redact(&s).into_owned();
        assert_eq!(redact(&once), once, "una vez: {once:?}");
    }
}
