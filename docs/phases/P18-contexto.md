# P18 · Contexto y ahorro de tokens

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p18-contexto |
| Inicio / cierre | 2026-10-30 / — |
| Agentes que trabajaron | codex/gpt-6.1 (P18.S1 parte 1), codex/gpt-6-luna (P18.S4 parte 1), kimi-code (P18.S4 cierre), kimi-code/kimi-for-coding (P18.S3: WIP #95 y cierre #96) |
| Tag | — |
| Docs usados | PLAN § P18, ADR-0013 (enmienda 2026-10-30, D2), DECISIONES-2026-10-29 (29–30, 50), CONSTRAINTS §6, LEARNINGS.md |

## P18.S3 · Sesiones cortas con compactación automática — ✅

- **Agente:** kimi-code/kimi-for-coding · **Fecha:** 2026-10-31 (tercer intento: #91 paró sin código; #95 (Kimi) dejó un WIP sin verificar en `a84603d` y su proceso murió; este intento hace cherry-pick de ese WIP, lo revisa, lo corrige y lo verifica).
- **Qué se hizo:** contador de tareas terminadas por sesión (run) con el tope configurable `[plan] compact_every_tasks` (default 3, ya existía en config desde P18.S5). El daemon anota en memoria cuántas tareas del plan estaban hechas al abrir cada run (`Inner.plan_done`; se anota en `start_successor`/`continue_session` leyendo `.symphony/plan/<task>.md`, y con el valor ya leído para el run de compactación). Al cerrar una sesión con fases pendientes, `close_phase_and_continue` compara las hechas contra la base: si la sesión llegó al tope, cierra el run como siempre (`HANDED_OFF`/`PHASE_DONE`, transiciones existentes, sin estados nuevos) y deja al agente `READY` con la decisión pendiente. `agent.inspect` expone `agent.awaits_compact` y hay un IPC nuevo `agent.long_session`.
  - **Compactar** (`agent.next_phase`, opción por defecto de la TUI): si el adapter declara compactación por comando (`ProviderAdapter::slash_compact`, solo Claude Code devuelve `Some("/compact")`) y el run anterior dejó `cli_session_id`, se abre un run que **retoma la misma sesión del CLI** y le manda el comando; al salir bien, `advance_plan` ve el plan sin cambios y arranca la fase normal (la base anotada evita recompactar). Si el proveedor no tiene `/compact` (fake y todos los demás), la sesión nueva arranca con el **handoff completo del assembler** (prompt del assembler: decisiones/constraints en la conversación, archivos tocados, estado de git y plan) en vez del prompt acotado a fase.
  - **Sesión larga** (`agent.long_session`): solo tras confirmación explícita; arranca la fase siguiente con el prompt acotado normal, sin compactar. Interpretación registrada: «automático» = Symphony ejecuta la compactación (el usuario no escribe `/compact` a mano); la elección en el tope pasa por el prompt de la TUI, cuya opción por defecto (Enter) es compactar. Una sesión larga sin confirmación no existe: el agente queda READY hasta decidir (mismo patrón que `phase_start = "ask"`).
  - La TUI muestra el prompt en la cabecera del agente («Compactación: … Enter compactar y seguir · L sesión larga») y acepta solo esas dos teclas mientras `awaits_compact`.
  - **Corrección de este intento (bug del WIP de #95):** `close_phase_and_continue` borraba la base del contador (`clear_plan_done`) *antes* de leerla (`done_since`), así que la cuenta de tareas de la sesión caía a «todas las del plan»: cualquier cierre con ≥3 tareas hechas en total compactaba (aunque la sesión hubiera hecho 1) y el run de compactación, al cerrarse, volvía a pedir compactación **sin fin**. Se movió el borrado después de la lectura y `confirm_next` dejó de recomputar `compact_due` contra la base ya consumida: usa `awaits_compact`, la decisión exacta guardada al cerrar. La limpieza de la base también se añadió en `archive_plan` y `stop_on_failed_plan`. El resto del WIP (adapters, IPC, TUI, tests) se mantuvo: solo necesitó `rustfmt`.
- **Archivos clave:** `crates/daemon/src/plan.rs` (contador, decisión, `start_compact_run`, `confirm_next`), `crates/daemon/src/executor.rs` (`note_plan_done` en `start_successor`/`continue_session`, `awaits_compact` en `inspect`), `crates/daemon/src/runtime.rs` (campos `plan_done`/`awaiting_compact`), `crates/daemon/src/server.rs` (`agent.long_session`), `crates/adapters/common/src/lib.rs` (`slash_compact`), `crates/adapters/claude/src/lib.rs`, `crates/testkit/src/fake_adapter.rs` (proveedor `fake-compact`), `crates/tui/src/app.rs`, `crates/tui/src/ui.rs`.
- **Pruebas (corridas en este intento):** L2 con fake-agent en `crates/daemon/tests/runtime.rs`: `three_tasks_in_a_session_wait_for_compact_then_handoff` (3 tareas → READY con decisión pendiente → confirmar da handoff del assembler → COMPLETED) y `a_compact_capable_provider_runs_slash_compact_on_the_same_session` (`/compact` sobre la misma sesión del CLI), ambos en verde. Test de la TUI `compact_prompt_offers_compact_or_long_session` dentro de `cargo nextest run -p symphony-tui`: **149 passed**. `cargo nextest run -p symphony-daemon -E 'not(binary(daemon))'`: **167 passed, 1 skipped**; el único fallo es `handoff_combines_the_checkpoint_with_live_git`, regresión preexistente documentada de P18.S1 (falla igual sin este cambio). `cargo nextest run -p symphony-adapter-common -p symphony-adapter-claude -p symphony-testkit`: **45 passed**. `cargo clippy -p symphony-daemon -p symphony-tui -p symphony-adapter-common -p symphony-adapter-claude -p symphony-testkit -- -D warnings` y `cargo fmt --check` verdes, todo con las variables `SYMPHONY_*` fuera del ambiente. No se corrió `cargo xtask check` (lo corre quien integra) ni tests live (la medición L3 con Codex sigue pendiente del OK de Leo, como dice la enmienda).
- **Pendiente:** medición live (L3) del ahorro real de tokens con la compactación, pendiente del permiso de Leo (PLAN P18.S3 «Verifica: medición live con Codex pendiente»).

## P18.S4 · Grafo del proyecto al iniciar — ✅
- **Agente:** codex/gpt-6-luna (parte 1, sin verificar) + kimi-code (verificación y cierre) · **Fecha:** 2026-10-30.
- **Qué se hizo:** grafo local del proyecto 100 % en Rust sobre `crates/context` (`graph.rs`): archivos (recorrido acotado y ordenado con `ignore`), símbolos principales por archivo reutilizando los niveles L0–L5 de tree-sitter ya existentes (`ast::symbols`), crates y dependencias entre crates resueltas desde los `Cargo.toml` (incluidas `workspace = true` y secciones `target`). El daemon (`crates/daemon/src/project_graph.rs`) construye el grafo en segundo plano al lanzar un agente y al abrir el chat (`chat_get`), con un trabajador por raíz y un semáforo global; refresca al cerrar cada fase del plan (`plan.rs`). El JSON se guarda en `.symphony/graphs/<hash>/graph.json` y el resumen (≤ 8 KB, con `redact`) se indexa en el índice ctx como `ctx://file/.symphony/graphs/<hash>/summary.md` y se inyecta al prompt del agente como referencia resumida. El watcher no poda los grafos del índice. Sin Python, sin dependencias nuevas fuera de STACK (`ignore`, `serde`, `tempfile`, `toml_edit` ya estaban en el workspace).
- **Archivos clave:** `crates/context/src/graph.rs`, `crates/daemon/src/project_graph.rs`, `crates/daemon/src/executor.rs`, `crates/daemon/src/plan.rs`, `crates/daemon/src/server.rs`, `crates/daemon/src/watcher.rs`, `crates/daemon/src/runtime.rs`.
- **Pruebas:** `cargo nextest run -p symphony-context -p symphony-daemon -E 'not(binary(daemon))'` → los 3 tests nuevos pasan (`grafo_extrae_modulos_simbolos_y_dependencias_locales`, `grafo_respeta_ignorados_limites_y_manifiestos_rotos`, `arranque_no_espera_al_grafo_y_refresco_actualiza_ctx`, este último con barrera real de semáforo, no medición de tiempos). 114 passed; 1 failed preexistente documentado de P18.S1 (`a_context_transformer_rewrites_the_handoff_and_a_broken_one_is_ignored`, falla igual sin este cambio; interferencia ambiental de `SYMPHONY_USER_HOME`, sin causa confirmada). `cargo clippy -p symphony-context -p symphony-daemon -- -D warnings` y `cargo fmt --check` verdes. No se corrió `cargo xtask check` (lo corre quien integra).
- **Commit:** este commit (amend del wip de agente 92, ya verificado).

## P18.S5 · Init de proyecto actualizado — ✅
- **Agente:** codex/gpt-5.x · **Fecha:** 2026-10-30.
- **Qué se hizo:** `symphony init` ahora genera en `project.toml` el estilo del harness, las animaciones de UI y el compactado automático cada 3 tareas; el `.gitignore` del proyecto incluye `.symphony/brain/`; la CLI imprime un resumen de lo creado.
- **Archivos clave:** `crates/core/src/config.rs`, `crates/daemon/src/server.rs`, `crates/cli/src/main.rs`, `crates/cli/tests/projects.rs`.
- **Pruebas:** `unset SYMPHONY_LIVE; cargo nextest run -p symphony-cli --test projects` (3 passed); `cargo clippy -p symphony-cli -- -D warnings` (verde). La corrida completa del crate tuvo fallos ambientales previos en hooks/MCP y dejó LEAK; no se atribuyen a este cambio.
- **Commit:** pendiente.

## P18.S6 · Segundo cerebro — ✅
- **Agente:** kimi-code/kimi-for-coding · **Fecha:** 2026-10-30.
- **Qué se hizo:** el daemon regenera markdowns deterministas en `<proyecto>/.symphony/brain/` (ignorado por git): `decisiones.md` (de `executor_changes`: fecha, agente, modelo de→a, motivo legible), `cambios.md` (últimas 300 `tool_calls` de edición/escritura con estado y detalle recortado) y `bitacora/agente-N.md` (tarea, estado y tabla de runs por agente; las huérfanas se borran). Todo desde datos ya persistidos: consultas SQL acotadas y plantillas, sin embeddings ni LLM (CONSTRAINTS). Escrituras solo vía el daemon (`Runtime::refresh_brain`, en línea porque es barato: ni trabajador en segundo plano ni tablas nuevas), enganchado al cerrar runs (natural y fallido) y a las transiciones de fase del plan, junto al refresco del grafo. Sin dependencias nuevas; `export::stamp` pasó a `pub(crate)`.
- **Archivos clave:** `crates/daemon/src/brain.rs` (nuevo), `crates/daemon/src/executor.rs`, `crates/daemon/src/plan.rs`, `crates/daemon/src/lib.rs`, `crates/daemon/src/export.rs`, `.gitignore` (entrada explícita `.symphony/brain/`, ya cubierta por `.symphony/`).
- **Pruebas:** 3 tests nuevos con proyecto temporal (DB real en tempdir y `Runtime` real): render de los tres documentos desde datos sembrados, base vacía con texto amable, y escritura/regeneración idempotente que solo toca `agente-*.md`. `cargo nextest run -p symphony-daemon -E 'not(binary(daemon))' --no-fail-fast`: 159 passed, 1 skipped; 7 failed, de los que 6 pasan al quitar `SYMPHONY_*` del ambiente (interferencia ambiental documentada; el transformer tardó 83 s pero en verde) y 1 es la regresión preexistente de P18.S1 (`handoff_combines_the_checkpoint_with_live_git`, falla igual sin este cambio). `cargo clippy -p symphony-daemon -- -D warnings` y `cargo fmt --check` verdes. No se corrió `cargo xtask check` (lo corre quien integra).
- **Commit:** pendiente.

## Pasos

### P18.S2 · Flintstone por defecto — ✅
- **Agente:** codex/gpt-5.x · **Fecha:** 2026-10-30.
- **Qué se hizo:** el prompt del harness añade por defecto instrucciones Flintstone (respuestas comprimidas y directas, sin relleno). `[harness] style = "normal"` lo apaga; `mode` y los proveedores no cambian.
- **Archivos clave:** `crates/core/src/config.rs`, `crates/daemon/src/executor.rs`, `crates/adapters/common/src/lib.rs`, `crates/adapters/claude/src/lib.rs`, `docs/book/src/configuracion.md`.
- **Medición:** el sufijo Flintstone mide 152 caracteres, aproximadamente 38 tokens con la estimación fija de 4 caracteres/token usada por P18.S1. Es el coste del prompt inicial; no se añadió al benchmark de handoff porque ese benchmark no construye prompts de proveedor.
- **Pruebas:** `env -u SYMPHONY_LIVE cargo nextest run -p symphony-adapter-common` (13 passed); `env -u SYMPHONY_LIVE cargo clippy -p symphony-adapter-common -- -D warnings` (verde).
- **Commit:** `9d9ba05`.

### P18.S1 · Medición inicial de routing y handoff — 🟡
- **Agente:** codex/gpt-6.1 · **Fecha:** 2026-10-30.
- **Qué se hizo:** se añadió una prueba de benchmark ignorada, reproducible y sin red/cuota en el ensamblador puro de contexto. Mide cargas fijas que representan rotación por contexto, fallback por cuota, cambio manual y ruta limitada al mismo proveedor en RAW/SAFE/BALANCED/AGGRESSIVE. No cambia runtime.
- **Archivos clave:** `crates/context/src/handoff.rs`, `docs/research/tokens-p18-base.md`.
- **Cómo se verificó:** test del baseline → 1 passed; pendientes los nextest y clippy indicados por el plan de sesión.
- **Commits:** pendiente.
- **Pendiente / notas:** los escenarios miden el valor de handoff que consume el router, pero no ejercitan la selección de candidatos del daemon; se describe este límite en el baseline. La siguiente parte de S1 puede añadir cobertura de rutas reales antes de optimizar.

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|
| Ensamblado de cuatro modos y salida estable por escenario | `p18_routing_handoff_token_baseline` | 1 passed; tabla guardada en `docs/research/tokens-p18-base.md` |

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|
| No hay aún prueba integrada de selección de candidatos en daemon | El baseline mide handoff consumido por routing, no la decisión del router | Revisar alcance anotado en baseline | Continuar P18.S1 antes de cambiar comportamiento |

## Decisiones tomadas
- El benchmark usa la estimación determinista ya usada en runtime; no agrega tokenizador ni dependencia.
- Se reutiliza el fixture y el patrón de test ignorado existentes; no se añade comando xtask.

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| Petición P18.S1 | Cubrir las rutas del router y del handoff | Primera parte mide los tamaños de handoff consultados por routing; no llama a SQLite ni selecciona candidatos | Mantener esta parte pura y determinista; la cobertura integrada queda explícitamente pendiente |

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|
| — | — | Ninguna | — |

## Métricas
Comando de baseline arriba; conteos completos en `docs/research/tokens-p18-base.md`.

## Pruebas
- Comando: `cargo nextest run -p symphony-context p18_routing_handoff_token_baseline --run-ignored only --no-capture` → 1 passed.
- Gates de fase pendientes al cierre de este borrador.

## P18.S1 · Parte 2: recortes exactos — validación bloqueada

- **Agente:** codex · **Fecha:** 2026-10-30.
- **Cambio:** `crates/context/src/handoff.rs` sustituye mensajes idénticos al objetivo o plan completo por referencias más cortas en BALANCED/AGGRESSIVE. Preserva autor, orden y texto fuente; RAW/SAFE siguen completos. El router consume el tamaño resultante por `handoff_tokens`, sin necesitar otra implementación.
- **Pruebas nuevas:** dos regresiones de coincidencia exacta, preservación, ahorro, fidelidad y mensajes cortos; ambas pasaron. No se modificaron tests ni snapshots existentes.
- **Verificación:** con `SYMPHONY_LIVE` desactivado, benchmark previo 1 passed y tabla idéntica a la publicada; `cargo fmt -p symphony-context` y `git diff --check` correctos. `cargo nextest run -p symphony-context -p symphony-daemon -E 'not(binary(daemon))'`: 111 passed, 1 failed, 3 skipped; 99 no ejecutados por fail-fast.
- **Fallo:** `runtime::a_context_transformer_rewrites_the_handoff_and_a_broken_one_is_ignored` esperaba transformación a mayúsculas y obtuvo el prompt original. LEARNINGS documenta interferencias de `SYMPHONY_USER_HOME` en plugins; es una hipótesis, no una causa confirmada en esta sesión.
- **Parada:** se respeta AGENTS ante el gate fallido. Pendientes clippy, nueva medición posterior, comparación por ruta y cierre de S1. No se declara la meta del 40 % lograda. El benchmark fijo no contiene las repeticiones eliminadas, por lo que se espera ahorro cero en él; falta confirmarlo ejecutándolo.
- **Alcance y decisiones:** no se cambian fixtures ni presupuestos para mejorar artificialmente la cifra, ni se omiten archivos por un checkpoint potencialmente atrasado. Sin dependencias nuevas. Se conserva la rama asignada por Symphony según DOGFOOD; no se ejecutó `cargo xtask check` ni pruebas live.

## P18.S1 · Continuación de parte 2: segundo intento bloqueado

- **Agente:** codex/gpt-6 · **Fecha:** 2026-10-30.
- **Entorno:** antes de Cargo se eliminaron las variables SYMPHONY_* y CARGO_BUILD_BUILD_DIR del proceso de shell; no se modificó configuración global ni se invocaron proveedores reales.
- **Gate general:** `cargo xtask check` completó fmt y clippy del workspace con `-D warnings`; la herramienta interrumpió el comando a los 600 s durante la compilación de tests. No se declara aprobado.
- **Gate solicitado:** `cargo nextest run -p symphony-context -p symphony-daemon -E 'not(binary(daemon))'` → 168 passed, 1 failed, 3 skipped, 42 no ejecutados. El transformador que fallaba en la sesión anterior pasó (96,245 s).
- **Causa concreta del fallo nuevo:** `handoff_combines_the_checkpoint_with_live_git`, `crates/daemon/tests/runtime.rs:1686`, exige `**Asistente:** Voy por partes.`. La deduplicación lo sustituye por `[igual al Último plan del executor anterior]`; conservar el texto completo en el plan no satisface ese contrato. No se relajó ninguna aserción.
- **Parada:** segundo intento fallido de la tarea; plan local marcado `[!]` según instrucción del usuario. Sin nueva modificación de runtime ni tercer intento. Pendientes clippy acotado, benchmark posterior y tabla antes/después. No hay porcentaje medido ni cierre de S1.
- **Handoff:** corregir la deduplicación conservando el prefijo requerido de conversación; verificar tests existentes sin debilitarlos y luego medir. Requiere nueva autorización para reanudar tras la parada solicitada. Logs locales en `target/p18-validation/`.

## Estado final
P18.S1 parte 2 sigue incompleta: recortes implementados en el commit anterior, pero con una regresión confirmada en el contrato de conversación. Benchmark posterior pendiente; no se afirma alcanzar el 40 %. No se inició otra fase.

## Pausa del 2026-10-30 (orquestador)

Leo paró el trabajo al final del día. Estado de la rama `phase/p18-contexto`:

| Paso | Estado | Commit |
|---|---|---|
| S1 benchmark fijo | ✅ | `96b6719` |
| S1 recortes | 🟡 recorte chico (objetivo/plan no se repiten); test de handoff en rojo; cifra antes/después sin medir | `933fba9`, `ac90131` |
| S2 Flintstone por defecto | ✅ | `8d9fca6` |
| S3 `/compact` automático | ✅ (tercer intento #96: cherry-pick del WIP de #95, corrección del contador y verificación; ver § P18.S3) | — |
| S4 grafo al iniciar | ✅ (Rust, `crates/daemon/src/project_graph.rs`) | `a440358` |
| S5 `/init` | ✅ | `610eae7` |
| S6 segundo cerebro | ✅ (`.symphony/brain/`, ignorado) | `e66ad7c` |
| Enmienda ADR-0013 D2 | ✅ | `16f8227` |

**El benchmark no sirve para la meta del 40 %:** sus fixtures son sintéticos (mensajes de 800 «x», diff de «x»), así que ningún compresor puede mostrar ahorro. Hay que rehacerlo con contenido realista (tool calls, salidas largas de cargo, diffs unificados, logs repetidos, hechos ya presentes en el checkpoint) antes de recortar más.

**Para retomar:** (1) S3 a partir de `a84603d` (`git cherry-pick a84603d`, terminar y verificar); (2) arreglar el test de handoff y rehacer el benchmark realista; (3) gate completo; (4) revisión de cierre; (5) docs, merge y `p18-done`.

## Notas para el siguiente agente
El comando exacto para reproducir la tabla está en `docs/research/tokens-p18-base.md`. No lanzar CLIs reales; el benchmark es sintético y local.

## P18.S1 · Parte 3, fase 1: apertura del handoff corregida; validación detenida

- **Agente:** codex/gpt-6 · **Fecha:** 2026-10-31 · rama asignada `symphony/m9ndhjq7/agent-097` conservada según DOGFOOD.
- **Causa raíz:** la sustitución de mensajes idénticos al plan eliminaba también la apertura conversacional. `render_conversation` conserva ahora la primera línea de mensajes multilínea antes de la referencia y compara el tamaño de toda esa representación; si no ahorra, conserva el mensaje completo. Coincidencia exacta, autor, orden, límites y fidelidades siguen vigentes. RAW/SAFE sin cambios.
- **Alcance:** solo `crates/context/src/handoff.rs`; el daemon consume el ensamblador compartido. No se modificaron callers, interfaces, dependencias, snapshots ni aserciones existentes. Test nuevo para apertura, ahorro, conservación del plan y RAW/SAFE.
- **Prueba L2:** `cargo nextest run -p symphony-daemon --test runtime handoff_combines_the_checkpoint_with_live_git` → **1 passed, 110 skipped** (70,161 s), con la corrección compilada y el test original intacto.
- **Suite:** `cargo nextest run -p symphony-context -p symphony-daemon -E 'not(binary(daemon))'`. Intento 1: **35 passed, 1 failed, 3 skipped, 184 no ejecutados**; la fixture nueva tenía un espacio al final y la plantilla aplica `trim()`. Se quitó ese espacio en el dato, sin relajar la aserción de conservación completa. Intento 2: **166 passed, 1 failed, 3 skipped, 53 no ejecutados**; falló `cpu_keeps_beating_after_a_busy_child_ends`, runtime.rs:2184, `NO_HEARTBEAT` en vez de `COMPLETED`. La prueba nueva de contexto ya no falló. No se atribuye este segundo fallo a una causa ambiental sin investigarlo.
- **Entorno:** antes de cada Cargo se eliminaron todas las variables `SYMPHONY_*` y `CARGO_BUILD_BUILD_DIR`. `cargo fmt -p symphony-context` y `git diff --check` correctos. No se ejecutaron clippy posterior, xtask ni live.
- **Parada:** dos intentos fallidos de validación; tarea marcada `[!]` en el plan temporal solicitado, sin tercer intento. Corrección guardada, fase 1 sin aprobar. Pendientes clippy acotado y suite aprobada; no se iniciaron las fases 2–3 (fixtures realistas, línea base, optimizaciones y tabla antes/después). No se afirma alcanzar el 40 %.

## P18.S1 · Parte 3, fase 1: benchmark realista y línea base

- **Agente:** claude/opus-5.5 · **Fecha:** 2026-10-31 · rama asignada `symphony/m9ndhjq7/agent-099`.
- **Arreglo del handoff:** el cherry-pick de 86e12a8 resultó vacío (su contenido ya era HEAD, e41e0a6). L2 `cargo nextest run -p symphony-daemon --test runtime handoff_` → **14 passed, 97 skipped**, aserciones sin tocar.
- **Benchmark:** `p18_routing_handoff_token_baseline` carga ahora `fixtures/handoff-p18/` (conversaciones con tool calls y salidas reales de cargo/git, diffs unificados reales con `Cargo.lock`, un archivo de código nuevo, `plan_tail` armado como en `checkpoint.rs`). Test nuevo `p18_fixtures_load_as_alternating_conversations`. Línea base en `docs/research/tokens-p18-base.md` § «Benchmark realista».
- **Verificación:** `cargo nextest run -p symphony-context p18_ --run-ignored all` → 2 passed; `cargo clippy -p symphony-context -- -D warnings` limpio.
- **Siguiente (fase 2):** colapsar salidas de herramientas dentro de los mensajes en BALANCED/AGGRESSIVE en vez de recortarlas a ciegas y no repetir el final que ya trae `plan_tail`; meta −40 % por ruta en BALANCED.

## P18.S1 · Parte 3, fases 2–3: recortes deterministas y tabla antes/después

- **Agente:** claude/opus-5.5 · **Fecha:** 2026-10-31 · rama `symphony/m9ndhjq7/agent-099`.
- **Qué:** en BALANCED/AGGRESSIVE, `handoff.rs::squeeze_blocks` cambia los bloques ``` de salida repetidos en un mensaje posterior por referencia y resume los largos con `compress` (colapso de logs/tests) en vez del recorte ciego por el principio; el final de un mensaje igual al `plan_tail` pasa a referencia conservando la apertura; `compress/diff.rs` deja una línea de contexto por cambio (`-U1`) y cuenta el resto. RAW/SAFE intactos.
- **Archivos:** `crates/context/src/handoff.rs`, `crates/context/src/compress/{diff.rs,tests.rs,tests_summary.rs}`.
- **Resultado (BALANCED, antes → después):** rotation_context 16.016 → 11.127 (−30,5 %), fallback_quota 6.390 → 4.207 (−34,2 %), manual_switch 1.602 → 1.561 (−2,6 %), same_provider 6.468 → 1.654 (−74,4 %). Tabla completa por ruta y modo en `docs/research/tokens-p18-base.md` § «Antes/después».
- **Meta −40 %:** solo en same_provider. Rotation y fallback quedan cortas porque el resto es diff ya en contexto mínimo; manual_switch no tiene nada repetido que colapsar. No se forzó con pérdida.
- **Verificación:** `cargo nextest run -p symphony-context` 57 passed, 2 skipped; `p18_ --run-ignored all` 3 passed; clippy `--all-targets -D warnings` y fmt limpios. Tests de regresión nuevos: `p18_balanced_keeps_failures_openings_and_authors`, `a_message_ending_with_the_plan_keeps_its_head_and_points_to_the_plan` y los de `compress`. Sin xtask ni live.
- **Siguiente:** gate completo de P18, S7 (revisión y cierre).

## P18.S7 · Revisión de cierre (PLAN §4.6)

- **Agente:** claude/opus-5.5 · **Fecha:** 2026-10-31 · rama `symphony/m9ndhjq7/agent-101`. Revisado `git diff main...HEAD` (benchmark, recortes, Flintstone, `/init`, compact automático, grafo, segundo cerebro).
- **Sin hallazgos ALTOS.** Sin `unwrap/expect/panic` nuevos en runtime; el grafo y el cerebro solo escriben desde el daemon (writer para la base); `.symphony/brain/` queda ignorado (raíz `.gitignore` y el `*` de `.symphony/.gitignore`); el arranque solo encola el grafo.
- **MEDIOS arreglados (un test cada uno):**
  1. El contexto del grafo se pegaba también a los prompts de sesiones reanudadas: en el run de compactación quedaba `/compact <grafo>`, y Claude toma ese texto como instrucciones del resumen (podía centrarse en la estructura y soltar decisiones). Ahora solo va en sesiones nuevas (`executor.rs::with_graph_context`; test `el_grafo_no_se_pega_a_sesiones_reanudadas_ni_al_compact`).
  2. `symphony init` escribe `[harness] style` y `[plan] compact_every_tasks` en `project.toml`, pero el daemon solo leía `config.toml`: no tenían efecto. Ahora el estilo del proyecto pisa el global y `compact_every_tasks` del proyecto pisa solo ese campo (`plan.rs::plan_config_for`; test `el_project_toml_de_init_pisa_compact_y_estilo`).
  3. Handoff BALANCED/AGGRESSIVE: un bloque repetido se cambiaba por «igual a la de un mensaje posterior» aunque la copia posterior se recortara por el tope por mensaje → se perdía la salida. Un mensaje recortado ya no sirve de referencia (`handoff.rs`; test `dedup_no_remite_a_un_bloque_que_el_recorte_corta`).
  4. `chat.get` se sondea en cada tick de la TUI mientras no hay agente de chat y cada llamada pedía reconstruir el grafo entero (slot pesado ocupado sin fin). Ahora usa `ensure_graph`, que solo lo crea si falta (test `ensure_graph_no_reconstruye_si_ya_hay_grafo`).
- **BAJOS (pendientes, sin arreglar):**
  - `brain.rs`: celdas de tabla sin escapar `|` ni backticks (un comando con `|` rompe la fila); `refresh_brain` hace I/O síncrono en el hilo async con el lector bloqueado (barato; `spawn_blocking` si crece).
  - `squeeze_blocks`: un bloque repetido dentro del mismo mensaje se etiqueta «de un mensaje posterior» aunque esté en el mismo.
  - `plan_done` en memoria: tras reiniciar el daemon la base cae a 0 y la siguiente rotación pide compactar aunque la sesión hiciera menos tareas (documentado como conservador).
  - `symphony init` imprime «creado: .symphony/project.toml…» aunque ya existiera; `project_init` del server fija Flintstone/animaciones/3 sin opción por IPC.
  - Sin prueba L2 del texto exacto que recibe el CLI en el run de compactación (cubierto por la unitaria del prompt).
- **Entorno:** dentro de un agente de Symphony hay que quitar todas las `SYMPHONY_*` (no solo `SYMPHONY_LIVE`) antes de `cargo nextest -p symphony-daemon`: con `SYMPHONY_USER_HOME` heredado fallan `lifecycle_single_instance_and_methods` y `system_view_and_performance_profile_work_without_a_project`.
