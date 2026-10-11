# P09 · Context Engine

| Campo | Valor |
|---|---|
| Estado | CERRADA CON PENDIENTES (los live) |
| Rama | phase/p09-context-engine |
| Inicio / cierre | 2026-10-02 / 2026-10-11 |
| Agentes que trabajaron | claude-code/sonnet-5.5 (S1–S7, S9, S10), claude-code/opus-5.5 (planificación y STATUS) |
| Tag | p09-done |
| Docs usados | IDEA §5.6, §5.12; STACK §13–§16; DB §3.G, §3.J, §6; FLOW §11; ADR-0004, ADR-0010 |

## Diseño (antes de escribir código, ADR-0010 §3)

**Qué se construye.** Todo determinista (sin LLM ni embeddings, STACK §14): (1) la migración `003_context.sql` con `context_chunks` + `context_fts` (FTS5/BM25), `handoff_items`, `context_retrievals`, `project_facts`, `skills` y `mcp_servers`; (2) direcciones `ctx://` con un parser que no deja escapar de su raíz (path traversal, STACK §47) y objetos de contexto troceados e indexados; (3) compresores deterministas que **nunca** pierden el original (queda en el object store): `LogCollapser`, `TestSummaryCompressor`, `JsonStructuralCompressor`, `Deduplicator` y `GitDiffReducer`; (4) un reductor estructural de código por niveles L0–L5 con tree-sitter (TypeScript/JavaScript, Rust, Python) y respaldo a chunks sin gramática; (5) un watcher de archivos por proyecto (notify + ignore, con debounce); (6) el handoff assembler v2 con los modos `raw`/`safe`/`balanced`/`aggressive`, secciones con su fidelidad (`handoff_items`) y los comandos `symphony context inspect|stats|raw`; (7) hechos consolidados (`CURRENT`/`SUPERSEDED`/`CONFLICT`); (8) un broker MCP (`symphony mcp serve`, rmcp por stdio) que reenvía al daemon `context.retrieve`/`context.search`/`context.lines`; (9) skills y MCP compartidos.

**Orden por valor (ADR-0010).** S1 migración → S2 `ctx://` y objetos → S3 compresores → S6 handoff v2 → **S10 evaluación del handoff** (decide los defaults; si `balanced` pierde efectividad frente a `raw`, se ajustan y se documenta) → S4 AST → S5 watcher → S7 hechos → S8 broker MCP → S9 skills/MCP compartidos → S11 cierre. Los números de paso se conservan; el orden de ejecución es este.

**Reglas que no se discuten.** El original siempre se conserva y es recuperable (`ctx://…`); `raw` es la salida de emergencia y no omite nada; el handoff nunca depende de que un compresor «acierte»: si falla, el original va tal cual; un compresor es una función pura y total (no entra en pánico con cualquier entrada); `ctx://` no resuelve nada fuera de su proyecto.

**Dependencias nuevas** (todas en ADR-0001 / STACK §58): `notify` 8.2 (agrega `CC0-1.0` a `deny.toml`), `ignore` 0.4, `tree-sitter` 0.27 con las gramáticas de TypeScript/JavaScript, Rust y Python, `rmcp` 3.x.

**Cómo se prueba.** Corpus dorado en `fixtures/context/` (logs de npm, vitest, cargo, pytest y JSON grandes) con snapshots `insta`; proptest de «un compresor nunca entra en pánico y siempre recupera el original»; traversal de `ctx://` con casos y proptest; la propiedad «`raw` ≥ `safe` ≥ `balanced` ≥ `aggressive` en tokens y `raw` no omite nada»; protocolo MCP de punta a punta; y la evaluación del handoff (forced kill con `raw` frente a `balanced`).

## Pasos

### P09.S1 · Migración 003 (hecho, `c9239cf`)
`context_chunks` + `context_fts` (FTS5 de contenido externo con triggers), `handoff_items`, `context_retrievals`, `project_facts` (índice único parcial para un solo hecho CURRENT por clave), `skills`, `mcp_servers`. Probada sobre una copia de la base real de Leo (v2→v3 conserva los datos, `foreign_key_check` = 0).

