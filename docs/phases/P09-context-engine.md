# P09 · Context Engine

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p09-context-engine |
| Inicio / cierre | 2026-10-02 / — |
| Agentes que trabajaron | claude-code/sonnet-5.5 |
| Tag | p09-done (pendiente) |
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
`context::compress`: `LogCollapser` (ANSI y líneas `` fuera, repeticiones «×N», recorte del medio; las líneas de error/panic/Traceback **siempre** se conservan), `TestSummaryCompressor` (cargo, vitest/jest, pytest → «896 pruebas: 895 pasaron, 1 falló» + nombres de fallos), `JsonStructuralCompressor` (esquema, conteos, rangos, ejemplos, campos opcionales marcados «solo en N de M»), `Deduplicator` (bloques/líneas largas repetidos → «igual a la línea N»), `GitDiffReducer` (lista de archivos con +/−, lockfiles y generados en una línea, hunks largos recortados). `compress()` elige por contenido (o por `Hint`) y **nunca infla**. El daemon guarda la versión comprimida junto al objeto (`set_compression`, desde 2 KB) y el original sigue en su blob. Etiquetas en DB: TestSummary→`LOG_COLLAPSE`, GitDiff→`DEDUP` (el CHECK solo admite cinco). Verificado: unit con corpus sintético por formato, proptest «no entra en pánico y no infla», proptest «la línea de error sobrevive al colapso», bench 1 MB de log ≈ 30 ms.
Desvío: el corpus dorado se genera en los tests en vez de `fixtures/context/` + snapshots (más corto de mantener y las aserciones son más precisas que un snapshot de texto comprimido).

### P09.S6 · Handoff assembler v2 (hecho)
`handoff::assemble` ahora devuelve, además del prompt, los **items** (sección, archivo o `ctx://`, fidelidad 0–5, tokens). Fidelidad: 5 completo · 4 recortado por tamaño · 3 resumen determinista · 2 esqueleto (AST, S4) · 1 solo referencia · 0 omitido. Cambios de comportamiento: en `BALANCED` y `AGGRESSIVE` el diff pasa primero por `GitDiffReducer` (archivos con +/−, lockfiles y generados en una línea, hunks largos recortados) y solo se usa si, con su aviso «el original completo está en ctx://…», es más corto que el original; `RAW` y `SAFE` no resumen nada. Lo comprimido se calcula en el momento: el original nunca se toca. Los archivos nuevos y la conversación **no** se comprimen (colapsar líneas parecidas de código o de chat perdería contenido): solo se recortan, como en v1.
Persistencia: `handoff_items` se llena en cada cambio de executor (`repo::insert_handoff` con `NewHandoffItem`; id `HandoffItemId`). Comandos: `symphony context inspect <agente>` (tabla de secciones con fidelidad), `context stats` (compresión por compresor, coste de handoffs por modo, recuperaciones y misses) y `context raw <ctx://…>` (el original; la dirección se valida con `CtxUri` y se busca en la base, nunca se abre una ruta). Protocolo: `context.inspect`, `context.stats`, `context.raw`.
Verificado: `raw_omits_nothing_and_modes_are_monotonic_with_a_realistic_checkpoint` (raw ≥ safe ≥ balanced ≥ aggressive en tokens con un diff de 3.000 líneas de lockfile, 60 mensajes y 6 archivos nuevos; RAW sin un solo recorte y todo a fidelidad 5), `balanced_reduces_the_diff_and_points_to_the_original`, snapshots v1 sin cambios (un diff pequeño no se resume), L2 del daemon (`handoff_items` tras un switch), CLI de punta a punta contra un daemon real.
Desvío: `/context inspect|stats|raw` del PLAN son subcomandos `symphony context …` (la TUI no tiene todavía un punto de entrada de comandos con barra).

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|
| (corregido en S6) La poda de checkpoints borraba diffs con chunks indexados y fallaba por la clave foránea: el checkpoint siguiente no se guardaba | Perdía checkpoints en agentes con muchos cambios; lo cazó `checkpoints_stay_monotonic_and_consistent` | 38+ checkpoints con diffs indexados | `prune_checkpoints` borra antes chunks y recuperaciones y anula `handoff_items.object_id`; test de regresión en `crates/store/tests/context.rs` |

## Decisiones tomadas
- ADR-0010: P10 → P09 → P08 completas.

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| PLAN P09.S1 | «Migración 003» | `003_context.sql` | ya era la siguiente en el orden de ejecución |

## Dependencias agregadas
`regex` y `serde_json` (ya aprobadas, workspace) en `symphony-context`; `criterion` como dev-dependency.
