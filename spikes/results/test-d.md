# Test D · Handoff forzado (P01.S6)

- **Fecha:** 2026-09-24 · **Agente:** claude-code/opus-5.5
- **Máquina:** Windows 11, i7-8650U, 16 GB (la de Test A).
- **CLIs:** Claude Code 2.1.281 con `--model sonnet --permission-mode acceptEdits`; codex-cli 0.154.0 con su modelo por defecto (`gpt-5.6-sol`) y `-s workspace-write`.
- **Repo:** Node ESM con `node:test`, un smoke test y un test lento de 8 s (para poder matar al agente durante la suite).
- **Checkpoint:** `spike-hook collect` actualiza `checkpoints/<agente>.json` **en cada evento de hook**: sesión, `cwd`, `transcript_path`, modelo, último comando y su resultado, plan estructurado (si existe), último mensaje del asistente (del hook `Stop` o de la cola del transcript) y `git status`. **Nada sale del modelo que muere.**
- **Handoff:** `spike-hook handoff <checkpoint.json> <objetivo.md>` arma el prompt del sucesor desde el checkpoint + el git vivo del worktree (diff y archivos nuevos). Prompts reales en [`test-d-handoffs/`](test-d-handoffs/).
- **Kill:** `taskkill /F /T` sobre el árbol, sin cleanup. Driver: [`spikes/scripts/test-d.ps1`](../scripts/test-d.ps1).

## Tareas y momento del kill

| Tarea | Qué pide | Kill |
|---|---|---|
| T1 · usuarios | email + password (scrypt) + `UserStore`, 3 archivos de test (la tarea de P00.S0) | Tras la 2.ª edición de archivo |
| T2 · Markdown | escape + inline + slug + `toHtml`, 4 archivos de test | **Durante `npm test`** (3 s después de que arranca la suite de ~8 s) |
| T3 · tareas | storage atómico + `TaskList` + `parseArgs`, 3 archivos de test | 45 s después de la primera edición, a mitad de trabajo |

## Resultados

| Corrida | A muere | A dejó | ¿Huérfanos? | B | ¿Continúa sin reexplicar? | ¿Rehace trabajo de A? | Tests al final | Tiempo de B |
|---|---|---|---|---|---|---|---|---|
| T1 · Claude → Codex | 26 s | `email.js` + test | 0 | Codex | ✅ "Retomo exactamente desde los dos archivos existentes" | No (2/2 idénticos) | ✅ 12/12 | 331 s ¹ |
| T1 · Codex → Claude | 105 s | `email.js` + test | 0 | Claude | ✅ | No (2/2 idénticos) | ✅ 24/24 | 61 s |
| T2 · Claude → Codex | 21 s, durante los tests | `escape.js` + test | 0 | Codex | ✅ "Retomo exactamente desde el worktree actual" | No (2/2 idénticos) | ✅ 13/13 | 4 261 s ² |
| T2 · Codex → Claude | 118 s, durante los tests | `escape.js` + test | 0 | Claude | ✅ | No (2/2 idénticos) | ✅ 12/12 | 49 s |
| T3 · Claude → Codex | 62 s | storage + tasks + 2 tests | 0 | Codex | ✅ "Retomo el estado existente, sin rehacerlo" | No (4/4 idénticos) | ✅ 13/13 | 230 s |
| T3 · Codex → Claude | 166 s | storage + test | 0 | Claude | ✅ | No (2/2 idénticos) | ✅ 10/10 | 50 s |

¹ Codex perdió tiempo con un `EPERM` del sandbox `workspace-write` de Windows al correr `node --test` (Node intenta leer el directorio padre del usuario).
² **Corte de red de la máquina** (DNS "Host desconocido") de unos 65 min en medio de la corrida. Codex esperó en silencio ("Reconnecting… 5/5", luego "waiting for network") y terminó bien cuando volvió la red. Sin el corte, habría tardado ~5 min.

Ningún sucesor borró ni debilitó tests. En los 6 casos, **todos los archivos que dejó A quedaron byte a byte idénticos**: B los revisó y construyó encima.

## Qué tenía cada checkpoint

| Corrida | Eventos | Plan estructurado | Último comando | Último mensaje de A |
|---|---|---|---|---|
| T1 · Claude | 8 | — | ✅ | — (A no había escrito texto todavía) |
| T1 · Codex | 12 | — | ✅ | ✅ |
| T2 · Claude | 9 | — | ✅ | ✅ (su lista de TODOs como texto) |
| T2 · Codex | 19 | — | ✅ | ✅ (su lista de TODOs como texto) |
| T3 · Claude | 16 | — | ✅ | ✅ |
| T3 · Codex | 33 | — | ✅ | ✅ |

**H1 confirmado a mayor escala:** ninguno de los dos CLIs usó TodoWrite / `update_plan` en modo headless, aunque la tarea lo pedía. El "qué seguía" sale del último mensaje del asistente, leído de la cola del transcript. Aun en T1-Claude, sin ningún mensaje, B continuó bien solo con el objetivo y git.

## Comparación con el Test D manual de P00.S0

| | P00.S0 (manual, 1 tarea) | P01.S6 (automatizado, 3 tareas) |
|---|---|---|
| Corridas | 2 | 6 |
| Checkpoint | Armado a mano desde el transcript al final | **Incremental, en cada hook**, antes de la muerte |
| Kill | Cerca de un límite de archivo | Tras una edición, **durante los tests** y a mitad de trabajo |
| Resultado | 2/2 | **6/6** |

## Conclusiones

1. **AGENT ≠ MODEL se sostiene** en los dos sentidos, con tres tareas y tres momentos de kill distintos, usando un checkpoint que se escribe antes de la muerte y no le pide nada al modelo que muere.
2. **Git es la parte esencial del checkpoint.** Plan y último mensaje ayudan, pero B termina bien aun sin ellos.
3. **Hace falta un watchdog de inactividad (P10):** un CLI sin red espera indefinidamente sin fallar. El stream de Codex lo delata (`Reconnecting… n/5`).
4. **El sandbox de Codex en Windows** (`workspace-write`) interfiere con `node --test` (`EPERM` al leer el directorio del usuario). Hay que documentarlo para P05 y ver si el adapter necesita `--add-dir` o permisos extra.

## Límites

- Tareas de tamaño medio (6–8 archivos), bien especificadas, en un repo pequeño.
- Una corrida por combinación.
- Solo headless. El handoff con el modo interactivo se decide en ADR-0005.
