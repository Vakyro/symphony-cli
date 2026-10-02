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

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|

## Decisiones tomadas
- ADR-0010: P10 → P09 → P08 completas.

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| PLAN P09.S1 | «Migración 003» | `003_context.sql` | ya era la siguiente en el orden de ejecución |

## Dependencias agregadas
Ninguna todavía.
