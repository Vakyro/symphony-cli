#![no_main]

//! Redactor central de secretos (STACK §22): corre sobre cualquier
//! stdout/stderr persistido. Ninguna entrada puede hacer panic y redactar
//! dos veces debe dar lo mismo (idempotencia).

use libfuzzer_sys::fuzz_target;
use symphony_core::redact;

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    let once = redact(&s);
    assert_eq!(redact(&once), once, "redact debe ser idempotente");
});
