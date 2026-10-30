# Introducción

**Symphony** es un runtime local en Rust que coordina varios CLIs de IA (**Claude Code**, **Codex**, **Kimi Code**, **Antigravity**, **GitHub Copilot CLI** y **OpenCode**) desde una sola terminal.

Se compone de dos binarios que se instalan juntos de forma global:
- `symphony`: la interfaz de línea de comandos y la TUI (construida con `ratatui`).
- `symphonyd`: el daemon local por proyecto, que administra la base de datos SQLite, los worktrees de Git, el supervisor de procesos, el bus de eventos, los checkpoints y el motor de contexto. `symphony` arranca `symphonyd` automáticamente cuando hace falta.

## Principios de diseño

1. **El agente no es el modelo (`AGENT ≠ MODEL`, ADR-0004).** La tarea, el worktree y el historial pertenecen al agente; el modelo o proveedor es un ejecutor reemplazable. Si un proveedor agota su cuota o prefieres otro modelo a mitad de camino, Symphony detiene el proceso, genera un checkpoint incremental apoyado en el estado vivo de Git (`git diff` y archivos del worktree) y arranca el nuevo proveedor en el mismo worktree sin que tengas que volver a explicar la tarea.
2. **El estado no vive en la memoria del modelo (`STATE ≠ MODEL MEMORY`).** La fuente de verdad vive en Symphony (SQLite + Git + almacén de objetos comprimidos).
3. **Estado y daemon aislados por proyecto (ADR-0018).** Cada proyecto tiene su propio directorio `<proyecto>/.symphony/` y su propio daemon. Las sesiones, oleadas, agentes y chats de un proyecto no se mezclan con los de otro.
4. **Una sola instalación global.** No hay que instalar ni copiar archivos de Symphony dentro de cada repositorio. Los hooks, las skills compartidas y el broker MCP se inyectan en cada invocación del CLI del proveedor.
5. **Reglas deterministas antes que ML (ADR-0016, ADR-0017).** El router de modelos, el planificador de operaciones pesadas, los compresores de contexto y los clasificadores de errores son deterministas, auditables y sin llamadas de red ocultas ni telemetría.

## Modos de trabajo

### Chat general con continuidad entre proveedores (ADR-0006)

Al ejecutar `symphony` dentro de un proyecto, la vista inicial es el **chat general**. El chat es un agente persistente del proyecto que trabaja sobre su propia rama `symphony/chat` (basada en `main`, como rama previa a `main`):
- Al terminar cada turno con cambios, Symphony hace un commit automático en `symphony/chat`.
- Puedes cambiar de proveedor o de modelo entre turnos de forma manual, por agotamiento de cuota (failover automático) o por umbral de tokens configurado (`[chat] switch_at_tokens`).
- Tu rama `main` no se toca hasta que pides explícitamente integrar el chat con `symphony chat merge` (que incluye preflight y revisión opcional por un *review agent*).
- En el harness de Symphony, los agentes pueden colorear partes de sus respuestas con `[[symphony:title]]…[[/symphony:title]]`, `[[symphony:bullet]]…[[/symphony:bullet]]`, `[[symphony:highlight]]…[[/symphony:highlight]]`, `[[symphony:warning]]…[[/symphony:warning]]`, `[[symphony:error]]…[[/symphony:error]]` y `[[symphony:table]]…[[/symphony:table]]`. Úsalos solo cuando ayuden a distinguir títulos, puntos, destacados, avisos, errores o tablas; el texto normal queda sin marcar. La TUI conserva los marcadores en el transcript y colorea solo los spans válidos (ADR-0023).

### Agentes de tareas en paralelo y planes por fases (ADR-0011, ADR-0013)

Puedes lanzar uno o varios agentes autónomos (`symphony spawn` o tecla `n` en la TUI):
- Cada agente trabaja en su propio `git worktree` aislado bajo `<proyecto>/.symphony/worktrees/`.
- Las ramas de los agentes nacen de `symphony/chat` y se integran de vuelta en ella (`main ← symphony/chat ← agentes`).
- Con `[plan] enabled = true` (activo por defecto), el agente escribe primero un plan por fases y tareas en `.symphony/plan/<task_id>.md`. Cada fase se ejecuta en una sesión nueva del proveedor (equivalente a limpiar el contexto con un handoff en modo `balanced`), evitando acumular historiales largos que agoten la cuota.
- El **scheduler** limita las operaciones pesadas simultáneas (builds, suites de tests) mediante hooks (`PreToolUse`) y límites del sistema operativo (Job Objects en Windows, cgroups v2 en Linux y grupos de procesos en macOS) para que la máquina siga fluida.

### Harness unificado y trabajo más allá del código (ADR-0019, ADR-0020)

- **Harness de Symphony (`[harness] mode = "symphony"`):** todos los modelos compatibles (Claude Code, Codex, OpenCode, Copilot y Kimi) reciben el mismo conjunto de herramientas servidas por `symphony mcp serve` (`read`, `write`, `edit`, `glob`, `grep`, `shell` y `context_*`), apagando las herramientas dispares de cada CLI.
- **Proyectos de cualquier tipo (`symphony init --kind`):** además de código (`code`), una carpeta puede inicializarse para documentos (`document`), contenido multimedia (`media`) o propósito general (`general`), ajustando los profiles por defecto (`@write`, `@media`, `@research`).

