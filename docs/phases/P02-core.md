# P02 · Cimientos del core

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p02-core |
| Inicio / cierre | 2026-09-24 / — |
| Agentes que trabajaron | claude-code/opus-5.5 |
| Tag | — |
| Docs usados | STACK §3, §5, §6, §11, §21, §22, §45–§47, §49; IDEA §5, §6; FLOW §4.1 |

## Pasos

### P02.S1 · Crate `protocol` — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** framing `u32` BE + JSON con máximo de 8 MiB (validado antes de reservar memoria); `FrameDecoder` incremental puro; `read_frame`/`write_frame` async; `Message` = `Request`/`Response`/`Event`/`Subscribe` con `deny_unknown_fields`; la versión se valida antes de parsear el resto (`UnsupportedVersion`, `MissingVersion`); `Connection<S>` genérica sobre `AsyncRead + AsyncWrite`.
- **Archivos clave:** `crates/protocol/src/{frame,message,lib}.rs`, `crates/protocol/tests/interprocess.rs`
- **Cómo se verificó:** `cargo xtask check` → 14 passed (3 proptest de framing: frames partidos en cualquier punto y concatenados, stream truncado, cabecera sobredimensionada; formato estable en el wire; versión desconocida; campos desconocidos; 50 request/response sobre named pipe real con `interprocess`).

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|

## Decisiones tomadas
- ADR-NNNN: …

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|
| serde | 1.0.229 | Serialización de mensajes | Sí |
| serde_json | 1.0.151 | Wire format | Sí |
| thiserror | 2.0.21 | `ProtocolError` | Sí |
| tokio | 1.53 (`io-util`) | E/S async del codec | Sí |
| interprocess | 2.4 (dev) | Test sobre el transporte real | Sí |
| proptest | 1 (dev) | Tests de framing | No en §58, pero es la herramienta de STACK §24.4 |

## Métricas
(benchmarks, tiempos, RAM, cobertura — con comando)

## Pruebas
- Comando(s): …
- Totales: N passed, M failed (cuáles y por qué)

## Estado final
Resumen de 3–5 líneas para Leo.

## Notas para el siguiente agente
Trampas, comandos útiles y lo que no vale la pena reintentar.
