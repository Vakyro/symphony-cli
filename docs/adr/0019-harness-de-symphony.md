# ADR-0019 · Harness de Symphony: la misma interfaz de ejecución para todos los modelos

- **Estado:** ACEPTADO
- **Fecha:** 2026-10-23
- **Autor:** agente (claude-code/opus-5.5) · **Aprobado por:** Leo (2026-10-23)
- **Fase/paso:** P15.5.S0 · actualiza ADR-0005

## Contexto
ADR-0005 eligió headless: cada CLI corre con sus herramientas nativas y sus propios permisos (Claude `acceptEdits`, Codex `workspace-write`, Kimi y OpenCode sin sandbox…). Resultado: los modelos tienen interfaces distintas, los sandboxes bloquean a unos y no a otros (Codex no podía correr pnpm ni leer los worktrees) y las funciones de una sesión normal (skills, MCP del usuario, subagentes) llegan desigual. Leo pidió que todos los modelos corran en una misma interfaz de ejecución, la de Symphony, y descartó dos lecturas: A (cada modelo en su CLI completo) y B (todos dentro de Claude Code).

## Opciones consideradas
1. **A · Cada modelo en su CLI sin recortes.** Interfaces distintas.
2. **B · Todos dentro de Claude Code con otros backends.** Solo viable con APIs compatibles; choca con los términos de servicio de Codex, Copilot y Antigravity.
3. **C · Harness de Symphony.** Un servidor MCP propio con todas las herramientas; las nativas de cada CLI se apagan.
4. **D · Loop propio llamando a las APIs de los modelos.** Obliga a usar API keys de pago; usar el OAuth de una suscripción fuera de su CLI oficial va contra sus términos.

## Decisión
Opción C (elegida por Leo). Opción D queda fuera de v1.

- **El broker crece.** `symphony mcp serve` (`crates/cli/src/mcp.rs`) pasa de las tres herramientas de contexto a: `read`, `write`, `edit`, `glob`, `grep`, `shell`, `skill`, `task` (subagente = otro agente de Symphony), las `context_*` actuales y los MCP del usuario (`mcp.toml`) reexpuestos.
- **Dueño de la ejecución: el daemon.** El broker solo reenvía por IPC (como hoy). El daemon aplica permisos, el sandbox de proyecto (el worktree del agente más las rutas extra de la config), el proceso con `crates/process`, el timeout y el registro en `tool_calls`. Así los permisos son los mismos para todos y no dependen del CLI.
- **Permisos por defecto: acceso local total** (decisión de Leo): sin sandbox de red ni de rutas. La config puede restringir por herramienta y por ruta; el cambio de defaults a más restrictivos sigue siendo ⛔.
- **Herramientas nativas apagadas por proveedor**, verificado en S4: Claude (`--allowedTools`/`--disallowedTools`), Codex (`-c` de tools y `sandbox_mode`), Copilot (`--deny-tool`), OpenCode (permisos en su config). Kimi y Antigravity se verifican en S4. Un proveedor que no permita apagarlas se marca **harness parcial** y se muestra así en `providers list`.
- **Prompt de sistema:** uno de Symphony, neutro (no habla de código), igual para todos; se añade al del CLI.
- **Medición obligatoria (S4):** tareas fijas por proveedor, harness nativo frente a harness de Symphony, con tokens y tasa de éxito. Si un modelo rinde claramente peor, se documenta y el router lo tiene en cuenta en su perfil (no se oculta).
- **Handoff:** el checkpoint registra las llamadas a herramientas del harness, que ahora son las mismas para cualquier modelo.

## Consecuencias
- Cambia ADR-0005 en la parte de herramientas y permisos; el modo headless, `resume` y `attach` siguen igual.
- `[providers.permissions]` pasa a ser `[harness]` (permisos de Symphony); los campos por proveedor se conservan solo como respaldo para proveedores con harness parcial.
- Se necesita un fixture por proveedor que confirme que las herramientas nativas están apagadas y que las MCP se llaman.
- Live: máximo 2 mensajes por corrida (STATUS § Permisos).
