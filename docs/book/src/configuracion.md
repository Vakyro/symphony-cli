# Configuración

Symphony utiliza dos archivos TOML:
1. **`~/.symphony/config.toml`** (o en `$SYMPHONY_USER_HOME`): configuración global del usuario. Se crea automáticamente la primera vez con todos los valores por defecto comentados. Symphony usa `toml_edit` para preservar tus comentarios y **rechaza cualquier clave desconocida** indicando el nombre exacto de la clave inválida.
2. **`<proyecto>/.symphony/project.toml`**: configuración específica de un proyecto. Se crea con `symphony init` y permite definir el tipo de proyecto, el gate de validación, los comandos de preparación de worktrees, la política de merge y los plugins activos.

> Importante: cuando cambies opciones en `~/.symphony/config.toml` o `<proyecto>/.symphony/project.toml` que afecten al daemon (como `[harness]`, `[providers.permissions]` o el tipo de proyecto), detén el daemon con `symphony daemon stop` para que el siguiente comando lo arranque con la nueva configuración.

---

## Configuración global (`~/.symphony/config.toml`)

Ejemplo completo con todas las secciones válidas y sus valores por defecto:

```toml
[performance]
# ECO · BALANCED · PERFORMANCE · CUSTOM
profile = "BALANCED"
# Opcionales: pisan los límites del perfil.
# max_heavy = 1
# cpu_ceiling_pct = 70.0
# ram_ceiling_pct = 70.0
# hold_max_secs = 60
# Límites del SO por agente (Job Object en Windows, cgroup v2 en Linux).
# os_cpu_cores = 2.0
# os_max_memory_mb = 4096
# os_max_processes = 512

[routing]
# Profile o modelo por defecto al crear un agente.
default_profile = "@code"
# Qué hacer si el proveedor falla: NONE · SAME_PROVIDER · ANY
failover = "ANY"

[plan]
# El agente planifica por fases y tareas (`.symphony/plan/<tarea>.md`, temporal)
# y cada fase corre en una sesión nueva.
# phase_start: "auto" (arranca sola) o "ask" (espera confirmación).
enabled = true
phase_start = "auto"
min_tasks_per_phase = 2

[harness]
# Con qué herramientas trabajan los modelos (ADR-0019):
# "symphony" (por defecto: read, write, edit, glob, grep, shell y context_* vía MCP)
# o "native" (las herramientas propias de cada CLI).
mode = "symphony"

[context]
# Cuánto se comprime el contexto en un handoff: RAW · SAFE · BALANCED · AGGRESSIVE
mode = "BALANCED"

[providers]
# Fracción de cuota que los profiles automáticos reservan (0.0–1.0).
quota_reserve = 0.20

# Ajustes por proveedor (opcional). Con un presupuesto, Symphony estima la cuota
# por tokens usados (siempre marcada como ESTIMATED, nunca como porcentaje exacto).
# [providers.limits.github]
# reserve = 0.30
# window_hours = 720
# window_tokens = 5000000

[providers.permissions]
# Qué puede hacer cada CLI sin preguntarte. Los valores de abajo son los predeterminados
# desde P15.5.S2: acceso local total, como una terminal normal.
# Para restringir, descomenta y cambia. Kimi y OpenCode no tienen modo restringido.
claude_permission_mode = "bypassPermissions" # default · acceptEdits · plan · bypassPermissions
codex_sandbox = "danger-full-access"         # read-only · workspace-write · danger-full-access
antigravity_skip_permissions = true          # false: pide permiso para los comandos
copilot_allow_all_tools = true               # false: pide permiso para los comandos

[logging]
# error · warn · info · debug · trace
level = "info"

[chat]
# Apagado por defecto. Si se define, cuando el último turno del chat llega a esta
# cantidad de tokens de contexto, el siguiente mensaje pasa al siguiente proveedor.
# switch_at_tokens = 150000
```

### Detalle de cada sección global

- **`[performance]` (ADR-0002, ADR-0003):**
  - `profile`: `"ECO"` (1 núcleo por agente), `"BALANCED"` (2 núcleos por defecto), `"PERFORMANCE"` (sin tope de núcleos) o `"CUSTOM"`.
  - `max_heavy`: máximo de operaciones pesadas (clases 3 y 4, como `npm test` o `cargo build`) corriendo a la vez (> 0).
  - `cpu_ceiling_pct` y `ram_ceiling_pct`: techos porcentuales (`1.0` a `100.0`) por encima de los cuales las operaciones pesadas esperan en cola.
  - `hold_max_secs`: segundos máximos que un hook `PreToolUse` retiene una herramienta en cola antes de responder `deny` con la explicación de espera (> 0).
  - `os_cpu_cores`, `os_max_memory_mb`, `os_max_processes`: límites aplicados por el supervisor de procesos del sistema operativo a cada árbol de agente.
- **`[routing]`:**
  - `default_profile`: profile inicial cuando no se indica `--model` ni `--profile` (por defecto `"@code"`). Profiles disponibles: `@code`, `@debug`, `@fast`, `@reasoning`, `@docs`, `@review`, `@conserve`, `@write`, `@media`, `@research`.
  - `failover`: `"ANY"` (cambia a cualquier proveedor elegible), `"SAME_PROVIDER"` (solo dentro del mismo proveedor) o `"NONE"` (no cambia automáticamente).
- **`[plan]` (ADR-0011, ADR-0013):**
  - `enabled`: si es `true`, cada agente creado planifica su tarea por fases en `.symphony/plan/<task_id>.md` y abre una sesión limpia al iniciar cada fase (`PHASE_DONE`).
  - `phase_start`: `"auto"` o `"ask"`.
  - `min_tasks_per_phase`: mínimo de tareas por fase (por defecto `2`; debe ser > 0).
