# Guía rápida: Usar Symphony v0.1

## 0. Requisitos previos

- **Rust 1.95+** y Git instalados
- Al menos uno de estos CLIs instalado y autenticado: **Claude Code**, **Codex**, **Kimi Code**, **Antigravity** (`agy`), **GitHub Copilot CLI** u **OpenCode**
- Windows, macOS o Linux

Verifica:
```bash
rustc --version
claude --version    # si lo tienes
codex --version     # si lo tienes
kimi --version      # si lo tienes
agy --version       # si lo tienes
copilot --version   # si lo tienes
```

---

## 1. Obtener Symphony

### Opción A (recomendada): instalar globalmente
Así `symphony` se abre desde cualquier carpeta, no solo desde este repo.
```bash
git clone https://github.com/Vakyro/symphony-cli.git
cd symphony-cli

# Instala `symphony` y `symphonyd` en ~/.cargo/bin (toma varios minutos la primera vez)
cargo install --path crates/cli --locked
cargo install --path crates/daemon --locked

# Comprueba, desde otra carpeta cualquiera:
cd ~/mi-proyecto
symphony --version
```

`symphony` busca `symphonyd` junto a su propio ejecutable, así que ambos deben quedar en la misma carpeta (con `cargo install` lo están). `~/.cargo/bin` debe estar en el `PATH` (rustup lo agrega).

**Para actualizar:** repite los dos `cargo install` con `--force`. En Windows, detén antes el daemon (`symphony daemon stop`); si sigue corriendo, el instalador falla con «Acceso denegado» porque `symphonyd.exe` está en uso.

**Si `symphony --version` muestra una versión vieja:** hay otra copia antes en el `PATH`. Búscalas todas (PowerShell: `Get-Command symphony -All`; bash: `which -a symphony`) y borra la vieja o pon `~/.cargo/bin` primero. `symphony` y `symphonyd` deben venir del mismo lugar.

### Opción B: solo compilar
```bash
cargo build -p symphony-cli -p symphony-daemon --release
# Binarios en target/release/: symphony y symphonyd (.exe en Windows)
```

### Opción C: sin instalar (solo dentro del repo)
```bash
cd symphony-cli
# Todos los comandos abajo funcionan con "cargo run -p symphony-cli --"
# en lugar de "symphony". Solo funciona con este repo como carpeta actual.
```

Asume `symphony` en el PATH de aquí en adelante.

### Dónde guarda Symphony sus datos
- **Global, en `~/.symphony`** (o `SYMPHONY_HOME`): la base de datos SQLite, el daemon (socket y logs), los worktrees de los agentes y `config.toml`. Se comparte entre todos tus proyectos.
- **En tu proyecto, en `<proyecto>/.symphony/`**: la configuración propia del proyecto (`project.toml`). Se crea en el primer arranque de la TUI (first-run), no al crear un agente desde la línea de comandos.

No hace falta instalar Symphony dentro de cada proyecto.

### Qué se instala y qué no (regla de diseño)
- **Una sola instalación global** en tu computadora (`symphony` y `symphonyd`). Después, `symphony` se abre desde cualquier carpeta y es la única puerta de entrada: el daemon lo arranca `symphony` cuando hace falta.
- **No hay nada que instalar a mano por proyecto o carpeta**: ni Symphony, ni hooks, ni skills, ni el broker MCP en cada proveedor. Symphony los inyecta en cada ejecución del CLI del proveedor y no escribe configuración en tu proyecto (los agentes trabajan en sus worktrees bajo `~/.symphony`).
- Lo único que haces por tu cuenta es instalar y autenticar los CLIs de los proveedores, una vez por computadora.
- Si algún paso te pide copiar archivos de Symphony a un proyecto, es un defecto: repórtalo (CONSTRAINTS A12).

---

## 2. Entrar a tu proyecto

```bash
cd /ruta/a/tu/proyecto
symphony
```

**Primera vez:** Se abre la TUI. Symphony detecta:
- ✅ CLIs de proveedores (Claude Code, Codex) en tu máquina
- ✅ El proyecto Git
- ✅ Dependencias (pnpm, npm, pip, etc.)

Si todo está verde, pasas a **Home**.

---

## 3. Home: El dashboard

Ves una lista de agentes (vacía la primera vez) y opciones:

