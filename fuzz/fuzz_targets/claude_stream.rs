#![no_main]

//! Parser de líneas del stdout de Claude Code (`--output-format stream-json`):
//! líneas rotas, basura o JSON con campos raros no pueden hacer panic ni
//! producir eventos distintos entre corridas.

use libfuzzer_sys::fuzz_target;
use symphony_adapter_claude::ClaudeAdapter;
use symphony_adapter_common::ProviderAdapter;

fuzz_target!(|data: &[u8]| {
    let line = String::from_utf8_lossy(data);
    let adapter = ClaudeAdapter::default();
    let events = adapter.parse_stream_line(&line);
    assert_eq!(
        events,
        adapter.parse_stream_line(&line),
        "el parser del stream debe ser determinista"
    );
});
