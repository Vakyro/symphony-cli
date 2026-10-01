# P11 · Proveedores restantes

| Campo | Valor |
|---|---|
| Estado | EN CURSO (adelantada antes de P08–P10 por ADR-0008) |
| Rama | phase/p11-proveedores |
| Inicio / cierre | 2026-10-01 / — |
| Agentes que trabajaron | claude-code/sonnet-5.5 (S1–S3) |
| Tag | p11-done (pendiente) |
| Docs usados | STACK §18.4–§18.6, `docs/research/cli-p11.md`, ADR-0008 |

## Pasos

### P11.S1 · Investigación — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-01
- **Qué se hizo:** reconocimiento por `--help` y pruebas en vivo autorizadas por Leo (un turno, un `resume` y un modelo inexistente por CLI; `--session-id` en Copilot). Los tres hacen headless con salida estructurada y `resume` conserva el contexto.
- **Archivos clave:** `docs/research/cli-p11.md`, `fixtures/providers/{kimi,copilot,antigravity}/`
- **Cómo se verificó:** 7 turnos reales con exit 0; las fixtures no contienen rutas ni usuario.
- **Pendiente / notas:** ToS de los tres (Leo); hooks/Test C no investigado (no hace falta para el chat); errores de cuota solo sintéticos; eventos de herramientas sin observar.

### P11.S2 · Adapter Kimi — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-01
- **Qué se hizo:** crate `symphony-adapter-kimi` (id de proveedor `moonshot`): `spawn`/`resume`/`attach` con `-S`, prompt por stdin, eventos de texto y herramientas (Shell → comando, `WriteFile`/`StrReplaceFile` → edición), `parse_error`, modelo `moonshot/default` (sin `-m`; los modelos salen de la config de Kimi). Registrado en `providers.rs`. **ADR-0009:** el trait gana `parse_stderr_line` (por defecto vacío) y el executor lo aplica; Kimi saca de ahí su id de sesión. Sin dependencias nuevas.
- **Archivos clave:** `crates/adapters/kimi/`, `crates/adapters/common/src/lib.rs`, `crates/daemon/src/{executor,providers}.rs`, `crates/testkit/` (`--session-on stderr`), `fixtures/providers/kimi/`
- **Cómo se verificó:** `cargo xtask check` → 259 passed. L2 `session_id_from_stderr_is_stored_and_resumed` (falla sin el cambio del executor).
- **Live L3 (2026-10-01, permiso de Leo):** `live_kimi_remembers_after_a_message_past_the_turn` → 2 runs, 1 sesión compartida, recordó el dato tras `send` (38,8 s).
- **Pendiente / notas:** skills de Kimi (prefijo y catálogo) sin investigar; `parse_error` de cuota/auth es sintético; decisión de Leo sobre Kimi sin sandbox (`--print` auto-aprueba herramientas).

### P11.S3 · Adapter Antigravity — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-01
- **Qué se hizo:** crate `symphony-adapter-antigravity` (id de proveedor `google`, CLI `agy`): prompt por stdin como evento JSON `user`, `resume` con `--conversation`, id de sesión desde `init`, herramientas (`run_command` → comando; `write_to_file` y similares → edición), `TurnUsage` del último paso de respuesta, texto desde `result.response`, `denied_actions` explicado en el chat, `parse_error`. Cinco modelos Gemini; los de Anthropic y OpenAI que ofrece `agy` quedan fuera hasta revisar ToS. Permisos: `--mode accept-edits` por defecto (sin shell) y `skip_permissions` opt-in. Registrado en `providers.rs`. Sin cambios en el core ni dependencias nuevas.
- **Archivos clave:** `crates/adapters/antigravity/`, `crates/daemon/src/providers.rs`, `fixtures/providers/antigravity/`
- **Cómo se verificó:** `cargo xtask check` → 271 passed (10 nuevos). Live L3 `live_antigravity_remembers_after_a_message_past_the_turn` (`google/gemini-3.8-flash-low`): 2 runs, 1 conversación, recordó el dato (31,6 s).
- **Pendiente / notas:** con `accept-edits` el agente no puede ejecutar comandos (decisión de Leo: ¿`skip_permissions` desde la config?); el texto llega entero al final del turno (no hay deltas utilizables); skills de Antigravity sin investigar; `parse_error` de cuota/auth sintético; el id de conversación no se puede fijar.

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|
| Turno headless estructurado en Kimi, Copilot y Antigravity | live mínimo | ✅ |
| `resume` conserva contexto en los tres | live: «¿qué palabra te pedí?» → `ok` | ✅ |
| Copilot acepta `--session-id` para un UUID nuevo | live | ✅ |
| Adapter de Kimi pasa la suite de contrato con salidas reales | `cargo nextest run -p symphony-adapter-kimi` | ✅ 8/8 |
| El runtime guarda el id de sesión dado por stderr y lo retoma | L2 `session_id_from_stderr_is_stored_and_resumed` | ✅ |
| Kimi y Antigravity retoman su sesión y recuerdan lo dicho, con el daemon real | L3 `live_resume` (`SYMPHONY_LIVE=1`) | ✅ |
| Adapter de Antigravity pasa la suite de contrato con salidas reales | `cargo nextest run -p symphony-adapter-antigravity` | ✅ 10/10 |

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|
| Antigravity en headless deniega el shell con `accept-edits` y se cuelga con `--sandbox` en Windows | un chat con Antigravity no ejecuta comandos salvo `skip_permissions` | `agy … --mode accept-edits` + `run_command` | decisión de Leo |
| Copilot cambia de modelo al retomar con `auto` | rompe «modelo exacto» | `copilot --resume=<id> -p x` sin `--model` | S4: pasar siempre `--model` |

## Decisiones tomadas
- ADR-0008: P11 antes de P08–P10.
- ADR-0009: el executor entrega stderr al adapter (`parse_stderr_line`).

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| PLAN P11.S1 | tres archivos `cli-*.md` | uno solo, `cli-p11.md` | contrato común |
| PLAN P11.S1 | mini Test C por CLI | no hecho | el chat no lo necesita (ADR-0008 §2) |

## Dependencias agregadas
Ninguna.
