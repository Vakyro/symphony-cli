# P18 · Contexto y ahorro de tokens

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p18-contexto |
| Inicio / cierre | 2026-10-30 / — |
| Agentes que trabajaron | codex/gpt-6.1 (P18.S1 parte 1), codex/gpt-6-luna (P18.S4 parte 1), kimi-code (P18.S4 cierre) |
| Tag | — |
| Docs usados | PLAN § P18, CONSTRAINTS §6, LEARNINGS.md |

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
| S3 `/compact` automático | ⏸ primer intento (#91) paró sin código; segundo (#95, Kimi) se colgó: el proceso de Kimi murió y el daemon lo siguió viendo RUNNING. Su WIP quedó en `a84603d` (rama `symphony/7fkbtvmy/agent-095`, 13 archivos, +464), sin verificar ni integrar | — |
| S4 grafo al iniciar | ✅ (Rust, `crates/daemon/src/project_graph.rs`) | `a440358` |
| S5 `/init` | ✅ | `610eae7` |
| S6 segundo cerebro | ✅ (`.symphony/brain/`, ignorado) | `e66ad7c` |
| Enmienda ADR-0013 D2 | ✅ | `16f8227` |

**El benchmark no sirve para la meta del 40 %:** sus fixtures son sintéticos (mensajes de 800 «x», diff de «x»), así que ningún compresor puede mostrar ahorro. Hay que rehacerlo con contenido realista (tool calls, salidas largas de cargo, diffs unificados, logs repetidos, hechos ya presentes en el checkpoint) antes de recortar más.

**Para retomar:** (1) S3 a partir de `a84603d` (`git cherry-pick a84603d`, terminar y verificar); (2) arreglar el test de handoff y rehacer el benchmark realista; (3) gate completo; (4) revisión de cierre; (5) docs, merge y `p18-done`.

## Notas para el siguiente agente
El comando exacto para reproducir la tabla está en `docs/research/tokens-p18-base.md`. No lanzar CLIs reales; el benchmark es sintético y local.
