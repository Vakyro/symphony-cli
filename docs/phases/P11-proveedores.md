# P11 · Proveedores restantes

| Campo | Valor |
|---|---|
| Estado | EN CURSO (adelantada antes de P08–P10 por ADR-0008) |
| Rama | phase/p11-proveedores |
| Inicio / cierre | 2026-10-01 / — |
| Agentes que trabajaron | claude-code/sonnet-5.5 (S1–S5) |
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

### P11.S4 · Adapter Copilot — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-01
- **Qué se hizo:** crate `symphony-adapter-copilot` (id de proveedor `github`, CLI `copilot`): prompt por stdin sin `-p`, `--resume=<id>`, id de sesión desde el `result`, herramientas (shell → comando; `create`/`apply_patch` → edición con la ruta del parche), permiso denegado → herramienta fallida, errores de stderr con `parse_stderr_line` (ADR-0009). Modelo único `github/auto` (lo único que aceptó `--model` en la cuenta de las pruebas). Permisos: `--allow-tool=write` por defecto (equivale a `acceptEdits`) y `allow_all_tools` opt-in. Registrado en `providers.rs`. Sin cambios en el core ni dependencias nuevas.
- **También:** `providers::detect_all` ahora detecta los CLIs **en paralelo** (`std::thread::scope`, orden conservado). Con Copilot (`--version` de 1,7–3,4 s) el arranque del daemon habría pasado de ~0,5 s a ~5 s y los tests con daemon chocaban entre sí (ver LEARNINGS).
- **Archivos clave:** `crates/adapters/copilot/`, `crates/daemon/src/providers.rs`, `fixtures/providers/copilot/`
- **Cómo se verificó:** `cargo xtask check` → 281 passed (9 nuevos). Live L3 `live_copilot_remembers_after_a_message_past_the_turn` (`github/auto`): 2 runs, 1 sesión, recordó el dato (65 s).
- **Pendiente / notas:** solo `auto` como modelo (un plan con más modelos no los ve; habría que sondear o leer config); no hay tokens de uso, así que el failover por umbral no aplica a Copilot; `FileModified` se anota al pedir la edición (si luego se deniega, el git status corrige); skills de Copilot sin investigar; `parse_error` de cuota/auth sintético.

### P11.S5 · Matriz de handoff — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-01
- **Qué se hizo:** forced kill cruzado (Test D) en dos capas. **L2:** `handoff_matrix_from_<proveedor>` (5 tests, 4 destinos cada uno = 20 pares dirigidos) con `fake-agent` bajo los ids reales (`anthropic`, `openai`, `moonshot`, `google`, `github`): A edita dos archivos, anuncia el siguiente paso y se cuelga; el watchdog lo mata sin cleanup (`NO_HEARTBEAT`); B continúa solo con el handoff. Se verifica el estado, los dos runs, el handoff `CONTINUED`, que el prompt de B lleve el objetivo, el «qué seguía» y los archivos de A, y los 4 archivos finales en el worktree. **L3:** `live_handoff_*` con CLIs reales: A recibe una tarea de 6 archivos encadenados (cada uno se crea leyendo el anterior), `symphony switch` lo corta al crear su primer archivo (termina su árbol de procesos) y B la termina.
- **Archivos clave:** `crates/daemon/tests/runtime.rs` (matriz L2), `crates/cli/tests/live_handoff.rs` (L3)
- **Cómo se verificó:** `cargo xtask check` → 291 passed, tres corridas seguidas. Live L3 (2026-10-01): ver la matriz.
- **También:** `symphony_testkit::pinned_bin` y los helpers de test de cli, tui, daemon y testkit ahora ejecutan una copia fija de `symphonyd` y `fake-agent` (ver «Desviaciones» y LEARNINGS): resuelve la carrera de «Acceso denegado» que ya existía.
- **Pendiente / notas:** el corte live es un `switch` (el runtime termina el proceso), no un crash externo del CLI; n = 1 por par; los 17 pares live restantes no se probaron.

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
| Adapter de Copilot pasa la suite de contrato con salidas reales | `cargo nextest run -p symphony-adapter-copilot` | ✅ 9/9 |
| Copilot retoma su sesión y recuerda lo dicho, con el daemon real | L3 `live_resume` | ✅ |
| Los 20 pares dirigidos entre los 5 proveedores sobreviven a un forced kill con `fake-agent` | `cargo nextest run -p symphony-daemon handoff_matrix` | ✅ |
| Claude→Kimi, Codex→Copilot y Antigravity→Claude terminan una tarea cortada a media | L3 `live_handoff` | ✅ |

