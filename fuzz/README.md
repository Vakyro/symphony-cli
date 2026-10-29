# Fuzzing de Symphony (P13.S2, STACK §24.5)

Targets de [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) para los
parsers críticos. Cada target tiene su propiedad equivalente en proptest dentro
del crate dueño (`cargo nextest run -p <crate>`), así el gate corre en stable y
en CI; los targets de libFuzzer corren a mano o en el job dedicado de CI con
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

## CI corta y corpus semilla

El job `fuzz` de `.github/workflows/ci.yml` corre en Ubuntu solo con push a
`main` o `workflow_dispatch`. Usa nightly-2026-10-01 y ejecuta cada uno de
los cuatro targets durante 60 segundos, con
`cargo fuzz run <target> -- -max_total_time=60`. La matriz no cancela los otros targets si uno falla.
Un crash hace fallar el job y su entrada se sube desde
`fuzz/artifacts/<target>/` como artefacto `fuzz-<target>`.

Cargo-fuzz carga las semillas de `fuzz/corpus/<target>/` de forma automática:

- `frame_decoder`: frame vacío y dos frames concatenados con cabecera u32
  big-endian y payload JSON. Son archivos binarios, no texto hexadecimal.
- `ctx_uri`: archivos, revisión, escapes canónicos, diff, mensaje, run y herramienta;
  sin salto de línea final, porque el parser rechaza caracteres de control.
- `claude_stream`: líneas completas de `fixtures/providers/claude-code/stream.jsonl`
  (hook, asistente y resultado), sin modificar su JSON.
- `redact`: texto corriente y credenciales ficticias en cabecera, variable y JSON.

Esta corrida corta no sustituye los 30 minutos por target del cierre de P13.S2.
