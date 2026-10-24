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
- **En cada proyecto, en `<proyecto>/.symphony/`** (desde P15.5): su base de datos, su daemon (socket, pid y logs), los worktrees de sus agentes, su memoria (hechos, checkpoints, chat) y `project.toml`. Cada proyecto tiene su propio daemon y **no ve nada de los demás**: sesiones, oleadas y chats son de la carpeta donde abres Symphony. `symphony daemon status|stop` actúan sobre el daemon de la carpeta actual (todavía no hay `--all`). Un `.gitignore` dentro de esa carpeta deja fuera de git todo menos `project.toml`.
- **En tu usuario, en `~/.symphony`** (o `SYMPHONY_USER_HOME`): lo que no es de un proyecto: `config.toml`, skills y `mcp.toml`, plugins, cachés y `learning.db` (lo que el router aprende de cada modelo, compartido entre proyectos).
- `SYMPHONY_HOME` fuerza un solo directorio para todo (útil en pruebas). `symphony` ubica el proyecto subiendo desde la carpeta actual hasta el primer `.symphony/project.toml` o `.git`; dentro de un worktree de un agente usa el proyecto dueño. En tu carpeta personal no funciona: abre Symphony dentro de un proyecto.
- **Si venías de antes de P15.5** (una sola base en `~/.symphony`): `symphony migrate split` apaga el daemon global y reparte esa base en cada proyecto. No borra nada; la base global queda sin usarse y la quitas tú.

No hace falta instalar Symphony dentro de cada proyecto.

### Qué se instala y qué no (regla de diseño)
- **Una sola instalación global** en tu computadora (`symphony` y `symphonyd`). Después, `symphony` se abre desde cualquier carpeta y es la única puerta de entrada: el daemon lo arranca `symphony` cuando hace falta.
- **No hay nada que instalar a mano por proyecto o carpeta**: ni Symphony, ni hooks, ni skills, ni el broker MCP en cada proveedor. Symphony los inyecta en cada ejecución del CLI del proveedor y no escribe configuración en tu proyecto (los agentes trabajan en sus worktrees bajo `<proyecto>/.symphony/worktrees`, que git ignora).
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

Si todo está verde, pasas a **Home**. Si la carpeta no es un repositorio git (documentos, video, cualquier trabajo que no sea código), prepárala antes con `symphony init --git-init` (ver «Trabajo que no es código»).

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

### Desde el terminal
```bash
symphony agents        # ver todos los agentes
symphony logs 1        # historial del agente #1
symphony diff 1        # ver el diff del agente #1
symphony inspect 1     # detalles del agente #1
```

En la barra de la TUI (`:` en Home) hay atajos: `spawn [modelo] <tarea>`, `agent <n>`, `new`, `providers`, `recovery`, `tasks`, `milestones`, `context`, `skills`, `mcp`, `stats`, `system`, `profile` y `quit`.

---

## 7. Cambiar de modelo (handoff)

Si Claude se queda sin cuota o quieres otro modelo:

```
Presiona [ s ]  →  el menú de "Switch model"
```

O desde el terminal:
```bash
symphony switch 1 openai/gpt-6-luna
```

**Lo que pasa:**
- Claude se detiene sin perder su trabajo
- Symphony crea un checkpoint (diff + última línea del transcript)
- Codex arranca **en el mismo worktree** sin que le vuelvas a explicar
- Continúa desde donde Claude se quedó

---

### Profiles, salud y failover

En vez de un modelo exacto puedes pedir un **profile** (`@code`, `@debug`, `@fast`, `@reasoning`, `@docs`, `@review`, `@conserve`, y desde P15.5 `@write`, `@media`, `@research`): Symphony elige entre los modelos que se pueden usar **ahora**.

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
| Claude Code | `claude/haiku`, `sonnet`, `opus`, … | editar archivos y ejecutar comandos | sí (`/skill`) |
| Codex | `openai/gpt-6.1-sol`, `openai/gpt-6-{astra,sol,luna}`, `openai/gpt-5.6-{sol,terra,luna}` | editar y ejecutar en su worktree, con red y rutas locales (sin sandbox por defecto) | sí (`$skill`) |
| Kimi Code | `moonshot/default` (el modelo que tengas configurado en Kimi) | **todo, sin pedir permiso**: `--print` aprueba las herramientas y Kimi no tiene sandbox | no |
| Antigravity | Google: `google/gemini-4-argon-{high,medium,low}`, `google/gemini-3.8-flash-{high,medium,low}` · Anthropic: `anthropic/claude-opus-5-5-{high,medium,low}`, `anthropic/claude-sonnet-5-5-{high,medium,low}` · OpenAI: `openai/gpt-oss-120b-medium` | editar archivos y ejecutar comandos (por defecto sin pedir permiso); es el único que no usa el harness de Symphony | no |
| Copilot | `github/auto` (el único que aceptó `--model` en la cuenta de prueba) | editar archivos y ejecutar comandos (`--allow-all-tools`) | no |
| OpenCode | `opencode/default` y lo que liste `opencode models`: `opencode/big-pickle`, `opencode/<proveedor>:<modelo>` | **todo, sin pedir permiso** en `run`, sin sandbox; Symphony le permite escribir solo en su worktree | no |

