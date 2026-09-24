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

### P02.S2 · Crate `core`: tipos de dominio — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** IDs ULID tipados (`ProjectId`, `SessionId`, `TaskId`, `AgentId`, `RunId`, `WorktreeId`, `CheckpointId`), con serde como texto. Diez enums con los valores exactos de DB (`AgentState` ×12, `TaskStatus`, `RunStatus`, `RunEndReason`, `ExecutionMode`, `FailoverPolicy`, `ContextMode`, `ProviderState`, `QuotaCertainty`, `FailureType`). Transiciones puras de `AgentState` y `TaskStatus`, más `requires_reason()` (FLOW §7: frase humana obligatoria).
- **Archivos clave:** `crates/core/src/{ids,enums,transitions}.rs`
- **Cómo se verificó:** `cargo xtask check` → 24 passed. `values_match_db_spec` lee `docs/spec/symphony_database.md` y exige que cada enum coincida con una fila de DB en valores y orden. Tablas exhaustivas de transiciones (12×12 y 8×8). 2 proptest: una transición inválida nunca cambia el estado.
- **Pendiente / notas:** los ~30 enums restantes de DB se agregan en la fase que los use.

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|

## Decisiones tomadas
- **Tabla de transiciones de `AgentState` y `TaskStatus`** (`crates/core/src/transitions.rs`): FLOW §7/§9.2 solo listan los estados. La tabla sale de FLOW §6 (sin agentes "medio rotos"), §9.3 (dependencias → READY/BLOCKED), §10.2 (WAITING_RESOURCE) e IDEA §5.10 (reclaim: FAILED → READY). Un estado nunca pasa a sí mismo; COMPLETED/DONE y CANCELLED son terminales. Si FLOW agrega una tabla explícita, gana FLOW y se ajusta el test.

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
| ulid | 3 | IDs de entidades | Sí (en ulid 3, `Ulid::new()` pasó a llamarse `Ulid::generate()`) |

## Métricas
(benchmarks, tiempos, RAM, cobertura — con comando)

## Pruebas
- Comando(s): …
- Totales: N passed, M failed (cuáles y por qué)

## Estado final
Resumen de 3–5 líneas para Leo.

## Notas para el siguiente agente
Trampas, comandos útiles y lo que no vale la pena reintentar.
