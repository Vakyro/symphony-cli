# OpenCode como proveedor y estado de calidad de los proveedores

Registro de dos peticiones de Leo (2026-10-10). **Es documentación de la idea: no hay código.** Durante la sesión se empezó un adapter de OpenCode y se revirtió completo a pedido de Leo («solo es documentar la idea»); lo que sigue incluye lo que se observó del CLI real antes de revertir, para no repetir la investigación.

---

## 1. Peticiones de Leo

**OpenCode como agente disponible** (textual):

> «quiero que se agregue opencode como agente disponible, trata de agregarlo, esta instalado en esta computadora, ejecutalo como los demas, y que obvio tenga la persistencia de contexto»

**Calidad de los proveedores** (textual):

> «hasta ahora solo claude code y codex son los modelos que mejor me han funcionado, a los demas hay que volver a probarlos que tengan buen funcionamiento»

Lectura de ambas:

| Petición | Qué implica |
|---|---|
| OpenCode disponible | Un proveedor más, igual que Claude, Codex, Kimi, Antigravity y Copilot: se detecta, aparece en `symphony providers`, se lanza con `spawn`, funciona en el chat y en failover/cambio de proveedor |
| Persistencia de contexto | Lo mismo que ya ofrecen los demás: la conversación sobrevive entre turnos (`resume`) y entre proveedores (handoff) |
| Claude Code y Codex son los que funcionan | Son los únicos validados por el uso real de Leo; Kimi, Antigravity y Copilot (y OpenCode, cuando exista) quedan **sin validar** hasta repetir la prueba |

---

## 2. OpenCode: lo observado en esta máquina

Versión 1.18.34, instalado con npm (`~/AppData/Roaming/npm`). Todo salió de ejecuciones reales con modelos gratuitos de OpenCode, en una carpeta temporal fuera del repo.

### Cómo se lanza

| Necesidad | Comando |
|---|---|
| Turno headless con eventos JSON | `opencode run --format json [-m proveedor/modelo]` |
| Prompt | por **stdin** hasta EOF (verificado); también acepta argumento |
| Continuar la sesión | `opencode run --format json -s <sessionID>` (también `-c` = última sesión, `--fork`) |
| Abrir la sesión en la TUI (attach) | `opencode -s <sessionID>` con el directorio como `project` |
| Versión | `opencode --version` → `1.18.34` |
| Modelos disponibles | `opencode models` → una línea `proveedor/modelo` |
| Ejecutable nativo en Windows | `<npm>/node_modules/opencode-ai/bin/opencode.exe` (el shim `opencode.cmd` solo lo invoca; Claude y Codex ya se resuelven igual, evitando el `.cmd`) |

Sin `-m`, `run` usa un modelo por defecto y funciona.

### Formato del stream (`--format json`)

Un JSON por línea, todos con `sessionID`. Tipos vistos: `step_start`, `text`, `tool_use`, `step_finish`, `error`.

```jsonl
{"type":"step_start","sessionID":"ses_…","part":{"type":"step-start","snapshot":"…"}}
{"type":"text","sessionID":"ses_…","part":{"type":"text","text":"ok"}}
{"type":"tool_use","sessionID":"ses_…","part":{"type":"tool","tool":"bash","callID":"call_…","state":{"status":"completed","input":{"command":"echo done"},"output":"done\n","metadata":{"exit":0}}}}
{"type":"step_finish","sessionID":"ses_…","part":{"type":"step-finish","reason":"stop","tokens":{"total":28658,"input":217,"output":25,"reasoning":0,"cache":{"write":0,"read":28416}},"cost":0}}
```

- Un turno son **varios pasos**: los que cierran con `reason: "tool-calls"` continúan; el último cierra con `reason: "stop"` y el proceso sale con código 0.
- `tool_use` llega **ya terminado**: `state.status` es `completed` o `error` (no hay evento «pedido» aparte).
- Herramientas vistas: `write`, `edit`, `read`, `glob`, `bash`. El comando sale de `state.input.command` y el código de salida de `state.metadata.exit` (un `exit 3` llega como `completed` con `exit: 3`: el éxito hay que derivarlo del código, no del estado).
- El archivo editado sale de `state.input.filePath`.
- Uso por paso en `step_finish.tokens`, con caché separada.