```
SYMPHONY · proyecto-name

 HOME

 Agentes         Activos: 0

 (lista vacía)

─────────────────────────────────────────────────────────────

 [ n ]  Crear agente       [ p ]  Proveedores
 [ r ]  Recuperación       [ q ]  Salir

 Presiona : para la barra de comandos
```

---

## 4. Crear tu primer agente

**Opción A: Con menú (teclas)**
1. Presiona `n`
2. Escribe la tarea:
   ```
   Agregar validación de email con tests
   ```
3. Presiona Enter para avanzar por el menú (selecciona proveedor, modelo)
4. El agente se crea y abre en la vista de trabajo

**Opción B: Comando rápido (barra de comandos)**
1. Presiona `:`
2. Escribe:
   ```
   spawn claude/sonnet Agregar validación de email con tests
   ```
3. Presiona Enter → agente creado al instante

### ¿Qué hace Symphony?
- Crea un `worktree` (rama aislada) para el agente
- Lanza Claude Code (o el modelo que elegiste)
- Muestra la vista de trabajo del agente en vivo

---

## 5. Trabajar con el agente

### Ves la vista de agente:
```
AGENTE #1 · Agregar validación de email con tests

 Estado: RUNNING (Claude Code / haiku-4.5)
 Worktree: symphony/session-xyz/agent-1

 ┌─────────────────────────────────────────┐
 │ Conversation                            │
 │                                         │
 │ [Claude empieza a trabajar]             │
 │                                         │
 └─────────────────────────────────────────┘

 [ ↑ ↓ ]  Scroll  [ a ]  Attach
 [ d ]  Diff     [ s ]  Switch  [ : ]  Comando
```

El agente trabaja solo. Tú ves:
- ✅ El progreso en tiempo real
- ✅ Los archivos que toca
- ✅ Los tests que corre
- ✅ Cuándo termina o falla

---

## 6. Intervenir (opciones)

### Ver el diff de lo que cambió
```
Presiona [ d ]  →  Ves git diff del worktree del agente
```

### Seguir la conversación después de que el agente termine su turno
Cuando el agente termina (`COMPLETED`), puedes escribirle otro mensaje: Symphony retoma la sesión del CLI (Claude Code o Codex) con el mismo modelo, y el CLI recuerda lo anterior. El agente vuelve a `RUNNING` y, al terminar, a `COMPLETED`.
```
symphony send 1 "ahora agrega tests"
```
En la TUI, presiona `m` en la vista del agente y escribe el mensaje. Si el agente está `FAILED` o `PAUSED`, el mensaje se rechaza con una pista (recupéralo o reanúdalo primero).

### Abrir el agente en su CLI nativo
```
Presiona [ a ]  →  Se abre Claude Code en su propia terminal
                   Puedes escribir mensajes nuevos
                   Después vuelve a Symphony
```

### Hacer un comando en la barra
```
Presiona [ : ]
agents           # ver todos los agentes
logs 1           # ver logs del agente #1
diff 1           # ver diff del agente #1
inspect 1        # detalles del agente #1
```

---

## 7. Cambiar de modelo (handoff)

Si Claude se queda sin cuota o quieres otro modelo:

```
Presiona [ s ]  →  Men menú de "Switch model"
```

O comando:
```
:
switch 1 codex/gpt-5.x
```

**Lo que pasa:**
- Claude se detiene sin perder su trabajo
- Symphony crea un checkpoint (diff + última línea del transcript)
- Codex arranca **en el mismo worktree** sin que le vuelvas a explicar
- Continúa desde donde Claude se quedó

---

### Profiles, salud y failover

En vez de un modelo exacto puedes pedir un **profile** (`@code`, `@debug`, `@fast`, `@reasoning`, `@docs`, `@review`, `@conserve`): Symphony elige entre los modelos que se pueden usar **ahora**.

```bash
symphony spawn "Arreglar el login" --profile @fast     # el router elige el modelo
symphony switch 1 --profile @reasoning                 # cambia a lo mejor para razonar
symphony explain-route 1                               # por qué se eligió, con cada candidato
```

En la TUI: **Nuevo agente → Ejecución → Profile**, o `s` en un agente; con `?` (o `:explain-route`) ves la explicación.

