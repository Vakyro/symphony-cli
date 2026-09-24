# P02 · Cimientos del core

| Campo | Valor |
|---|---|
| Estado | CERRADA |
| Rama | phase/p02-core |
| Inicio / cierre | 2026-09-24 / 2026-09-24 |
| Agentes que trabajaron | claude-code/opus-5.5 |
| Tag | p02-done |
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

### P02.S3 · Config — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `SymphonyHome` (`$SYMPHONY_HOME` o `~/.symphony`, con `std::env::home_dir`, sin agregar `dirs`). `config.toml` tipado con defaults y plantilla comentada. `project.toml` con `[project]` y overrides opcionales. `set_value` edita con `toml_edit` conservando comentarios y **valida antes de escribir**. Escritura atómica (tmp + rename). Claves desconocidas → error con la ruta del archivo.
- **Archivos clave:** `crates/core/src/config.rs`; `PerformanceProfile` agregado a `enums.rs`
- **Cómo se verificó:** `cargo xtask check` → 31 passed (crear en frío, defaults parciales, editar sin perder comentarios, una edición inválida no toca el archivo, claves desconocidas y valores fuera de rango, project.toml sin pisar lo existente, plantilla == defaults).

### P02.S4 · Logging y redacción — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** redactor central `symphony_core::redact` (regex, 7 reglas): Authorization/Proxy-Authorization, Cookie/Set-Cookie, Bearer, campos JSON y `NOMBRE=valor` con nombre de credencial, formatos de llaves (Anthropic, OpenAI, GitHub, Google, AWS, Slack) y JWT. Logging del daemon: `tracing` → `RedactingWriter` → archivo rotativo diario `symphonyd.*.log` en `~/.symphony/logs/`, 14 archivos, non-blocking. El daemon pasa a ser lib + bin.
- **Archivos clave:** `crates/core/src/redact.rs`, `crates/daemon/src/logging.rs`, `crates/daemon/src/lib.rs`
- **Cómo se verificó:** `cargo xtask check` → 41 passed. Redactor: Bearer, API keys conocidas, cookies, variables de entorno, JSON y texto normal que **no** debe tocarse (`tokens_used=1234`, conteos de tokens). Logging: los campos de span se conservan y los secretos salen redactados; el archivo rotativo queda en el directorio con el secreto redactado.
- **Notas:** los nombres de credencial solo se aceptan si después de la palabra clave viene `_`/`-` o fin de nombre (así `tokens_used` no se redacta).

### P02.S5 · Daemon `symphonyd` — ✅ (Windows; Linux/macOS en CI)
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `protocol::transport` (`connect`, `listen`, `run_dir`). En Unix, socket de archivo `0600` en `<home>/run/` `0700` (no el namespace abstracto, que no tiene permisos). En Windows, named pipe con SDDL `D:P(A;;GA;;;OW)` (solo el dueño) y nombre con FNV-1a del home. Lock de instancia con `File::try_lock` + archivo de pid. Servidor tokio (2 workers) con `ping`, `status`, `shutdown`; `unknown_method`, `invalid_params`, `bad_request`; `Subscribe` responde `unsupported` explícito hasta P05. Timeout de 10 s por request; apagado con `CancellationToken` + `TaskTracker` (2 s de drenaje); Ctrl-C. `symphonyd` usa miette para los errores.
- **Archivos clave:** `crates/protocol/src/transport.rs`, `crates/daemon/src/{server,main,lib}.rs`, `crates/daemon/tests/daemon.rs`
- **Cómo se verificó:** `cargo xtask check` → 45 passed. Test de integración con el binario real: ping/status/errores; **un segundo daemon sale con "ya hay un daemon de Symphony corriendo … (pid N)"**; shutdown por IPC; el lock se libera; log y config creados; frames sobredimensionados y versión desconocida no tumban al daemon.
- **Pendiente / notas:** no se probó el acceso de otro usuario del SO (hace falta una segunda cuenta). La DACL `OW` y el `0600` son las garantías; se revisan en P02.S8 (`security-review`).

### P02.S6 · Cliente `symphony` — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** clap con `symphony` (sin args: aviso de TUI en P07), `status` (autoarranca), `daemon start|stop|status`, `--version`; errores con miette `fancy`. Autoarranque: `$SYMPHONYD` → binario hermano → PATH; spawn desacoplado (Windows: `DETACHED_PROCESS|CREATE_NO_WINDOW`; Unix: `process_group(0)`); stderr de arranque en `logs/symphonyd-start.err`, que se muestra si el daemon muere al arrancar. `SymphonyHome` pasa a ruta absoluta.
- **Archivos clave:** `crates/cli/src/{main,client}.rs`, `crates/cli/tests/{cli.rs,cmd/*}`
- **Cómo se verificó:** trycmd: `--help`, `--version`, sin args, comando desconocido y el ciclo del daemon (status detenido → start → start repetido → status → stop → stop repetido → status con autoarranque → stop). CI en verde en ubuntu, windows y macos.
- **Problemas resueltos:** (1) **H4 real:** el daemon heredaba el pipe de stdout de quien lanzó al CLI y trycmd se colgaba; se quita `HANDLE_FLAG_INHERIT` de los std handles propios antes del spawn (bloque `unsafe` aislado con `SAFETY`, CONSTRAINTS C6). (2) macOS: el socket en `$TMPDIR` supera `sun_path` (104 B) → fallback `/tmp/symphony-<hash>/` con verificación de dueño y permisos. (3) macOS no soporta `fchmod` en sockets → `mode(0o600)` solo fuera de macOS; la garantía es el directorio `0700`.
- **Test reemplazado:** `tests/version.rs::unknown_args_exit_with_usage_error` esperaba exit 2 sin argumentos; P02.S6 cambia ese comportamiento al aviso de la TUI. Lo cubren ahora `cmd/no-args.toml` y `cmd/unknown-command.toml`.

