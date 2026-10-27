# Referencia de comandos

Referencia completa de los comandos y opciones de `symphony`, fiel a `crates/cli/src/main.rs` (`symphony --help` y `symphony <subcomando> --help`).

Ejecutar `symphony` sin argumentos en una terminal interactiva abre la **TUI** en el proyecto actual (y arranca `symphonyd` automáticamente si no está corriendo).

---

## 1. Proyecto, daemon y migración

### `symphony init`
Prepara la carpeta actual como proyecto de Symphony creando `.symphony/project.toml`.

```bash
symphony init [--kind <KIND>] [--name <NAME>] [--git-init]
```
- `--kind <KIND>`: Tipo de trabajo del proyecto: `code` (por defecto), `document`, `media` o `general`.
- `--name <NAME>`: Nombre del proyecto (por defecto, el nombre del directorio actual).
- `--git-init`: Si la carpeta no es un repositorio Git, inicializa uno con un commit vacío (los worktrees lo necesitan).

### `symphony status`
Muestra el estado del daemon del proyecto actual (lo arranca si no está vivo).

```bash
symphony status
```

### `symphony daemon`
Controla el proceso `symphonyd` del proyecto actual.

```bash
symphony daemon start    # Arranca el daemon si no está corriendo
symphony daemon stop     # Detiene el daemon
symphony daemon status   # Dice si el daemon está corriendo, sin arrancarlo
```

### `symphony migrate split`
Migra una instalación anterior a P15.5 (una sola base global en `~/.symphony/symphony.db`) repartiendo cada proyecto en su propio `<proyecto>/.symphony/`. Detiene el daemon global antiguo y no borra la base original.

```bash
symphony migrate split
```

---

## 2. Proveedores, uso y enrutamiento

### `symphony providers`
Lista los CLIs de IA detectados, versión, autenticación, salud, cuota y soporte del harness.

```bash
symphony providers           # Muestra los proveedores detectados
symphony providers refresh   # Vuelve a detectar los CLIs instalados y sus modelos
```

### `symphony usage`
Muestra el uso de tokens por proveedor y modelo (lo que informa cada CLI frente a lo que se estima).

```bash
symphony usage [--days <DAYS>]
```
- `--days <DAYS>`: Cuántos días hacia atrás consultar (por defecto `7`).

### `symphony explain-route`
Explica por qué el router eligió el modelo de un agente, el puntaje de los candidatos y el motivo de descarte de los no elegibles.

```bash
symphony explain-route <AGENT>
```

### `symphony suggestion`
Gestiona las sugerencias de cambio de modelo emitidas cuando cambia el tipo de tarea de un agente (nunca se aplican solas).

```bash
symphony suggestion list          # Lista las sugerencias pendientes del proyecto
symphony suggestion switch <ID>   # Cambia al modelo sugerido
symphony suggestion keep <ID>     # Sigue con el modelo actual y descarta la sugerencia
symphony suggestion mute <ID>     # Descarta la sugerencia y silencia futuras para ese agente
```

---

## 3. Ciclo de vida de agentes

En todos los comandos que reciben `<AGENT>`, puedes pasar el número corto (`1`, `#1`) o el ULID completo del agente.

### `symphony spawn`
Crea un agente nuevo con su tarea y `git worktree` aislado.

```bash
symphony spawn <TASK> [opciones]
```
- `<TASK>`: Tarea u objetivo del agente.
- `-m, --model <MODEL>`: Modelo canónico exacto (ej. `claude/sonnet`, `openai/gpt-6-luna`).
- `--profile <PROFILE>`: Profile del router (`@code`, `@debug`, `@fast`, `@reasoning`, `@docs`, `@review`, `@conserve`, `@write`, `@media`, `@research`).
- `--failover <FAILOVER>`: Política de failover (`any`, `same-provider`, `none`; por defecto `any`).
- `--context-mode <CONTEXT_MODE>`: Modo de compresión de contexto en handoffs (`raw`, `safe`, `balanced`, `aggressive`; por defecto `balanced`).
- `--priority <PRIORITY>`: Prioridad entera en la cola (por defecto `0`).
- `-d, --description <DESCRIPTION>`: Descripción adicional o contexto de la tarea.
- `--cwd <CWD>`: Directorio raíz del proyecto (por defecto, el directorio actual).

Ejemplos:
```bash
symphony spawn "Agregar validación de email con tests" --profile @code
symphony spawn "Corregir bug de concurrencia" -m claude/opus --failover same-provider -d "Revisa src/pool.rs"
```

### `symphony agents`
Lista los agentes del proyecto.

```bash
symphony agents        # Solo agentes activos o recientes
symphony agents --all  # Incluye completados y cancelados (-a / --all)
```

### `symphony send`
Envía un mensaje al agente (por `stdin` si el turno lo admite en vivo, o retomando la sesión del CLI con `resume` si el turno ya terminó).

```bash
symphony send <AGENT> <MESSAGE>
```

### `symphony pause` / `symphony resume`
Pausa (suspende el árbol de procesos del ejecutor) o reanuda un agente pausado.

```bash
symphony pause 1
symphony resume 1
```

### `symphony stop` / `symphony kill`
Detiene ordenadamente o termina forzosamente el ejecutor y cancela el agente (conservando su worktree y último checkpoint).