### Sesión y persistencia de contexto

- El id (`ses_…`) lo **asigna OpenCode**; no se puede fijar uno. Sale en el primer evento (no por stderr, a diferencia de Kimi).
- **Verificado:** un segundo `run -s <id>` recordó lo dicho en el primer turno. El contexto persiste en el almacenamiento de OpenCode (`~/.local/share/opencode`); Symphony no necesita reenviar la conversación entre turnos del mismo proveedor.
- Entre proveedores aplica el handoff genérico de Symphony, igual que con los demás.

### Errores

Un modelo inexistente o un proveedor desconocido sale por stdout, con código de salida 1 y sin nada en stderr:

```json
{"type":"error","sessionID":"ses_…","error":{"name":"UnknownError","data":{"message":"Unexpected server error. Check server logs for details.","ref":"err_…"}}}
```

El mensaje es genérico: **no distingue «modelo no existe» de otras fallas**. Cuota, rate limit y auth **no se pudieron provocar** con los modelos gratuitos; las formas esperadas (`error.name` + `error.data.message/statusCode`) son sintéticas hasta verlas en vivo, como se hizo con Kimi y Antigravity en P11.

### Modelos y credenciales

- `opencode models` listó 10 modelos gratuitos del proveedor propio `opencode/` (p. ej. `opencode/big-pickle`).
- `opencode providers list` muestra **una credencial: GitHub Copilot (OAuth)**. Es la misma cuenta que usa el proveedor Copilot de Symphony; conviene revisar `docs/research/tos.md` antes de usarla desde OpenCode (no revisado).
- Con más credenciales, `opencode models` puede listar cientos de modelos.

### Permisos

En `run` las herramientas (`write`, `bash`) **se ejecutaron sin pedir permiso** con la config por defecto. No hay sandbox; el mismo caso que Kimi. Existe `--auto` («aprobar permisos no denegados»), no hizo falta.

---

## 3. Cómo encajaría (propuesta, sin implementar)

Sigue el patrón de P11 (ADR-0008, `docs/research/cli-p11.md`): un crate por proveedor detrás de `ProviderAdapter`; el core no cambia.

| Pieza | Propuesta |
|---|---|
| Crate | `crates/adapters/opencode` (`symphony-adapter-opencode`), sin dependencias nuevas (solo `serde_json`, `symphony-core`, `symphony-adapter-common`, como Kimi) |
| `provider_id` / `cli_name` | `opencode` / `opencode` |
| Turno | `run --format json`, prompt por stdin, `close_stdin_after_prompt = true`; `resume` con `-s`; mensajes a media tarea no soportados (van en el siguiente turno, como Codex y Kimi) |
| Eventos | `step_start` → `SessionStarted` (id); `text` → `AssistantText`; `tool_use` → `ToolRequested` + (`FileModified` si edita) + `ToolFinished`; `step_finish` → `TurnUsage` (+ `TurnFinished` si `reason == "stop"`); `error` → `ProviderError` |
| Id de modelo | El id canónico no admite dos `/` (la TUI y `attach` parten por la primera). `opencode/big-pickle` queda igual y los de otros proveedores serían `opencode/<proveedor>:<modelo>`; el `cli_model_id` conserva `proveedor/modelo`. Más un `opencode/default` sin `-m` |
| Descubrimiento | `opencode models` con tope de tiempo (no puede colgar el arranque del daemon) y tope de modelos registrados |
| Cableado | miembro del workspace, dependencia del daemon, `providers::builtin()` y `builtin_arc()`, y una fila de puntajes iniciales en `routing.rs` |
| Verificación | fixtures sanitizados + suite de contrato (L1), `fake-agent` ya cubre el runtime (L2); una prueba live con un modelo gratuito de OpenCode no gasta la cuota de Leo |
| Docs | `docs/research/cli-opencode.md`, QUICKSTART (permisos y límites), README |

**Decisiones de Leo (2026-10-10), registradas en [ADR-0012](../adr/0012-opencode-como-proveedor.md):**
1. ADR corto, no un paso suelto del PLAN.
2. Esquema de ids `opencode/<proveedor>:<modelo>`: aprobado.
3. Failover automático desde el inicio, sin esperar a validarlo (§ 4).