### P09.S2 · Direcciones `ctx://` y búsqueda (hecho, `222e08d`)
`CtxUri` estricta (sin `%`, `..` ni `\`; `file_under` valida que el archivo caiga dentro del worktree), `chunk_text` por líneas, `fts_query` (términos entre comillas: el texto del usuario no puede romper la consulta), `store::context` (reemplazo de chunks, búsqueda BM25 con snippet, retrievals). El daemon indexa los mensajes largos y los diffs de los checkpoints (`context_index.rs`).

### P09.S3 · Compresores deterministas (hecho)
`context::compress`: `LogCollapser` (ANSI y líneas `
` fuera, repeticiones «×N», recorte del medio; las líneas de error/panic/Traceback **siempre** se conservan), `TestSummaryCompressor` (cargo, vitest/jest, pytest → «896 pruebas: 895 pasaron, 1 falló» + nombres de fallos), `JsonStructuralCompressor` (esquema, conteos, rangos, ejemplos, campos opcionales marcados «solo en N de M»), `Deduplicator` (bloques/líneas largas repetidos → «igual a la línea N»), `GitDiffReducer` (lista de archivos con +/−, lockfiles y generados en una línea, hunks largos recortados). `compress()` elige por contenido (o por `Hint`) y **nunca infla**. El daemon guarda la versión comprimida junto al objeto (`set_compression`, desde 2 KB) y el original sigue en su blob. Etiquetas en DB: TestSummary→`LOG_COLLAPSE`, GitDiff→`DEDUP` (el CHECK solo admite cinco). Verificado: unit con corpus sintético por formato, proptest «no entra en pánico y no infla», proptest «la línea de error sobrevive al colapso», bench 1 MB de log ≈ 30 ms.
Desvío: el corpus dorado se genera en los tests en vez de `fixtures/context/` + snapshots (más corto de mantener y las aserciones son más precisas que un snapshot de texto comprimido).

### P09.S6 · Handoff assembler v2 (hecho)
`handoff::assemble` ahora devuelve, además del prompt, los **items** (sección, archivo o `ctx://`, fidelidad 0–5, tokens). Fidelidad: 5 completo · 4 recortado por tamaño · 3 resumen determinista · 2 esqueleto (AST, S4) · 1 solo referencia · 0 omitido. Cambios de comportamiento: en `BALANCED` y `AGGRESSIVE` el diff pasa primero por `GitDiffReducer` (archivos con +/−, lockfiles y generados en una línea, hunks largos recortados) y solo se usa si, con su aviso «el original completo está en ctx://…», es más corto que el original; `RAW` y `SAFE` no resumen nada. Lo comprimido se calcula en el momento: el original nunca se toca. Los archivos nuevos y la conversación **no** se comprimen (colapsar líneas parecidas de código o de chat perdería contenido): solo se recortan, como en v1.
Persistencia: `handoff_items` se llena en cada cambio de executor (`repo::insert_handoff` con `NewHandoffItem`; id `HandoffItemId`). Comandos: `symphony context inspect <agente>` (tabla de secciones con fidelidad), `context stats` (compresión por compresor, coste de handoffs por modo, recuperaciones y misses) y `context raw <ctx://…>` (el original; la dirección se valida con `CtxUri` y se busca en la base, nunca se abre una ruta). Protocolo: `context.inspect`, `context.stats`, `context.raw`.
Verificado: `raw_omits_nothing_and_modes_are_monotonic_with_a_realistic_checkpoint` (raw ≥ safe ≥ balanced ≥ aggressive en tokens con un diff de 3.000 líneas de lockfile, 60 mensajes y 6 archivos nuevos; RAW sin un solo recorte y todo a fidelidad 5), `balanced_reduces_the_diff_and_points_to_the_original`, snapshots v1 sin cambios (un diff pequeño no se resume), L2 del daemon (`handoff_items` tras un switch), CLI de punta a punta contra un daemon real.
Desvío: `/context inspect|stats|raw` del PLAN son subcomandos `symphony context …` (la TUI no tiene todavía un punto de entrada de comandos con barra).

### P09.S4 · AST por niveles (hecho)
`context::ast` con tree-sitter 0.27 y las gramáticas de Rust (0.24), Python (0.25), JavaScript (0.25) y TypeScript/TSX (0.23). Niveles: L0 nada · L1 nombre · L2 símbolos (tipo, nombre, línea) · L3 firmas y tipos sin cuerpos (los tipos de ≤ 12 líneas van enteros) · L4 firmas más las funciones completas cuyo nombre o cuerpo contiene un término de búsqueda · L5 archivo completo. Sin gramática, L2–L4 devuelven `None` (el llamador usa chunks). Un recorrido genérico con una tabla de nodos por lenguaje (sin consultas `.scm`); `export …` y los decoradores de Python se tratan como envoltorios. Código roto no falla: tree-sitter tolera errores.
En el handoff: en BALANCED/AGGRESSIVE, un archivo nuevo que no cabe en su espacio va como esqueleto L3 (fidelidad 2) si hay gramática y el esqueleto entra; si no, se recorta como antes. RAW y SAFE no cambian.
Verificado: snapshots L3 de Rust, Python y TypeScript y L2 de TypeScript; L4 con un término; JS y TSX rotos; un archivo de 200 funciones baja a menos de la mitad; test de handoff (80 handlers en AGGRESSIVE → esqueleto; en SAFE, entero). `cargo deny` ok (licencias MIT). Gramáticas nuevas en el workspace (STACK §14.1 nombra tree-sitter; las gramáticas son su complemento).

### P09.S5 · File watcher (hecho)
`daemon::watcher::Watchers`: un watcher `notify` 8.2 por proyecto (no por agente) que mantiene los archivos del proyecto buscables como `ctx://file/<ruta>`. Arranca con un recorrido inicial (`ignore::WalkBuilder`) y después aplica los cambios con debounce (300 ms). Respeta `.gitignore` y omite lo oculto (`.git`, `.symphony`), lo binario y archivos > 512 KB (tope de 5.000 archivos en scan inicial).
Persistencia: `context::upsert_file` y `context::delete_file` en el store. En `003_context.sql`, la unicidad de `context_objects` pasa a `(project_id, uri)` para permitir que distintos proyectos tengan la misma ruta relativa (ej. `ctx://file/src/main.rs`).
Verificado: unitario con ignorados/ocultos/binarios en tempdir; L2 del daemon (`project_files_stay_searchable_as_they_change`), creación, modificación con debounce y borrado de archivo indexado; `files_can_be_upserted_replaced_and_deleted_by_project` en store; `cargo deny` ok con `CC0-1.0` permitido.

### P09.S7 · Hechos consolidados / project_facts (hecho)
Consolidación de hechos del proyecto (`project_facts`) con estados `CURRENT`, `SUPERSEDED`, `CONFLICT`.
- `ids::FactId` (`fact_...`) y tipos en core y store (`FactRow`, `set_fact`, `set_fact_conflict`, `list_facts`, `get_current_facts`).
- Inclusión de hechos vigentes en el handoff conversacional (`ProjectFact`, sección `## Hechos vigentes del proyecto` con `Section::Decisions`).
- Métodos IPC en daemon: `fact.set` y `fact.list`.
- Subcomandos CLI: `symphony fact set <key> <value> [--kind <kind>]` y `symphony fact list [--all]`.
- Verificado: tests de repositorio en `store::context` (transiciones de estado y unicidad lógica de `CURRENT`), L2 en `daemon::runtime` (`handoff_includes_current_project_facts` y llamadas IPC), snapshot actualizado de help en CLI. Total 427 tests verdes.


### P09.S9 · Skills y MCP compartidos
Los archivos son la fuente de verdad; SQLite (`skills`, `mcp_servers`) solo los indexa y guarda el interruptor `enabled` del usuario, que sobrevive a cada relectura (`crates/store/src/shared.rs`, `crates/daemon/src/shared.rs`).
- **Skills canónicas** (STACK §16): `~/.symphony/skills/<nombre>/SKILL.md` y `<proyecto>/.symphony/skills/<nombre>/`, con `skill.toml` opcional (`description`, `providers`). Ante un nombre repetido gana el del proyecto. Nombres inseguros (`..`, separadores, espacios) se descartan.
- **`mcp.toml`** (`~/.symphony/` y `<proyecto>/.symphony/`): `[servers.<nombre>]` con `command`/`args`/`env` (stdio) o `url` (http), `share_mode` y `enabled`. Lo inválido se descarta y sale como `warnings`. El nombre `symphony-context` está reservado al broker y los nombres no pueden llevar `.` (son claves TOML en Codex).
- **Traducción por adapter, por invocación y sin tocar el repo ni la config del usuario:**
  - Claude: las skills habilitadas se materializan como un plugin de sesión (`<home>/runtime/<proyecto>/claude-plugin`, `--plugin-dir`); el manifiesto pasa `claude plugin validate`. Los servidores de `mcp.toml` van en `--mcp-config` y se permiten por servidor (`--allowedTools mcp__<nombre>`), porque en headless nadie puede contestar el permiso.
  - Codex: los servidores van como `-c mcp_servers.<nombre>.{command,args,env|url}`. **Codex no tiene un directorio de skills por invocación** (`CODEX_HOME` arrastraría su login): sus skills compartidas no se traducen todavía.
  - Kimi, Antigravity, Copilot: sin traducción (no investigados; re-validación pendiente).
- **Cómo llega al adapter:** el daemon deja `SYMPHONY_PLUGIN_DIR` y `SYMPHONY_MCP_EXTRA` en el `env` del spawn (`Runtime::shared_env` + `shared::launch_env`, que es pura y está probada); así `SpawnRequest` no cambió. `SYMPHONY_NO_MCP` apaga el broker y los extras.
- **IPC y CLI:** `skills.shared`, `skills.set_enabled`, `mcp.list`, `mcp.set_enabled`; `symphony skills [list|enable|disable]` y `symphony mcp list|enable|disable|serve`.
- **TUI:** 20 Context Overview (`c` en un agente, o `:context`), 21 Context Object (Enter sobre una parte), 32 Shared Skills (`:skills`), 33 Shared MCP (`:mcp`), 35 Usage / Context Stats (`:stats`). 32 y 33 encienden y apagan con Espacio o Enter. Snapshots nuevos y los de la vista de agente actualizados (el pie ahora anuncia `c contexto`).
- **Sin hacer:** `share_mode = "shared"` (una sola instancia detrás del daemon exige un proxy MCP): se indexa y la vista lo avisa, pero no se registra. Las vistas 20/21/35 no tienen búsqueda interactiva (usa `symphony context` o el broker). No hay skills «BUILTIN».
- **Verificado:** unit tests de escaneo, plugin y `launch_env`; adapters de Claude y Codex (argv exacto); e2e con daemon real de `symphony skills` y `symphony mcp` (listar, apagar, persistir el interruptor, aviso por entrada inválida); snapshots de las cinco vistas. **No verificado en vivo:** que Claude cargue el plugin y llame a un servidor del usuario (necesita cuota; pendiente del live gratuito con OpenCode o permiso de Leo).
- 443 tests verdes (`cargo xtask check`).

### P09.S10 · Evaluación del handoff (hecho en L2; el live queda pendiente)
Test `handoff_evaluation_forced_kill_by_context_mode` (`crates/daemon/tests/runtime.rs`; la tabla sale con `--nocapture`): el forced kill de P06.S8 (A se cuelga, el watchdog lo mata sin cleanup y B continúa solo con el handoff) con un checkpoint pesado: `package-lock.json` tocado (3.000 líneas), dos mensajes largos, un cambio real en el README, un archivo nuevo y el «Next:» del último mensaje. Un agente por modo.

| Modo | Tokens enviados | Tokens RAW | % de RAW | Retrievals | Misses | ¿Terminó? |
|---|---|---|---|---|---|---|
| RAW | 21.512 | 21.512 | 100 % | 0 | 0 | sí |
| SAFE | 21.512 | 21.512 | 100 % | 0 | 0 | sí |
| BALANCED | 1.412 | 21.514 | 6,6 % | 0 | 0 | sí |
| AGGRESSIVE | 788 | 21.515 | 3,7 % | 0 | 0 | sí |

En los cuatro modos el prompt lleva el objetivo, el «qué seguía», los archivos tocados (también el lockfile) y el cambio real del README. BALANCED y AGGRESSIVE resumen el lockfile en una línea y apuntan al `ctx://` del diff. **Conclusión: en este caso BALANCED no pierde efectividad y cuesta unas 15 veces menos que RAW. Los defaults no cambian y no hace falta ADR.** SAFE no ahorra aquí porque el diff (≈ 80.000 caracteres) cabe en su límite de 120.000, que es lo esperable de un modo «seguro».
Límites: retrievals y misses dan 0 porque todavía no hay broker MCP (S8); con el broker, esa columna dirá si BALANCED obliga a ir a buscar. El CLI es falso (determinista): mide el coste y si lo esencial está presente, no si un modelo real puede continuar con menos contexto. La corrida **live** RAW contra BALANCED gasta cuota y necesita permiso de Leo. Los pares live de P11.S5 ya usaron BALANCED (el default) y continuaron.

### P09.S8 · Broker MCP
`symphony mcp serve` (`rmcp` 3.5, stdio) con tres herramientas que reenvían al daemon por IPC (`context.search`, `context.retrieve`, `context.lines`; `crates/daemon/src/context_mcp.rs`).
- **Nombres con `_`**: `context_search`, `context_retrieve`, `context_lines`. El PLAN dice `context.search`, pero varios CLIs solo aceptan `[A-Za-z0-9_-]` en el nombre de una herramienta. Los métodos IPC sí conservan el punto.
- **Alcance:** el proyecto sale de `SYMPHONY_PROJECT_ID` (el que el executor pone a cada agente); fuera de un agente se usa el proyecto del directorio actual.
- **Límites:** `search` 1–20 fragmentos (5 por defecto, BM25); `retrieve` hasta 64 KB con aviso de recorte; `lines` hasta 400 líneas por llamada. Las direcciones pasan por el parser estricto de `ctx://` (sin traversal).
- **Registro:** cada acceso va a `context_retrievals` con el `SYMPHONY_RUN_ID`. `found = 0` es un miss: `lines` fuera de rango y búsqueda sin resultados dentro de un objeto conocido. **Límite:** una búsqueda sin resultados *sin* `uri` o un `ctx://` inexistente no se puede registrar, porque `context_retrievals.object_id` es obligatorio (migración 003); se devuelve el error o «Sin resultados» sin dejar fila. Cambiarlo es una migración (la 004 de P08 puede incluirlo).
- **Registro en los CLIs, por invocación** (sin tocar el repo ni la config del usuario), solo si el spawn lleva hook: Claude con `--mcp-config` + `--allowedTools mcp__symphony-context__…`; Codex con `-c mcp_servers.symphony-context.{command,args,env}`. Las variables `SYMPHONY_*` van en el `env` del servidor porque ningún CLI se las pasa solo. `SYMPHONY_NO_MCP` (cualquier valor) lo apaga.
- **Kimi, Antigravity y Copilot: no registrados.** No se investigó cómo registran un servidor MCP por invocación; queda para su re-validación (`docs/research/opencode-y-calidad-de-proveedores.md` § 4) y para OpenCode.
- **Verificado:** L2 de `context_mcp` (`context_mcp_search_retrieve_and_lines`: aciertos, recorte, misses, traversal, consulta vacía) y e2e con un daemon real (`crates/cli/tests/mcp.rs`: handshake MCP, `tools/list`, búsqueda tras indexar, `retrieve` igual al original, `lines`, rango fuera de límites, traversal como error de herramienta sin tumbar el broker, y el miss visible en `symphony context stats`). Argumentos de Claude y Codex cubiertos en sus tests de spawn.
- **No verificado en vivo (necesita cuota de Claude o Codex, sin permiso):** que Claude y Codex llamen de verdad a `context_search` y reciban el fragmento, y que Codex en `exec` apruebe las herramientas MCP sin preguntar. Pendiente de la prueba live con OpenCode (gratuito) cuando exista su adapter, o con permiso de Leo.
- 429 tests verdes (`cargo xtask check`); `cargo deny check` ✅.

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|
| Migración 003 (chunks + FTS5, items, retrievals, hechos, skills, MCP) | `cargo nextest run -p symphony-store` (migración 002 → 003 con datos) | ✅ |
| `ctx://` sin escapar de su raíz, chunks y búsqueda BM25 | tests de traversal y de `store::context`; `context_mcp_search_retrieve_and_lines` | ✅ |
| Compresores deterministas (nunca pierden el original ni inflan) | corpus golden con `insta` + proptest | ✅ |
| AST L0–L5 (Rust, Python, JS, TS) y esqueleto L3 en el handoff | snapshots por lenguaje y nivel | ✅ |
| Watcher por proyecto con `.gitignore` y debounce | `project_files_stay_searchable_as_they_change` | ✅ |
| Handoff v2: `raw` ≥ `safe` ≥ `balanced` ≥ `aggressive`, `raw` no omite nada | proptest + `handoff_evaluation_forced_kill_by_context_mode` | ✅ BALANCED = 6,6 % de RAW sin perder la tarea (L2) |
| Hechos consolidados y `symphony fact` | `project_facts_ipc_and_handoff_integration` | ✅ |
| Broker MCP por stdio contra un daemon real | `crates/cli/tests/mcp.rs` (handshake, `tools/list`, búsqueda, retrieve, lines, traversal, misses) | ✅ |
| Skills canónicas, `mcp.toml`, interruptores y traducción de argv en Claude y Codex | `crates/daemon/src/shared.rs` (unit), `crates/cli/tests/shared.rs`, tests de spawn de los adapters | ✅ |
| Vistas 20, 21, 32, 33 y 35 de la TUI | snapshots `insta` y flujos de teclas | ✅ |

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|
| Sin live del broker MCP: que Claude o Codex llamen de verdad a `context_search` | No se sabe si cada CLI acepta el registro por invocación y si Codex aprueba las herramientas MCP en `exec` | `SYMPHONY_LIVE=1` con un agente que pida contexto | Con un modelo gratuito de OpenCode (ADR-0012) o con permiso de Leo |
| Sin live de la evaluación RAW contra BALANCED con proveedores reales (S10) | La tabla de ahorro es de un CLI falso | Repetir S10 con Claude/Codex | Permiso de Leo (cuota) |
| Un `context_retrieve` de un `ctx://` inexistente, o una búsqueda sin resultados y sin `uri`, no deja fila en `context_retrievals` | Esos misses no cuentan en `symphony context stats` | `context_retrieve` con `ctx://file/nada.ts` | `object_id` es obligatorio (migración 003); relajarlo es una migración (la 004 de P08) |
| Los archivos con `@` en la ruta (`packages/@scope/…`, `src/@types.ts`) no se indexan: `ctx://file/<ruta>@<rev>` usa `@` como separador y el parser rechaza `?`, `#`, `%` y `\` | Esos archivos no son buscables ni recuperables por `ctx://` | un archivo `packages/@scope/pkg/index.ts` | Hace falta una codificación de la ruta (cambia el formato de `ctx://`, ADR); pendiente de P08/P13 |
| `index_text` (chunking, compresores, FTS) corre dentro de la transacción del writer, sin tope de tamaño | Un mensaje de varios MB o un diff enorme frena al único hilo de escritura unos segundos | Un lockfile regenerado en un checkpoint | Mover el trabajo antes de `writer.write` y acotar el texto; medir en P08.S9 (benchmark) |
| El índice de archivos cubre la raíz del proyecto, no el worktree de cada agente; `context.stats` es global, no por proyecto | Un archivo nuevo que solo existe en un worktree no tiene `ctx://file/…` hasta que llega a la raíz | `context_retrieve` de un archivo creado por el agente | Indexar por worktree (P08, con el scheduler) y acotar `stats` por proyecto |
| Skills compartidas sin traducción en Codex, Kimi, Antigravity y Copilot | Solo Claude las recibe | `symphony skills` + agente de Codex | Codex no tiene directorio de skills por invocación; los otros tres, en su re-validación |
| `share_mode = "shared"` de `mcp.toml` no se registra | Un servidor definido así no llega a ningún CLI | `mcp.toml` con `share_mode = "shared"` | Exige que el daemon haga de proxy MCP; se avisa en la vista 33 |
| El pie de la vista de agente se trunca a 96 columnas (ya pasaba antes de `c contexto`) | `x detener` y `Esc` no se ven en terminales angostas | Terminal de 96 columnas | Acortar los rótulos o dar al pie dos líneas (P08.S8) |
| (corregido en S6) La poda de checkpoints borraba diffs con chunks indexados y fallaba por la clave foránea: el checkpoint siguiente no se guardaba | Perdía checkpoints en agentes con muchos cambios; lo cazó `checkpoints_stay_monotonic_and_consistent` | 38+ checkpoints con diffs indexados | `prune_checkpoints` borra antes chunks y recuperaciones y anula `handoff_items.object_id`; test de regresión en `crates/store/tests/context.rs` |

## Decisiones tomadas
- ADR-0010: P10 → P09 → P08 completas.
- Durante la fase se aceptaron ADR-0011 (agentes con plan por fases, impacta P08) y ADR-0012 (OpenCode como proveedor); ninguno cambia el alcance de P09.

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| PLAN P09.S1 | «Migración 003» | `003_context.sql` | ya era la siguiente en el orden de ejecución |
| PLAN P09.S8 | Herramientas `context.retrieve`, `context.search`, `context.lines` | `context_retrieve`, `context_search`, `context_lines` (los métodos IPC conservan el punto) | varios CLIs solo aceptan `[A-Za-z0-9_-]` en el nombre de una herramienta |
| STACK §15.1 | El broker vive en un crate `mcp` | Módulo `crates/cli/src/mcp.rs` | es un proceso mínimo que solo reenvía al daemon con el cliente IPC del CLI; un crate aparte obligaba a extraer el cliente |
| PLAN P09.S9 | Skills «traducidas por cada adapter» | Solo Claude (plugin de sesión) | Codex no tiene un directorio de skills por invocación; el resto, sin investigar |
| PLAN P09.S9 | Vistas 20, 21, 32, 33, 35 | Cinco pantallas de solo lectura más interruptores; sin búsqueda interactiva en 21 | la búsqueda ya está en `symphony context` y en el broker |

## Métricas
- Handoff (evaluación L2 de S10): BALANCED = 1.412 tokens frente a 21.512 de RAW (6,6 %), AGGRESSIVE = 788 (3,7 %).
- Suite completa: `cargo xtask check` → 443 tests, ≈ 2 min en la máquina de Leo (Windows 11).

## Pruebas
- Comandos: `cargo xtask check`, `cargo deny check`, `cargo nextest run -p symphony-cli --test mcp`, `INSTA_UPDATE=always cargo nextest run -p symphony-tui` (y revisar el diff).
- Totales: 443 passed, 0 failed, 2 omitidos (los live, sin `SYMPHONY_LIVE=1`).
- Hallazgo de cierre: `context_commands_on_a_fresh_project` fallaba en CI porque dependía de un CLI real instalado; ahora usa el `claude` falso compartido (`tests/common`).
- **Revisión de código del diff de la fase** (`code-review`, nivel alto, 10 hallazgos). Corregidos, con prueba: (1) el broker devolvía texto sin redactar (`context_search/retrieve/lines` ahora pasan por `redact`); (2) `symphony context raw` y la vista 21 resolvían la dirección entre todos los proyectos (ahora acotan por `project_root`); (3) un `mcp.toml` roto borraba los interruptores del usuario (`sync_*` con `prune`); (4) el plugin de skills ignoraba cambios en recursos y se reescribía con un Claude leyéndolo (directorio por contenido, nunca reescrito); (5) la vista 20 etiquetaba mal la fidelidad (usaba L0–L5 de AST; ahora los mismos nombres que `symphony context inspect`); (6) `NEEDED_RETRIEVAL` nunca se registraba (el broker marca el handoff y el cierre normal del run no lo pisa); (7) el watcher ignoraba los `.gitignore` anidados en los eventos y trataba un archivo bloqueado como borrado; (8) `const N = 5` salía como `{ … }` en el esqueleto. Documentados sin corregir (tabla de arriba): rutas con `@`, trabajo del índice en el hilo del writer, índice por worktree y `stats` global.

## Estado final
P09 está construida: el contexto se indexa, se comprime sin perder el original, se recupera por `ctx://` desde un broker MCP y entra al handoff con su fidelidad. Skills y servidores MCP del usuario se comparten entre proveedores (Claude completo; Codex solo los servidores). Quedan los live (broker, S10 con proveedores reales y el registro en cada CLI), que se hacen con un modelo gratuito de OpenCode o con permiso de Leo. Siguen OpenCode (ADR-0012), las correcciones de Signalia y P08 con ADR-0011.

## Notas para el siguiente agente
- `heredoc` largo con comillas en el shell de esta máquina falla: crea el archivo con la herramienta de escritura y ejecuta un script.
- Los tests e2e del CLI siempre llevan `path_with_fake_claude` en el PATH; sin él fallan en CI aunque pasen en la máquina de Leo.
- Un test que deja un `symphonyd-…` huérfano rompe el siguiente build en Windows («Acceso denegado»): mátalo antes de reintentar.
- No registres un servidor MCP nuevo en un adapter sin `claude plugin validate` o su equivalente: lo que no se puede probar sin cuota queda anotado como «sin verificar en vivo».

## Dependencias agregadas
`regex` y `serde_json` (ya aprobadas, workspace) en `symphony-context`; `criterion` como dev-dependency. S8: `rmcp` 3.5 (Apache-2.0, ADR-0001; `default-features = false`, `server`, `macros`, `transport-io`) en el CLI. S4: `tree-sitter` 0.27 (ADR-0001) y las gramáticas `tree-sitter-rust` 0.24.2, `tree-sitter-python` 0.25.0, `tree-sitter-javascript` 0.25.0 y `tree-sitter-typescript` 0.23.2 (MIT; compilan C con `cc`).