- **Un modelo exacto nunca se sustituye en silencio**: si lo eliges tú, se obedece (aunque su cuota esté baja). Solo se cambia por failover, si no está disponible y la política del agente lo permite (`none` · `same-provider` · `any`).
- **El router descarta** lo que no sirve y dice por qué: proveedor sin sesión (`AUTH`), cuota agotada (`EXHAUSTED`), en espera tras un límite temporal (`COOLDOWN`), contexto o herramientas insuficientes, o dentro de la **reserva** de cuota (los profiles automáticos no gastan el último 20 %; tú sí, a mano).
- **Un 429 no es «cuota agotada»**: es un límite temporal (`RATE_LIMITED`), con su espera, y el proveedor vuelve a probarse solo cuando pasa (`PROBING`).
- **La cuota nunca se inventa**: se muestra `KNOWN` (el CLI la informa: Claude y Codex), `ESTIMATED` (solo si le das un presupuesto en la config) o `UNKNOWN` («restante desconocido»).
- `symphony providers` y la pantalla de Proveedores (`p`) muestran la salud y la cuota de cada uno; `symphony usage` el uso de tokens (informado frente a estimado).

### Proveedores y permisos

| Proveedor | Id de modelo | Qué puede hacer el agente | Skills en el chat |
|---|---|---|---|
| Claude Code | `claude/haiku`, `sonnet`, `opus`, … | editar archivos | sí (`/skill`) |
| Codex | `openai/gpt-5.6-luna`, … | editar dentro del worktree | sí (`$skill`) |
| Kimi Code | `moonshot/default` (el modelo que tengas configurado en Kimi) | **todo, sin pedir permiso**: `--print` aprueba las herramientas y Kimi no tiene sandbox | no |
| Antigravity | `google/gemini-3.8-flash-low`, `…-medium`, `…-high`, `gemini-3.1-pro-high/low` | editar archivos; **no ejecuta comandos** (en headless deniega lo que pide permiso) | no |
| Copilot | `github/auto` (el único que aceptó `--model` en la cuenta de prueba) | editar archivos; comandos solo de lectura | no |
| OpenCode | `opencode/default` y lo que liste `opencode models`: `opencode/big-pickle`, `opencode/<proveedor>:<modelo>` | **todo, sin pedir permiso** en `run`, sin sandbox; Symphony le permite escribir solo en su worktree | no |

- Un modelo que tu cuenta no tenga habilitado falla al instante con «not available» y Symphony pasa al siguiente según la política de failover.
- Copilot con `auto` puede usar modelos distintos entre turnos, y no informa de tokens: el cambio por umbral de uso no le aplica.
- Los permisos de Claude, Codex, Antigravity y Copilot se cambian en `~/.symphony/config.toml`, sección `[providers.permissions]` (los valores de la tabla son los predeterminados; reinicia el daemon con `symphony daemon stop` para aplicarlos):
  ```toml
  [providers.permissions]
  claude_permission_mode = "acceptEdits"     # default · acceptEdits · plan · bypassPermissions
  codex_sandbox = "workspace-write"          # read-only · workspace-write · danger-full-access
  antigravity_skip_permissions = false       # true: cualquier comando, sin sandbox
  copilot_allow_all_tools = false            # true: cualquier comando
  ```
  Un valor inválido deja los predeterminados y el daemon lo avisa en su log. Kimi y OpenCode no tienen modo restringido: no hay opción.
- Los tres últimos se actualizan solos; si algo deja de funcionar tras una actualización, mira `docs/research/cli-p11.md`.
- **OpenCode** toma como raíz del proyecto el repo principal, no el worktree del agente: Symphony le avisa en el prompt cuál es su directorio, pero un modelo puede escribir en el repo con una ruta absoluta (`docs/research/cli-opencode.md`). La lista de modelos de OpenCode se pide en segundo plano la primera vez (tarda ~7 s): al principio solo aparece `opencode/default`; haz `symphony providers refresh` unos segundos después. Sus modelos gratuitos son inestables; Claude y Codex siguen siendo los que mejor funcionan (`docs/research/opencode-y-calidad-de-proveedores.md`).

### Plugins

Un plugin es un programa aparte que Symphony lanza para una tarea concreta. Hoy hay dos tipos: **validator** (revisa lo que cambió una tarea y corre en el tier 2 de la validación) y **context-transformer** (reescribe el handoff). No abre tu base ni pide a Symphony que ejecute nada: recibe un JSON por stdin y contesta un JSON por stdout.

```bash
symphony plugin list           # lo instalado en ~/.symphony/plugins, qué activa este proyecto y lo roto
symphony plugin check <nombre> # lo lanza, le pide `describe` y verifica que es quien dice ser
```