## Referencia rápida de comandos (`symphony --help`)

Ejecutar `symphony` sin subcomandos abre la TUI en el proyecto actual. Para operar desde la terminal sin TUI, estos son los subcomandos disponibles:

| Comando | Qué hace |
|---|---|
| `symphony status` | Muestra el estado del daemon del proyecto (lo arranca si no está vivo). |
| `symphony daemon start \| stop \| status` | Arranca, detiene o consulta el daemon `symphonyd`. |
| `symphony migrate split` | Reparte una base global anterior a P15.5 (`~/.symphony/symphony.db`) en cada `<proyecto>/.symphony/`. |
| `symphony init [--kind <kind>] [--name <name>] [--git-init]` | Prepara la carpeta actual como proyecto de Symphony (crea `.symphony/project.toml`). |
| `symphony providers [refresh]` | Lista los CLIs de IA detectados, su salud y su cuota (o vuelve a detectarlos con `refresh`). |
| `symphony usage [--days <días>]` | Muestra el uso de tokens por proveedor y modelo (informado frente a estimado; 7 días por defecto). |
| `symphony spawn <task> [opciones]` | Crea un agente nuevo con su tarea y worktree aislado (`--model`, `--profile`, `--failover`, `--context-mode`, `--priority`, `--description`, `--cwd`). |
| `symphony agents [--all]` | Lista los agentes del proyecto (`--all` incluye completados y cancelados). |
| `symphony send <agent> <message>` | Envía un mensaje al agente (en turno activo o retomando la sesión si ya terminó). |
| `symphony pause <agent>` / `symphony resume <agent>` | Pausa (suspende) o reanuda el ejecutor de un agente. |
| `symphony stop <agent>` / `symphony kill <agent>` | Detiene ordenadamente o termina forzosamente el ejecutor y cancela el agente. |
| `symphony switch <agent> [--model <m> \| <target_model> \| --profile <p>]` | Cambia el modelo o profile del agente y reinicia el ejecutor con handoff. |
| `symphony explain-route <agent>` | Explica por qué se eligió el modelo de un agente y por qué se descartaron otros candidatos. |
| `symphony diff <agent>` | Muestra el `git diff` del worktree del agente contra su commit base. |
| `symphony logs <agent> [--limit <n>]` | Muestra el historial de mensajes y conversación del agente. |
| `symphony attach <agent> [--print]` | Abre la sesión del agente en su CLI oficial en una terminal nueva (o imprime el comando con `--print`). |
| `symphony inspect <agent>` | Inspecciona en detalle estado, tarea, worktree, checkpoints y runs del agente. |
| `symphony review <agent> [--model <m> \| --profile <p>]` | Pide a un *review agent* que revise el trabajo de un agente (solo recomienda; no integra). |
| `symphony chat merge [--review] [--yes] [--skip-review] [--validate] [--model <m>] [--profile <p>]` | Preflight, revisión e integración de la rama `symphony/chat` en `main`. |
| `symphony context inspect <agent> \| stats \| raw <uri>` | Inspecciona qué entró al último handoff, estadísticas de compresión o el contenido original de una URI `ctx://`. |
| `symphony fact set <key> <value> [--kind <k>] \| list [--all]` | Registra o lista hechos consolidados del proyecto (`ARCHITECTURE`, `DECISION`, `CONSTRAINT`, `CONVENTION`). |
| `symphony suggestion list \| switch <id> \| keep <id> \| mute <id>` | Gestiona las sugerencias de cambio de modelo (nunca se aplican solas). |
| `symphony plugin list \| check <name>` | Lista los plugins instalados en `~/.symphony/plugins` o verifica un plugin con `describe`. |
| `symphony skills [list \| enable <name> \| disable <name>]` | Lista, activa o desactiva skills compartidas (`~/.symphony/skills` y `.symphony/skills`). |
| `symphony mcp serve \| list \| enable <name> \| disable <name>` | Sirve el broker MCP por stdio o gestiona los servidores definidos en `mcp.toml`. |

## Atajos principales en la TUI

- **En Home / Dashboard:** `n` crear agente, `p` pantalla de proveedores, `r` centro de recuperación (Recovery Center), `T` cambiar tema, `:` barra de comandos, `q` salir.
- **En la vista de un agente:** `↑`/`↓` desplazar conversación, `d` ver `git diff`, `m` enviar mensaje, `a` abrir en el CLI nativo (*attach*), `s` cambiar modelo (*switch*), `?` explicar decisión de ruta (`explain-route`), `S`/`K`/`N` aceptar, mantener o silenciar una sugerencia de modelo.
- **En la vista Chat:** `Ctrl+G` preflight de integración a `main` (y segunda `Ctrl+G` seguida para confirmar el merge cuando la revisión está aprobada), `Ctrl+R` pedir revisión del chat a un *review agent*, `Ctrl+T`/`Ctrl+K`/`Ctrl+N` cambiar al modelo sugerido, mantener el actual o silenciar sugerencias.
