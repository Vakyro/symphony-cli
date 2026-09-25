# P05 · Adapters y event bus

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p05-adapters |
| Inicio / cierre | 2026-09-24 / — |
| Agentes que trabajaron | claude-code/opus-5.5 |
| Tag | — |
| Docs usados | IDEA §5.2, §5.3; STACK §6.3, §18, §20; DB §3.D, §3.F; docs/research/cli-*.md; ADR-0003, ADR-0005 |

## Pasos

### P05.S1 · Trait `ProviderAdapter` — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `symphony-adapter-common`: trait `ProviderAdapter` (síncrono, usable como `dyn`): `provider_id`, `detect`, `auth_status`, `list_models`, `supports_hooks`, `spawn_spec`/`resume_spec` (devuelven un `ProcessSpec` que lanza el supervisor genérico), `encode_prompt` (el prompt va por stdin, nunca como argumento), `encode_user_message` (`None` = solo entre turnos, ADR-0005), `parse_stream_line`, `parse_hook`, `parse_error`. `AgentEvent` canónico con `type_name()` = nombres de IDEA §5.3 para `events.type`. `ProviderError` con `FailureType` de DB. `hooks::parse_standard_hook`: esquema común de Claude Code y Codex. `contract::check`: suite compartida (robustez ante basura, fixtures de stream/hooks/errores, redacción de mensajes, spawn en el worktree con modelo y `SYMPHONY_AGENT_ID`, prompt por stdin). `FakeAdapter` en `testkit` sobre `fake-agent` (con `--model` y `--session-id` nuevos).
- **Archivos clave:** `crates/adapters/common/src/{lib,hooks,contract}.rs`, `crates/testkit/src/fake_adapter.rs`, `crates/testkit/tests/fake_adapter.rs`
- **Cómo se verificó:** el adapter fake pasa la suite de contrato; E2E: spawn con `symphony-process` → prompt por stdin → stream y hooks traducidos a eventos canónicos en el orden esperado. `cargo xtask check` → 115 passed.

### P05.S2 · Event bus — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `symphony_daemon::bus::EventBus`: `publish` persiste primero en `events` por el writer único (mpsc acotado, lotes; `type` = `AgentEvent::type_name()`, `payload_json` = el evento serializado) y después difunde por `broadcast` (TUI) y actualiza un `watch` con el último evento de cada agente. `EventSource` = valores de `events.source`.
- **Archivos clave:** `crates/daemon/src/bus.rs`, `crates/daemon/tests/bus.rs`
- **Cómo se verificó:** 5 000 eventos con un suscriptor que nunca lee (capacidad 16) y otro que lee todo: el productor no se frena, **los 5 000 quedan persistidos en orden**, el colgado recibe `Lagged`, el vivo cuenta los 5 000 (recibidos + perdidos por atraso) y el estado del agente queda al día. `cargo xtask check` → 116 passed.

### P05.S3 · `symphony hook emit` — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** subcomando oculto `symphony hook emit` (`crates/cli/src/hook.rs`): sin `SYMPHONY_AGENT_ID` no hace nada; si no, lee el JSON por stdin (máx. 4 MiB) y lo manda al daemon (`hook.emit`, con `SYMPHONY_RUN_ID`/`PROJECT_ID`/`PROVIDER`), esperando la decisión hasta 10 s. **Nunca rompe al CLI** (cualquier problema → exit 0 sin salida), **nunca arranca el daemon** y en P05 **no imprime nada** (imprimir `allow` saltearía los permisos propios del usuario; `deny` llega en P08). Se resuelve antes que todo en `main`. Daemon: `hook.emit` valida parámetros y que el agente exista en el proyecto, traduce con `parse_standard_hook` y publica en el bus; responde `{"decision":"allow"}`.
- **Archivos clave:** `crates/cli/src/hook.rs`, `crates/daemon/src/server.rs` (`hook_emit`), `crates/cli/tests/{hook_emit.rs,cmd/hook-emit-*.toml}`
- **Cómo se verificó:** trycmd sin env (no-op) y con payload inválido (silencio, exit 0); E2E: con el daemon caído el hook sale 0 y no lo arranca; con el daemon arriba 20 `PreToolUse` → 20 `CommandRequested` y un `PostToolUse` de Write → `ToolFinished` + `FileModified` en `events`; un agente inexistente no escribe nada. **Latencia del proceso `symphony hook emit` completo: mediana 30 ms, p90 46 ms, máx 65 ms** (Windows, debug).
- **Arreglos que salieron:** (1) la recuperación al arrancar convertía IDs a ULID y una fila rara impedía arrancar el daemon; ahora usa texto. (2) Los tests del CLI podían usar un `symphonyd` viejo; ahora se construye siempre (`tests/common`).

