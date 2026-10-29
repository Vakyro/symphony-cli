#![no_main]

//! Direcciones `ctx://` (STACK §47): la barrera que impide salir de la raíz.
//! Cualquier texto no puede hacer panic; lo que se acepta es canónico.

use libfuzzer_sys::fuzz_target;
use symphony_context::uri::CtxUri;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(uri) = CtxUri::parse(s) {
        assert_eq!(uri.to_string(), s, "una dirección aceptada debe ser canónica");
    }
});