- En los modelos de Antigravity el id lleva delante el proveedor del modelo (Google, Anthropic u OpenAI) y el nombre lo muestra en el selector: `agy models` lista los trece.
- Un modelo que tu cuenta no tenga habilitado falla al instante con «not available» y Symphony pasa al siguiente según la política de failover.
- Copilot con `auto` puede usar modelos distintos entre turnos, y no informa de tokens: el cambio por umbral de uso no le aplica.
- Los permisos de Claude, Codex, Antigravity y Copilot se cambian en `~/.symphony/config.toml`, sección `[providers.permissions]`. Los valores predeterminados (desde P15.5.S2) son **acceso local total**, como una terminal normal: red, instalar paquetes y leer cualquier carpeta. Mandan cuando el CLI usa sus propias herramientas (modo `native` y siempre en Antigravity); con el harness de Symphony las herramientas las ejecuta Symphony con tu usuario. Reinicia el daemon con `symphony daemon stop` para aplicarlos. Si prefieres restringir, descomenta y cambia:
  ```toml
  [providers.permissions]
  claude_permission_mode = "bypassPermissions"   # default · acceptEdits · plan · bypassPermissions
  codex_sandbox = "danger-full-access"           # read-only · workspace-write · danger-full-access
  antigravity_skip_permissions = true            # false: pide permiso para los comandos
  copilot_allow_all_tools = true                 # false: pide permiso para los comandos
  ```
  Un valor inválido deja los predeterminados y el daemon lo avisa en su log. Kimi y OpenCode no tienen modo restringido: no hay opción.
- Los tres últimos se actualizan solos; si algo deja de funcionar tras una actualización, mira `docs/research/cli-p11.md`.
- **OpenCode** toma como raíz del proyecto el repo principal, no el worktree del agente: Symphony le avisa en el prompt cuál es su directorio, pero un modelo puede escribir en el repo con una ruta absoluta (`docs/research/cli-opencode.md`). La lista de modelos de OpenCode se pide en segundo plano la primera vez (tarda ~7 s): al principio solo aparece `opencode/default`; haz `symphony providers refresh` unos segundos después. Sus modelos gratuitos son inestables; Claude y Codex siguen siendo los que mejor funcionan (`docs/research/opencode-y-calidad-de-proveedores.md`).

### Trabajo que no es código (documentos, video, lo que sea)

Cualquier carpeta puede ser un proyecto. `symphony init` la prepara y le dice a Symphony qué se trabaja ahí (cambia los valores por defecto, no el flujo):

```bash
cd mis-informes
symphony init --kind document --git-init   # code (por defecto) · document · media · general
symphony spawn "Redactar el informe de octubre" --profile @write
```

- `--git-init` crea un repositorio con un commit vacío si la carpeta no lo es: los worktrees de los agentes y los checkpoints se apoyan en git. Sin esa opción, `symphony init` rechaza una carpeta que no sea repo git.
- Profiles nuevos: `@write` (documentos), `@media` (orquestar herramientas de video, audio e imágenes) y `@research` (investigación). Un proyecto `document`, `media` o `general` los usa por defecto; el profile del tipo se lee al arrancar el daemon, así que si acabas de cambiar `kind` con el daemon vivo corre `symphony daemon stop` (o pasa `--profile` al crear el agente).
- Sin `[validation]` en `project.toml` no hay gate de pruebas; los validators de plugin (`symphony plugin`) pueden revisar lo que importe.
- Quien produce el video o el documento son las herramientas y skills que lleve el harness (siguiente sección) o tus servidores MCP; Symphony orquesta, guarda y versiona.

### Harness de Symphony (mismas herramientas para todos los modelos)

Desde P15.5 el **harness de Symphony es el modo por defecto**: todos los modelos usan las mismas herramientas —leer, escribir, editar, buscar archivos y contenido, y shell (más las `context_*`)—, servidas por `symphony mcp serve`; las herramientas propias de cada CLI se apagan, así que todos trabajan con la misma interfaz y los mismos permisos (ADR-0019). Se elige en `~/.symphony/config.toml`:

