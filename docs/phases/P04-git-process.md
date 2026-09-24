# P04 · Git, worktrees y procesos

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p04-git-process |
| Inicio / cierre | 2026-09-24 / — |
| Agentes que trabajaron | claude-code/opus-5.5 |
| Tag | — |
| Docs usados | STACK §7, §12, §48, §60; IDEA §5.9; DB §3.C, §3.F; ADR-0002, ADR-0005 |

## Pasos

### P04.S1 · Crate `git` — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `symphony-git` sobre el `git` del sistema, con formatos pensados para máquinas: `worktree add -b / list --porcelain -z / remove [--force] / prune`, `delete_branch`, `status --porcelain=v2 -z` (tipos 1, 2 con rename, u, ?, !), `diff --no-ext-diff` y `--numstat -z -M` (binarios y renames), `head_commit`, `current_branch`, `agent_branch()` = `symphony/<sesión>/agent-NNN`, y `merge_preflight` con `merge-tree --write-tree` (sin tocar ningún worktree). Todas las llamadas con `GIT_TERMINAL_PROMPT=0`, `LC_ALL=C`, sin color, con `core.quotepath=false`.
- **Archivos clave:** `crates/git/src/lib.rs`, `crates/git/tests/git.rs`
- **Cómo se verificó:** 6 tests con repos temporales en `repo con espacios/`, worktrees en `worktrees con espacios/`, archivos `archivo ñandú.txt` y `renombrado con espacio.js`, sesión `sesión-1`: ciclo de worktrees (incluido el remove que falla sin force y el prune de uno borrado a mano), status v2, diff/numstat con binario y rename, preflight limpio y con conflicto. `cargo xtask check` → 80 passed.
- **Hallazgo:** `merge-tree` devuelve exit 1 tanto con conflictos como con una rama inexistente; se distinguen por el OID del tree.

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