Instalación (global, una vez): una carpeta `~/.symphony/plugins/<nombre>/` con un `plugin.toml` y el ejecutable. Activación (por proyecto), en `.symphony/project.toml`:

```toml
[plugins]
validators = ["validator-todo-check"]   # corren en el tier 2; el gate usa [validation] gate (2 por defecto)
context_transformers = []               # se aplican al handoff, en orden
```

Hay un ejemplo completo en `examples/plugins/validator-todo-check` (falla si el diff agrega un `TODO` sin issue): `cargo build -p validator-todo-check`, copia el binario y su `plugin.toml` a `~/.symphony/plugins/validator-todo-check/`. Para escribir el tuyo, el protocolo (v1) es:

```text
{"plugin_api":1,"kind":"validator","method":"describe","params":{}}
→ {"plugin_api":1,"ok":true,"result":{"name":"…","version":"…","plugin_api":1,"kind":"validator"}}
{"plugin_api":1,"kind":"validator","method":"validate","params":{"task_code","task_title","diff","files"}}
→ {"plugin_api":1,"ok":true,"result":{"passed":false,"summary":"…","findings":[{"severity","file","text"}]}}
{"plugin_api":1,"kind":"context-transformer","method":"transform","params":{"stage":"handoff","text":"…"}}
→ {"plugin_api":1,"ok":true,"result":{"text":"…"}}
```

Reglas: stdout es solo la respuesta (los registros, a stderr); tiempo máximo `timeout_secs` (10 s por defecto, 300 máx.); un plugin que muere o se cuelga queda como check `SKIPPED` con el motivo (no se da por bueno); lo que devuelve pasa por el redactor de secretos. Un plugin es código tuyo con tus permisos: el aislamiento protege a Symphony de sus fallos, no te protege de un plugin malicioso. Los tipos `tool` y `provider-adapter` se reservan para v1.1.

### Skills en el chat
Las skills que ya tienes instaladas en cada CLI funcionan en el chat (las de usuario y las de plugins; las de proyecto solo si están commiteadas en la rama base del chat):
- Con un modelo de **Claude**: escribe `/nombre-de-la-skill lo que quieras` al inicio del mensaje.
- Con un modelo de **Codex**: escribe `$nombre-de-la-skill lo que quieras`.
- Al cambiar de proveedor, la skill que escribas con el prefijo del proveedor **destino** se aplica; con el del otro no se reconoce. Lo que una skill deja activo en la sesión (p. ej. un «modo») no viaja en el cambio.

## 8. Ver cambios y mergear

Cuando el agente dice "listo":

1. **Revisar diff** desde Home:
   ```
   Elige el agente #1
   [ d ]  →  git diff
   ```

2. **Mergear a main** desde Home:
   ```
   [ : ]
   merge 1
   ```

O manualmente:
```bash
cd /ruta/a/tu/proyecto
git branch      # ves symphony/session-xyz/agent-1
git switch main
git merge symphony/session-xyz/agent-1
git branch -d symphony/session-xyz/agent-1
```

### Integrar el chat en `main`

El chat trabaja en la rama `symphony/chat` y los agentes se integran a ella: `main` no se toca hasta que tú lo pides. Para llevar lo que hay en el chat a `main` (un commit de merge `--no-ff`):

```bash
symphony chat merge            # preflight: qué entra, conflictos, estado de tu repo y de la revisión
symphony chat merge --review   # pide a un review agent que revise el chat (solo recomienda)
symphony chat merge --yes      # integra (exige el preflight sin ✗ y una revisión que apruebe)
```

En la vista Chat: `Ctrl+G` hace el preflight, `Ctrl+R` pide la revisión y una segunda `Ctrl+G` seguida (con la revisión aprobada) integra. Symphony no cambia tu rama: tu repo tiene que estar en `main` y sin cambios sin commitear. Si el revisor falló o no hubo revisión, `--yes --skip-review` integra igual, porque tú lo decides; si pidió cambios, también hace falta `--skip-review`.

---

## 9. Múltiples agentes en paralelo

Crea 2 agentes a la vez:

```
Home → [ n ]  Agregar autenticación
Home → [ n ]  Refactorizar estilos
```

Symphony **limita recursos automáticamente** para que ambos corran sin trabar la máquina.