```toml
[harness]
mode = "symphony"   # symphony (por defecto) · native (las herramientas de cada CLI)
```

Funciona con Claude Code, Codex, OpenCode, Copilot y Kimi; **Antigravity no lo admite** (no deja apagar sus herramientas sin tocar su configuración) y sigue con las suyas. Con `native` cada CLI vuelve a trabajar con sus herramientas y sus propios permisos. Verificado en vivo, con la comparación de costo, en `docs/research/harness-2026-10.md`: la misma tarea mínima con Claude costó un 68 % menos con el harness de Symphony. Reinicia el daemon (`symphony daemon stop`) para aplicarlo.

### Sugerencias de modelo

Si una tarea cambia de tipo (por ejemplo, de documentación a un refactor de varios archivos) y hay un modelo claramente mejor para ese trabajo, Symphony te lo **sugiere**; nunca cambia el modelo por su cuenta. La sugerencia aparece en el chat y en la vista del agente:

- chat: `Ctrl+T` cambia al modelo sugerido, `Ctrl+K` mantiene el actual, `Ctrl+N` no vuelve a sugerir cambios en ese agente;
- vista del agente: `S`, `K` y `N`;
- terminal: `symphony suggestion list|switch|keep|mute <id>`.

Con el uso, los modelos que rinden bien en un profile (`@code`, `@docs`…) ganan un pequeño ajuste de puntaje (solo con 10 o más runs observados; ADR-0016).

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

1. **Revisar diff** desde la vista del agente:
   ```
   Elige el agente #1
   [ d ]  →  git diff
   ```

2. **Integrar la tarea en el chat** desde la vista del agente:
   ```
   [ g ]  →  preflight de integración (p lo corre, c conflictos, w revisión)
   [ a ]  →  aprueba, si la política del proyecto lo pide
   [ i ]  →  integra en la rama del chat
   ```

O manualmente, sin pasar por el chat:
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
En la vista del agente, `x` dos veces lo detiene (el workspace se conserva). Desde el terminal:
```bash
symphony kill 1
```

### Necesitas recuperarte de un crash
```
Home → [ r ]  Recovery Center
       ↓
       Ver agentes interrumpidos
       Elegir uno → retomar
```

### Ver logs
```bash
symphony logs 1 --limit 50    # últimas 50 entradas
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
symphony logs 1 --limit 20

# Matar agente
symphony kill 1
```

La integración de un agente (preflight, aprobación y merge) se hace en la TUI, en su vista: `g` → `i` (ver «Ver cambios y mergear»). Para llevar el chat a `main`, `symphony chat merge`.

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

[providers.permissions]
# Acceso local total por defecto. Descomenta y cambia para restringir (ver «Proveedores y permisos»).
# claude_permission_mode = "bypassPermissions" # default · acceptEdits · plan · bypassPermissions
# codex_sandbox = "danger-full-access"         # read-only · workspace-write · danger-full-access
# antigravity_skip_permissions = true          # false: pide permiso para los comandos
# copilot_allow_all_tools = true               # false: pide permiso para los comandos

[harness]
# Herramientas con las que trabajan los modelos: symphony (por defecto, las de Symphony)
# o native (las de cada CLI). Reinicia el daemon para aplicarlo.
mode = "symphony"

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
7. Cuando termina, en su vista [ g ] y luego [ i ] lo integran en el chat
8. `symphony chat merge` hace el preflight del chat y `--yes` lo integra en main
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
- ✅ **P11**: Más proveedores (Kimi, Antigravity, Copilot, OpenCode): ver «Proveedores y permisos»
- ✅ **P15.5**: estado y daemon por proyecto, harness de Symphony por defecto, acceso local total y trabajo que no es código
- ⏳ **P15**: GUI de escritorio (Tauri)

Por ahora: **v0.1 + chat general + seis proveedores (Claude Code, Codex, Kimi, Antigravity, Copilot y OpenCode) + handoff entre ellos**, cada proyecto aislado en su carpeta y con las mismas herramientas para todos los modelos.

---

## ¿Preguntas?

- Spec completo: [`docs/spec/idea.md`](docs/spec/idea.md)
- Flow de UX: [`docs/spec/Symphony_CLI_User_Flow_and_Views.html`](docs/spec/Symphony_CLI_User_Flow_and_Views.html)
- Plan: [`PLAN.md`](PLAN.md)
- Issues: crea un ticket en GitHub

¡Bienvenido a Symphony!