- **`[harness]` (ADR-0019):**
  - `mode`: `"symphony"` (por defecto) expone las mismas herramientas mediante `symphony mcp serve` y desactiva las nativas en Claude Code, Codex, OpenCode, Copilot y Kimi; `"native"` deja a cada CLI con sus herramientas propias. Antigravity no soporta el harness por invocación y siempre usa sus herramientas nativas.
- **`[context]`:**
  - `mode`: nivel de compresión de los handoffs entre proveedores o fases: `"RAW"`, `"SAFE"`, `"BALANCED"` (por defecto) o `"AGGRESSIVE"`. El original completo siempre queda guardado en el almacén de objetos y es recuperable mediante URIs `ctx://` (`symphony context raw <uri>`).
- **`[providers]` y `[providers.limits.<id>]`:**
  - `quota_reserve`: fracción entre `0.0` y `1.0` (por defecto `0.20`) que los profiles automáticos guardan sin gastar. Si eliges un modelo exacto a mano, la reserva se ignora.
  - `reserve`, `window_hours` y `window_tokens`: permiten fijar una reserva específica por proveedor y un presupuesto de tokens por ventana horaria para estimar la cuota (`ESTIMATED`) en proveedores que no la reportan.
- **`[providers.permissions]`:**
  - Controla los flags de permisos y sandbox pasados a cada CLI. Por defecto otorgan acceso local total (`bypassPermissions`, `danger-full-access`, `true`, `true`).
- **`[logging]`:**
  - `level`: nivel del filtro de `tracing` (`"error"`, `"warn"`, `"info"`, `"debug"`, `"trace"`). También puedes definir `RUST_LOG=debug` en el entorno antes de lanzar `symphony`.
- **`[chat]`:**
  - `switch_at_tokens`: entero opcional (> 0). Si el uso reportado del último turno del chat alcanza este umbral, el turno siguiente cambia automáticamente de proveedor llevando el handoff conversacional.

---

## Configuración por proyecto (`<proyecto>/.symphony/project.toml`)

Para inicializar un proyecto usa `symphony init`:

```bash
cd /ruta/a/tu/proyecto
symphony init --kind code          # code (por defecto) · document · media · general
symphony init --kind document --git-init --name mis-informes
```

- `--kind` ajusta el tipo de trabajo del proyecto (ADR-0020). Cuando `routing.default_profile` sigue en su valor de fábrica, un proyecto `document`, `media` o `general` arranca con el profile de su tipo (`@write`, `@media` o `@research`).
- `--git-init` inicializa un repositorio Git con un commit vacío si la carpeta aún no es un repositorio Git (necesario porque los worktrees y checkpoints se apoyan en Git).

### Estructura de `project.toml`

```toml
[project]
name = "mi-proyecto"
default_branch = "main"
kind = "code" # code · document · media · general

# Opcionales: sobrescriben la configuración global en este proyecto
# [routing]
# default_profile = "@code"
# failover = "ANY"

# [context]
# mode = "BALANCED"

# [performance]
# profile = "BALANCED"

[worktree]
# Comandos que corren en el worktree nuevo antes de lanzar al agente
prepare = ["pnpm install"]
prepare_timeout_secs = 300

[validation]
# Comandos por tier que corren a través del scheduler
tier1 = ["cargo fmt --check"]
tier2 = ["cargo clippy -- -D warnings", "cargo nextest run"]
tier3 = ["cargo build --release"]
# Hasta qué tier debe pasar una tarea para completarse (0 = sin gate; 2 por defecto)
gate = 2
max_attempts = 3
timeout_secs = 900

[merge]
require_approval = true
commit_on_complete = true

[plugins]
# Nombres de plugins ya instalados en ~/.symphony/plugins/
validators = ["validator-todo-check"]
context_transformers = []
```

---

## Plugins (`~/.symphony/plugins/` y `[plugins]`, ADR-0015)

Un plugin es un ejecutable externo aislado que recibe una línea JSON por `stdin` y responde una línea JSON por `stdout` (protocolo `plugin_api = 1`), sin abrir la base de datos ni pedir a Symphony que ejecute comandos.

- **Instalación (global):** crea `~/.symphony/plugins/<nombre>/plugin.toml` junto con su ejecutable:
  ```toml
  name = "validator-todo-check"
  version = "0.1.0"
  plugin_api = 1
  kind = "validator"                # validator | context-transformer
  exec = ["./validator-todo-check"] # ruta relativa dentro de la carpeta o binario del PATH
  description = "Falla si el diff agrega un TODO sin issue"
  timeout_secs = 10                 # 1–300 (10 por defecto)
  ```
- **Activación (por proyecto):** añade el nombre en `[plugins]` dentro de `<proyecto>/.symphony/project.toml`.
- **Verificación desde la CLI:**
  ```bash
  symphony plugin list
  symphony plugin check validator-todo-check
  ```

---

## Skills y servidores MCP compartidos

- **Skills compartidas (`~/.symphony/skills` y `.symphony/skills`):**
  ```bash
  symphony skills list
  symphony skills enable <nombre>
  symphony skills disable <nombre>
  ```
  Además, en el chat funcionan las skills nativas que ya tengas instaladas en tus CLIs (`/nombre` en Claude Code, `$nombre` en Codex).
- **Servidores MCP (`mcp.toml` en `~/.symphony/` y `.symphony/`):**
  ```bash
  symphony mcp list
  symphony mcp enable <nombre>
  symphony mcp disable <nombre>
  ```
  El comando `symphony mcp serve` es el servidor MCP stdio que Symphony inyecta automáticamente en los agentes para servir el contexto (`context_search`, `context_retrieve`, `context_lines`) y las herramientas del harness.
