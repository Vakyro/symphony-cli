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
