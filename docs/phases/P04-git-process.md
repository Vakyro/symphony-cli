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

### P04.S2 · Estrategia de dependencias — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `symphony_git::deps`: `detect` por lockfile (orden de STACK §12.2); `plan(worktree, base)` → `PNPM_STORE` (`pnpm install --frozen-lockfile --prefer-offline`, store del usuario sin tocar su config), `LINK` (npm/yarn/bun con el lockfile idéntico al del base y `node_modules` presente), `INSTALL` (devuelve el comando; lo corre el scheduler como clase 4) o `NONE` (cargo, uv, poetry); `lock_hash` BLAKE3 para `worktrees.deps_lock_hash`; `link_node_modules` (symlink en Unix, junction con `mklink /J` en Windows, sin privilegios); `run_command`.
- **Bug grave encontrado por un test y corregido:** en Windows, `git worktree remove --force` **seguía la junction y borraba el `node_modules` del repo base**. `Repo::worktree_remove` ahora suelta los enlaces del primer nivel del worktree antes de llamar a git (arreglo en la función que usan todos los que quitan worktrees).
- **Archivos clave:** `crates/git/src/deps.rs`, `crates/git/src/lib.rs` (`unlink_top_level_links`), `crates/git/tests/deps.rs`
- **Cómo se verificó:** detección, planes por manager, enlace + borrado del worktree sin tocar el base (con rutas con espacios); `cargo xtask check` → 83 passed. **Medición (test ignorado, con pnpm y red):** con el store ya caliente por el repo base, cada worktree nuevo instala en **1.68 s y 1.54 s** (Test A de P01 midió 24.3 s con npm sin store).

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