Ves ambos:
```
Home

Agentes: 2

 #1  Agregar autenticación    RUNNING (Claude)
 #2  Refactorizar estilos     RUNNING (Codex)

─────────────────────────────────────────────────
```

Selecciona uno con `↑ ↓` / número y entra a su vista.

---

## 10. Si algo falla

### Agente se cuelga
```
[ : ]
kill 1
```

### Necesitas recuperarte de un crash
```
Home → [ r ]  Recovery Center
       ↓
       Ver agentes interrumpidos
       Elegir uno → retomar
```

### Ver logs
```
[ : ]
logs 1 --tail 50    # últimas 50 líneas
```

---

## 11. Cerrar y volver

```
Home → [ q ]  Salir
```

Cuando vuelvas a entrar al proyecto:
```bash
cd /ruta/a/tu/proyecto
symphony
```

**Se restaura todo:**
- ✅ Agentes anteriores listos
- ✅ Worktrees intactos
- ✅ Tareas sin terminar

---

## 12. Desde el terminal (sin TUI)

Si prefieres no usar la TUI, hay comandos:

```bash
# Crear agente
symphony spawn "Tarea aquí"

# Ver agentes
symphony agents

# Inspeccionar uno
symphony inspect 1

# Ver diff
symphony diff 1

# Ver logs
symphony logs 1

# Matar agente
symphony kill 1

# Merge
symphony merge 1
```

---

## 13. Configuración (opcional)

Edita `~/.symphony/config.toml`:

```toml
[routing]
# Profile (o modelo) por defecto, y qué hacer si un proveedor falla: NONE · SAME_PROVIDER · ANY
default_profile = "@code"
failover = "ANY"

[providers]
# Fracción de cuota que los profiles automáticos no gastan (0.0–1.0)
quota_reserve = 0.20

# Ajustes por proveedor (opcional). Con un presupuesto, Symphony estima tu cuota por tokens
# usados (siempre como «estimada», nunca como porcentaje exacto).
[providers.limits.github]
reserve = 0.30
window_hours = 720
window_tokens = 5000000

[chat]
# Apagado por defecto: si el último turno llegó a estos tokens, el chat pasa al siguiente proveedor.
# switch_at_tokens = 150000
```

El archivo completo, con sus comentarios, se crea la primera vez en `~/.symphony/config.toml`; `symphony` rechaza claves desconocidas y te dice cuál.

---

## 14. Flujo completo (resumen)

```
1. cd proyecto
2. symphony
3. Presiona [ n ] → tarea nueva
4. Ves al agente trabajar
5. Presiona [ d ] para ver cambios
6. Presiona [ s ] para cambiar modelo si falta cuota
7. Cuando termina, presiona [ : ] y escribe "merge 1"
8. Los cambios van a main
9. [ q ] para salir
```

---

## 15. Tips

- **Speed mode:** Presiona `T` en Home para cambiar tema (algo te lo dice con teclas)
- **Múltiples terminales:** Puedes tener 2 ventanas `symphony` simultáneas en proyectos distintos
- **Repo de prueba:** Clona https://github.com/Vakyro/symphony-demo (pequeño Next.js con autenticación lista)
- **Debug:** Seteá `RUST_LOG=debug` antes de `symphony` para ver tracing
- **Problema:** Si `symphonyd` queda huérfano (test interrupido), `pkill symphonyd` / Task Manager

---

## Próximos pasos (P08+)

Cuando Leo use v0.1 durante 2+ semanas y documente learnings:
- ⏳ **P08**: Scheduler de 3+ agentes, DAG de tareas
- ⏳ **P09**: Context Engine (handoff sin explicación)
- ⏳ **P10**: Failover automático
- ✅ **P11**: Más proveedores (Kimi, Antigravity, Copilot): ver «Proveedores y permisos»
- ⏳ **P15**: GUI de escritorio (Tauri)

Por ahora: **v0.1 + chat general + cinco proveedores (Claude Code, Codex, Kimi, Antigravity, Copilot) + handoff entre ellos**.

---

## ¿Preguntas?

- Spec completo: [`docs/spec/idea.md`](docs/spec/idea.md)
- Flow de UX: [`docs/spec/Symphony_CLI_User_Flow_and_Views.html`](docs/spec/Symphony_CLI_User_Flow_and_Views.html)
- Plan: [`PLAN.md`](PLAN.md)
- Issues: crea un ticket en GitHub

¡Bienvenido a Symphony!