```bash
symphony stop 1
symphony kill 1
```

### `symphony switch`
Detiene el ejecutor actual, genera un checkpoint con el handoff incremental y arranca otro modelo o profile en el mismo worktree.

```bash
symphony switch <AGENT> <TARGET_MODEL>
symphony switch <AGENT> --model <MODEL>
symphony switch <AGENT> --profile <PROFILE>
```
- `<TARGET_MODEL>` o `-m, --model <MODEL>`: Modelo exacto de destino (ej. `openai/gpt-6-luna`).
- `-p, --profile <PROFILE>`: Profile para que el router elija entre los modelos disponibles (incompatible con `--model` y `<TARGET_MODEL>`).

---

## 4. Inspección, revisión e integración

### `symphony inspect`, `symphony diff`, `symphony logs`, `symphony attach`

```bash
symphony inspect 1              # Estado, tarea, worktree, checkpoints y runs del agente
symphony diff 1                 # git diff del worktree del agente contra su commit base
symphony logs 1 --limit 50      # Historial de mensajes (-l / --limit opcional)
symphony attach 1               # Abre la sesión nativa del CLI en una terminal nueva
symphony attach 1 --print       # Solo imprime el comando nativo sin abrir otra ventana
```

### `symphony review`
Pide a un *review agent* que revise el trabajo de un agente (emite un veredicto y hallazgos; nunca integra por su cuenta).

```bash
symphony review <AGENT> [--model <MODEL> | --profile <PROFILE>]
```
- `--model <MODEL>`: Modelo exacto del revisor.
- `--profile <PROFILE>`: Profile del revisor (por defecto `@review`).

### `symphony chat merge`
Gestiona el preflight, la revisión y la integración (`git merge --no-ff`) de la rama `symphony/chat` en `main`.

```bash
symphony chat merge                                   # Preflight rápido (commits, conflictos, estado)
symphony chat merge --validate                        # Preflight corriendo además la validación del proyecto
symphony chat merge --review [--model <M> | --profile <P>] # Lanza un review agent sobre la rama del chat
symphony chat merge --yes                             # Integra en main (exige preflight limpio y revisión aprobada)
symphony chat merge --yes --skip-review               # Integra en main aunque no haya revisión o haya pedido cambios
```

---

## 5. Motor de contexto, hechos, plugins, skills y MCP

### `symphony context`
Inspecciona el motor de contexto y los paquetes de handoff (`crates/context`).

```bash
symphony context inspect <AGENT>           # Qué items entraron al último handoff y en qué nivel
symphony context stats                     # Estadísticas de compresión, ahorros y recuperaciones
symphony context raw ctx://file/src/lib.rs # Muestra el contenido original completo de una URI ctx://
```

### `symphony fact`
Gestiona los hechos consolidados del proyecto (`project_facts`) que se incluyen en los handoffs.

```bash
symphony fact set <KEY> <VALUE> [--kind <KIND>]
symphony fact list [--all]
```
- `-k, --kind <KIND>`: `ARCHITECTURE`, `DECISION` (por defecto), `CONSTRAINT` o `CONVENTION`.
- `-a, --all`: Incluye hechos reemplazados (`SUPERSEDED`) y en conflicto (`CONFLICT`).

### `symphony plugin`
Descubre y comprueba los plugins instalados en `~/.symphony/plugins/`.

```bash
symphony plugin list          # Lista plugins instalados, cuáles activa el proyecto y cuáles tienen errores
symphony plugin check <NAME>  # Ejecuta el plugin con `describe` y comprueba su respuesta
```

### `symphony skills`
Lista, enciende o apaga skills compartidas en `~/.symphony/skills/` y `.symphony/skills/`.

```bash
symphony skills                  # Equivalente a `symphony skills list`
symphony skills list
symphony skills enable <NAME>
symphony skills disable <NAME>
```

### `symphony mcp`
Gestiona los servidores definidos en `mcp.toml` y sirve el broker MCP por stdio.

```bash
symphony mcp list
symphony mcp enable <NAME>
symphony mcp disable <NAME>
symphony mcp serve               # Servidor MCP stdio (lo inyectan los adapters automáticamente)
```

> Uso interno: `symphony hook emit` (oculto en `--help`) es invocado por los hooks de los CLIs para enviar eventos por `stdin` al daemon.

---

## 6. Comandos en la barra de la TUI (`:`)

Dentro de la TUI, presiona `:` para abrir la barra de comandos:

| Comando en `:` | Acción |
|---|---|
| `spawn [modelo] <tarea>` | Crea un agente nuevo (con modelo opcional o el profile por defecto). |
| `agent <n>` | Abre la vista del agente `<n>`. |
| `new` | Abre el formulario de creación de agente. |
| `providers` | Abre la vista de proveedores y cuotas. |
| `recovery` | Abre el Centro de Recuperación (*Recovery Center*). |
| `tasks` / `milestones` | Abre la vista del DAG de tareas o de milestones. |
| `context` / `skills` / `mcp` | Abre las vistas de contexto, skills compartidas o servidores MCP. |
| `stats` / `system` / `profile` | Abre las vistas de estadísticas/uso, recursos del sistema o perfil de rendimiento. |
| `explain-route` | Muestra la explicación del router para el agente actual. |
| `quit` | Cierra la TUI (el daemon sigue activo). |