### P05.S4 · Adapter Claude Code — ✅ (L3 live sin correr: falta permiso)
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `symphony-adapter-claude`: `claude -p --input-format stream-json --output-format stream-json --verbose --model M --permission-mode <config> [--session-id|--resume]`; **nunca `--bare`**; binario nativo de npm (`@anthropic-ai/claude-code/bin/claude.exe`) en vez del shim `.cmd`; hooks por invocación con `--settings` (8 eventos, comando con comillas estilo bash, `timeout` 30 s por ADR-0003); prompt y mensajes a media tarea como líneas `stream-json` por stdin (ADR-0005). Fuentes: hooks para herramientas y turnos, stream para sesión (modelo real), texto, `api_retry` (error tipado), cuota KNOWN desde `rate_limit_event` (5 h y 7 días) y `result` con error. `parse_error` para límites de uso (diario/semanal), 429, auth, overloaded, modelo inexistente y red.
- **Fixtures L1:** `fixtures/providers/claude-code/{stream,hooks}.jsonl` sacados de P01 y sanitizados (usuario y rutas reemplazados; `fixtures/providers/README.md`).
- **Archivos clave:** `crates/adapters/claude/src/lib.rs`, `crates/adapters/claude/tests/claude.rs`
- **Cómo se verificó:** suite de contrato con 11 líneas de stream reales + 4 sintéticas documentadas, 9 hooks reales y 6 errores; cuota; comando de spawn/resume y JSON de `--settings`; codificación de mensajes. `cargo xtask check` → 121 passed. El live L3 (`SYMPHONY_LIVE=1`) existe pero no se corrió.

### P05.S5 · Adapter Codex — ✅ (L3 live no escrito: falta permiso)
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `symphony-adapter-codex`: `codex exec -s <sandbox> --json -m M … -` (prompt por stdin hasta EOF → `close_stdin_after_prompt() = true`, nuevo en el trait); `exec resume <id> -` con `-c sandbox_mode=…`; binario nativo de npm en vez del shim; hooks con 6 overrides `-c hooks.<Evento>=[…]` + `--dangerously-bypass-hook-trust`, comando en **PowerShell** (`& … …`) en Windows y en sh en Unix, siempre TOML válido; `encode_user_message = None` (solo entre turnos, ADR-0005). Stream: `thread.started` (sesión), `agent_message` (texto), `error`/`turn.failed` (errores; `Reconnecting…` = red transitoria, P01 Test D); los avisos `item.completed/error` se ignoran. `quota_from_rollout`: cuota KNOWN (5 h y 7 días) desde el rollout.
- **Fixtures L1:** `fixtures/providers/codex/{exec-stream,hooks}.jsonl` (P01, sanitizados).
- **Archivos clave:** `crates/adapters/codex/src/lib.rs`, `crates/adapters/codex/tests/codex.rs`
- **Cómo se verificó:** suite de contrato con 10 líneas reales + 1 sintética, 7 hooks reales y 5 errores; avisos vs. errores; comando de spawn/resume; comando del hook con comilla simple en la ruta; TOML del override; cuota desde un rollout con la forma real. `cargo xtask check` → 127 passed.

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
| STACK §18.1 | Trait con `async spawn/resume/stop/health` | Métodos síncronos: `spawn_spec`/`resume_spec` devuelven un `ProcessSpec`; `stop` es el `terminate_tree` genérico; la salud se deriva de `ProviderError` | Adapters puros y testeables sin procesos; el trait se usa como `dyn` en el registro |

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|

## Métricas
(benchmarks, tiempos, RAM, cobertura — con comando)

## Pruebas
- Comando(s): …
- Totales: N passed, M failed (cuáles y por qué)

## Estado final
Resumen de 3–5 líneas para Leo.

## Notas para el siguiente agente
Trampas, comandos útiles y lo que no vale la pena reintentar.