## Matriz de handoff (P11.S5)

**L2 · `fake-agent`, 20 pares dirigidos: ✅ todos** (origen en filas, destino en columnas; cada par: A muere por `NO_HEARTBEAT`, B termina con solo el handoff).

| A \ B | Claude | Codex | Kimi | Antigravity | Copilot |
|---|---|---|---|---|---|
| **Claude** | — | ✅ | ✅ | ✅ | ✅ |
| **Codex** | ✅ | — | ✅ | ✅ | ✅ |
| **Kimi** | ✅ | ✅ | — | ✅ | ✅ |
| **Antigravity** | ✅ | ✅ | ✅ | — | ✅ |
| **Copilot** | ✅ | ✅ | ✅ | ✅ | — |

**L3 · CLIs reales (2026-10-01, `SYMPHONY_LIVE=1`).** Tarea: 6 archivos `n1.txt`…`n6.txt` encadenados (cada uno se crea leyendo el anterior). A se corta al crear su primer archivo (o a los 45 s); B recibe solo el handoff. Resultado esperado: `n1`…`n6` con 1…6.

| Par | Corte (A) | B terminó en | Total | Resultado |
|---|---|---|---|---|
| Claude `haiku` → Kimi `default` | 19 s, 1/6 archivos | 52 s | 72 s | ✅ 1…6 |
| Codex `gpt-5.6-luna` → Copilot `auto` | 34 s, 1/6 | 26 s | 61 s | ✅ 1…6 |
| Antigravity `gemini-3.8-flash-low` → Claude `haiku` | 33 s, 1/6 | 35 s | 68 s | ✅ 1…6 |

Otras corridas: Claude → Kimi con una espera de 10 s cortó a los 12 s con 0/6 archivos (B partió solo del objetivo y también terminó los 6 en 33 s); Antigravity → Claude cortó a los 12 s con 1/6 y B terminó los 6 en 28 s (esa corrida falló solo por una aserción mía sobre el id del proveedor, ya corregida). Los tiempos dependen de la red y del arranque de cada CLI (~27k tokens de contexto al abrir una sesión en Claude, ~22k en Antigravity).

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|
| Antigravity en headless deniega el shell con `accept-edits` y se cuelga con `--sandbox` en Windows | un chat con Antigravity no ejecuta comandos salvo `skip_permissions` | `agy … --mode accept-edits` + `run_command` | decisión de Leo |
| Copilot con `auto` puede cambiar de modelo entre turnos | no hay «modelo exacto»; además solo `auto` es seleccionable en esta cuenta | `copilot --resume=<id>` | sondear modelos por cuenta |

## Decisiones tomadas
- ADR-0008: P11 antes de P08–P10.
- ADR-0009: el executor entrega stderr al adapter (`parse_stderr_line`).

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| PLAN P11.S1 | tres archivos `cli-*.md` | uno solo, `cli-p11.md` | contrato común |
| PLAN P11.S1 | mini Test C por CLI | no hecho | el chat no lo necesita (ADR-0008 §2) |

## Desviaciones (S5)
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| PLAN P11.S5 | «forced kill cruzado» | el live corta con `switch` (el runtime termina el árbol de procesos de A) | un crash externo del CLI real no se puede provocar de forma fiable; el efecto sobre el handoff es el mismo |
| Infraestructura de tests | — | `pinned_bin` en `symphony-testkit` + dev-dependency de `symphony-testkit` en `cli` y `tui` | los tests ejecutaban `target/debug/symphonyd.exe` y otros lo recompilaban: «Acceso denegado» en Windows |

## Dependencias agregadas
Ninguna externa (solo `symphony-testkit` como dev-dependency interna de `cli` y `tui`).
