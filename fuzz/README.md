# Fuzzing de Symphony (P13.S2, STACK §24.5)

Targets de [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) para los
parsers críticos. Cada target tiene su propiedad equivalente en proptest dentro
del crate dueño (`cargo nextest run -p <crate>`), así el gate corre en stable y
en CI; esto es el corpus pesado para correr a mano o en un job de CI con
nightly (libFuzzer no corre en MSVC: usar WSL o `ubuntu-latest`).

## Correr

```sh
cargo +nightly fuzz run frame_decoder   # o ctx_uri, redact, claude_stream
```

Necesita nightly (`rustup toolchain install nightly`). Corre cada target al
menos 30 min antes de cerrar P13.S2 y guarda el corpus interesante en
`corpus/<target>/`.

## Propiedad de cada target

| Target | Parser | Propiedad |
| --- | --- | --- |
| `frame_decoder` | `symphony_protocol::FrameDecoder` | bytes arbitrarios partidos en cualquier punto: sin panic, frames ≤ `MAX_FRAME_LEN`, nada consumido sin frame completo |
| `ctx_uri` | `symphony_context::uri::CtxUri::parse` | cualquier texto: sin panic; lo aceptado es canónico (`to_string() == entrada`) y no escapa de la raíz |
| `redact` | `symphony_core::redact` | cualquier texto: sin panic y redactar es idempotente |
| `claude_stream` | `ClaudeAdapter::parse_stream_line` | cualquier línea de stdout (basura o JSON arbitrario): sin panic y determinista |
