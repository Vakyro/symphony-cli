# P04 · Git, worktrees y procesos

| Campo | Valor |
|---|---|
| Estado | CERRADA |
| Rama | phase/p04-git-process |
| Inicio / cierre | 2026-09-24 / 2026-09-24 |
| Agentes que trabajaron | claude-code/opus-5.5 |
| Tag | p04-done |
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

### P04.S3 · Crate `process` — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `symphony-process` sobre ProcessKit (ADR-0002), sin PTY (ADR-0005). `spawn(ProcessSpec)` → `Supervised`: un `ProcessGroup` por proceso raíz; salida como `OutputLine::{Stdout,Stderr}` por un canal (la tarea de fondo siempre drena para que el hijo no se trabe); stdin abierto opcional (`write_stdin`, `close_stdin`); `terminate_tree`, `suspend`/`resume`, `stats` (ProcessKit + sysinfo como respaldo de memoria), `wait`/`wait_timeout`; `Guarantees { mechanism, limits_enforced, limits_note }`: si el SO no aplica los límites pedidos, el proceso arranca igual y lo informa. Ningún tipo de ProcessKit sale del crate.
- **Archivos clave:** `crates/process/src/lib.rs`, `crates/process/tests/process.rs`
- **Cómo se verificó:** 6 tests con `node`: árbol de 10 procesos → `terminate_tree` → **0 huérfanos**; soltar el handle mata el árbol; 2000 líneas en orden + stderr + exit code 3; eco por stdin a media ejecución (con ñ); cwd con espacios y env; programa inexistente → error; suspend/resume y mecanismo `JOB_OBJECT` en Windows. `cargo xtask check` → 89 passed; CI en 3 OS.

### P04.S4 · Saneamiento de ANSI — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `symphony_core::sanitize(input, AnsiMode::{Plain, KeepColors})`: máquina de estados tipo VTE, sin dependencias. Descarta OSC (título, OSC 52, hyperlinks OSC 8: queda el texto), DCS/SOS/PM/APC, todo CSI salvo SGR con parámetros numéricos válidos (y solo en `KeepColors`), ESC de un carácter, C1 de 8 bits (`U+009B` CSI, etc.) y controles C0 salvo `
`/`	`. `` se resuelve como una terminal (barras de progreso → último estado; CRLF → LF). Secuencias sin terminar no dejan nada.
- **Archivos clave:** `crates/core/src/ansi.rs`
- **Cómo se verificó:** 10 tests de ataques (cambio de título con BEL/ST/C1, OSC 52, borrado de pantalla, pantalla alternativa, reset, colores, hyperlinks, DCS/APC, backspace para esconder texto, CR, secuencias sin terminar, texto normal con ñ y emoji) + 3 proptest (salida plana sin controles, salida con color solo con SGR válidos, idempotencia). `cargo xtask check` → 102 passed.
- **Nota:** no se agregó una crate: STACK §58 no trae una de ANSI y el parser es chico.

### P04.S5 · `fake-agent` (testkit) — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** binario `fake-agent` en `symphony-testkit`, guiado por un guion TOML (`Script`/`Step`): `say`, `edit`, `run`, `wait_input`, `sleep`, `rate_limit`, `quota_exhausted`, `auth_error`, `crash`, `hang`. Emite JSONL al estilo de `claude -p --output-format stream-json` (`system/init`, `assistant`, `tool_use`, `tool_result`, `system/api_retry`, `user`, `error`, `result`), escribe transcript JSONL como Claude Code y llama al hook configurado con el payload JSON por stdin (campos comunes reales), respetando un `deny` de `PreToolUse`. Subcomando `record-hook` para usarlo de hook en tests.
- **Archivos clave:** `crates/testkit/src/{lib,script}.rs`, `crates/testkit/src/bin/fake-agent.rs`, `crates/testkit/tests/fake_agent.rs`
- **Cómo se verificó:** 8 tests, uno por escenario (turno normal con hooks y transcript, deny, 429 que sigue, cuota y auth con error tipado y exit 1, crash sin `result` ni `Stop`, cuelgue hasta kill, mensaje por stdin, guion inválido).
- **Nota:** `symphony hook emit` todavía no existe (P05.S3): el fake-agent llama a cualquier programa de hook, igual que un CLI real.

### P04.S6 · Integración — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `crates/testkit/tests/p04_integration.rs`: repo en `proyecto con espacios/` → worktree `agent-001` → `fake-agent` adentro con `symphony-process` → se lee el stream hasta el mensaje del asistente (el archivo editado aparece en `git status`) → el agente queda colgado → `terminate_tree` → borrar worktree y rama; el repo base queda intacto.
- **Cómo se verificó:** local y CI en 3 OS.

### P04.S7 · Cierre — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** revisión del diff (sin `unwrap`/`expect`/`todo!` fuera de tests; git sin prompts de credenciales; hashes y rutas validados; ANSI saneado). Merge a `main` y tag `p04-done`.


## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|
| Worktrees por agente (espacios y acentos en rutas) | `crates/git/tests` | ✅ 3 OS |
| Kill del árbol sin huérfanos | `crates/process/tests` (10 procesos) | ✅ 3 OS |
| Borrar un worktree no borra el `node_modules` enlazado | `crates/git/tests/deps.rs` | ✅ 3 OS |
| `fake-agent` con todos los escenarios | `crates/testkit/tests/fake_agent.rs` | ✅ 8 escenarios |
| Worktree + agente + stream + kill + limpieza | `p04_integration.rs` | ✅ 3 OS |

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|

## Decisiones tomadas
- ADR-NNNN: …

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| STACK §7.1, PLAN P04.S3 | Trait `ProcessSupervisor` | Tipo concreto (`spawn` → `Supervised`) | Una sola implementación: el trait se extrae cuando haya otra (plan B de STACK §60). La regla importante (ProcessKit no se filtra) se cumple |

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|
| processkit | 3.3 (`limits`, `stats`) | Grupos de procesos contenidos (ADR-0002) | Sí |
| sysinfo | 0.39.6 (`system`) | Memoria del árbol donde el SO no da stats | Sí |
| blake3 (en `git`) | 1.8 | `deps_lock_hash` | Sí |

## Métricas
- pnpm con store caliente: 1.68 s y 1.54 s por worktree nuevo (P01 Test A: 24.3 s con npm sin store).
(benchmarks, tiempos, RAM, cobertura — con comando)

## Pruebas
- Comando(s): `cargo xtask check`, `cargo deny check`, CI (3 OS + MSRV); medición pnpm con `--run-ignored only pnpm`
- Totales: 111 passed, 1 ignorado (pnpm con red)
- Totales: N passed, M failed (cuáles y por qué)

## Estado final
Aislamiento listo: worktrees por agente con git del sistema, dependencias sin reinstalar (pnpm store / enlace / instalación planificada), procesos contenidos por SO con kill del árbol sin huérfanos en los 3 OS, saneamiento de ANSI y un `fake-agent` con todos los escenarios para los E2E. Se encontró y corrigió un bug grave: en Windows, borrar un worktree seguía la junction de `node_modules` y borraba el del repo base.
Resumen de 3–5 líneas para Leo.

## Notas para el siguiente agente
Trampas, comandos útiles y lo que no vale la pena reintentar.