### P02.S7 · Integración — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** E2E `crates/cli/tests/e2e.rs`: `symphony status` en frío arranca el daemon → `daemon status` lo ve → **kill -9 / taskkill /F** → el CLI lo ve detenido → `symphony status` arranca uno nuevo (otro pid) aunque quedaron el lock y el socket del muerto → stop.
- **Cómo se verificó:** local (Windows) y en CI en los 3 OS.

### P02.S8 · Cierre — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** revisión de seguridad del diff de la fase (skill `security-review`): framing, parseo, control de acceso al socket y al pipe, métodos del daemon, autoarranque, bloque `unsafe`, redacción y escrituras de config. **Sin hallazgos con confianza ≥ 8.** Merge a `main` y tag `p02-done`.
- **Pendiente para P05:** en Windows el nombre del pipe es predecible; otro usuario local podría crearlo antes que el daemon (squatting). Hoy el impacto es nulo (solo ping/status/shutdown), pero antes de mandar prompts o eventos de hooks por IPC, el cliente tiene que verificar que el servidor del pipe pertenece al usuario actual (`GetNamedPipeServerProcessId` + dueño del token).


## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|
| `symphony status` en frío autoarranca el daemon | trycmd + E2E | ✅ ubuntu, windows, macos |
| Un solo daemon por home; error claro para el segundo | `crates/daemon/tests/daemon.rs` | ✅ |
| El CLI reinicia un daemon muerto sin cleanup | `crates/cli/tests/e2e.rs` | ✅ |
| Protocolo con proptest de framing | `crates/protocol/src/frame.rs` | ✅ |
| Enums idénticos a DB | `values_match_db_spec` | ✅ |
| Config con edición que conserva comentarios | `crates/core/src/config.rs` | ✅ |
| Logs redactados | `crates/daemon/src/logging.rs` | ✅ |

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|
| Acceso de otro usuario al socket o pipe no probado con una segunda cuenta | Bajo: la garantía es el SDDL y el directorio 0700 | — | Probar cuando haya una máquina con dos usuarios |
| Suplantación del pipe en Windows | Nulo hoy; relevante desde P05 | Otro usuario crea el pipe antes | Verificar el dueño del servidor en P05 |

## Decisiones tomadas
- **Tabla de transiciones de `AgentState` y `TaskStatus`** (`crates/core/src/transitions.rs`): FLOW §7/§9.2 solo listan los estados. La tabla sale de FLOW §6 (sin agentes "medio rotos"), §9.3 (dependencias → READY/BLOCKED), §10.2 (WAITING_RESOURCE) e IDEA §5.10 (reclaim: FAILED → READY). Un estado nunca pasa a sí mismo; COMPLETED/DONE y CANCELLED son terminales. Si FLOW agrega una tabla explícita, gana FLOW y se ajusta el test.

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| STACK §11 / FLOW §17 | Sin esquema explícito de config | Solo las claves que el spec ya define: performance.profile, routing.default_profile/failover, context.mode, providers.quota_reserve (DB §3.D), logging.level | YAGNI: cada fase agrega sus claves; `deny_unknown_fields` atrapa typos |

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
| toml_edit | 0.25.15 (`serde`) | Leer y editar config conservando comentarios | Sí |
| tempfile | 3.27 (dev) | Tests de config | Sí |
| regex | 1.13 | Redactor | Sí |
| tracing | 0.1.44 | Logs estructurados | Sí |
| tracing-subscriber | 0.3.23 (`fmt`, `env-filter`, `ansi`, `std`) | Formato y filtro | Sí |
| tracing-appender | 0.2.5 | Archivo rotativo non-blocking | Sí |
| tokio-util | 0.7.19 (`rt`) | `CancellationToken`, `TaskTracker` | Sí |
| miette | 7.6 | Errores de `symphonyd` | Sí |
| widestring | 1.2 (solo Windows) | SDDL del named pipe (`SecurityDescriptor::deserialize` pide `U16CStr`) | No, pero ya era dependencia transitiva de `interprocess`; MIT/Apache |
| clap | 4.6 (`derive`) | CLI | Sí |
| trycmd | 1.2 (dev) | Tests de CLI | No en §58, pero es la herramienta de STACK §24.2 |
| windows-sys | 0.61.2 (solo Windows, `Win32_Foundation`, `Win32_System_Console`) | Quitar la herencia de std handles antes de lanzar el daemon | Sí |
| (licencia) 0BSD | — | `doctest-file` y `recvmsg`, dependencias de `interprocess` | Se agregó `0BSD` a `deny.toml`: más permisiva que MIT |

## Métricas
(benchmarks, tiempos, RAM, cobertura — con comando)

## Pruebas
- Comando(s): `cargo xtask check`, `cargo deny check`, CI (ubuntu, windows, macos, msrv 1.95)
- Totales: 47 passed, 0 failed
- Totales: N passed, M failed (cuáles y por qué)

## Estado final
`symphony` y `symphonyd` hablan por IPC tipado y versionado en los 3 OS. El socket o pipe es accesible solo por el usuario. Hay un único daemon por home, se autoarranca y se recupera de un kill -9. La config es tipada, con edición que conserva comentarios. Los logs se rotan y se redactan. Nada de `unwrap()` en runtime (lint de workspace). Sin hallazgos de seguridad en la revisión; queda la verificación del dueño del pipe para P05.
Resumen de 3–5 líneas para Leo.

## Notas para el siguiente agente
Trampas, comandos útiles y lo que no vale la pena reintentar.