---

## 4. Calidad de los proveedores: lo que funciona y lo que hay que volver a probar

### Estado según Leo

| Proveedor | Estado | Origen |
|---|---|---|
| **Claude Code** | Funciona bien | Uso diario de Leo |
| **Codex** | Funciona bien | Uso diario de Leo |
| Kimi, Antigravity, Copilot | **Volver a probar** | Integrados en P11 con pares de handoff verificados, pero no validados por uso real |
| OpenCode | Sin integrar | Esta petición |

### Evidencia que ya hay (no la sustituye: Leo manda)

De `docs/research/uso-signalia.md` (30 agentes, 3–5 oct):

| Proveedor | Lo que pasó |
|---|---|
| Claude (Opus, Sonnet) | 14 de 22 runs terminaron bien; 4 cayeron con la cuota (`NO_HEARTBEAT`), 3 se cambiaron a mano y 1 sigue `RUNNING` |
| Codex (gpt-5.6-sol) | 11 de 14 runs terminaron bien |
| Kimi | 0 de 2: un `NO_HEARTBEAT` a los 15 min y una cuota diaria al minuto; en ambos Sonnet terminó el trabajo |
| Antigravity (Gemini) | El agente #10 «completó» en 39 s sin escribir nada; en headless no ejecuta comandos |
| Copilot | Solo `auto` en esta cuenta; una sesión fuera de reglas empujó a un repo público (fuera de Symphony) |

De P11 (`docs/phases/P11-proveedores.md`): Kimi no tiene sandbox, Antigravity y Copilot no ejecutan comandos sin `skip_permissions`/`allow_all_tools`, y las skills nativas de los tres no funcionan en el chat.

### Propuesta de re-prueba (a confirmar con Leo)

Un proveedor pasa a «validado» cuando cumple todo esto en una tarea real de tamaño medio:

1. Termina una tarea que **escribe archivos y ejecuta comandos** sin intervención.
2. **Continúa** (`resume`) una sesión y recuerda lo anterior.
3. Entra y sale de un **handoff** con otro proveedor sin perder el hilo.
4. Reporta **fallas clasificadas** (cuota, rate limit, auth) y Symphony hace el failover correcto.
5. Aguanta una sesión **larga** (≥ 100 mensajes) y termina limpio.

Los niveles L1/L2 ya existen (fixtures y `fake-agent`); lo que falta es L3 con cuota real, que requiere el permiso de Leo (CLAUDE.md).

### Efectos posibles en el producto (ideas, no decisiones)

- Marcar en la vista de Providers cuáles están **validados** y cuáles **por re-probar**.
- Que el failover automático **solo elija proveedores validados** por defecto; el resto queda por elección manual hasta que Leo los valide.
- Los puntajes iniciales del router para Kimi, Antigravity, Copilot y OpenCode son estimaciones sin evidencia (`routing.rs`); no deberían pesar más que los de Claude y Codex.

---

## 5. Actualización 2026-10-11: ya está integrado

OpenCode se integró (ADR-0012, bitácora P11 § S7, [cli-opencode.md](cli-opencode.md)) y se probó en vivo con modelos gratuitos: escribe en su worktree (con las dos correcciones de § 2 que no se conocían), `resume` conserva el contexto, el handoff entre dos modelos de OpenCode funciona y el broker MCP responde. **Sigue sin validarse como proveedor de uso diario:** no se probaron cuota ni auth reales, ni el handoff con Claude o Codex, y sus modelos gratuitos fallaron por upstream (429).

## 6. Límites de este registro

- No se corrió ninguna prueba con la cuota de Claude, Codex ni Copilot; todo lo de OpenCode salió de modelos gratuitos.
- No se probó cuota, rate limit ni auth de OpenCode.
- No se probó el cambio de proveedor desde y hacia OpenCode (handoff).
- No se midió la calidad de ningún modelo de OpenCode; los 10 listados son los gratuitos de su proveedor propio.
- Los fixtures capturados no se conservan en el repo (se revirtieron con el adapter); las formas están descritas arriba y se pueden volver a capturar con un modelo gratuito.
