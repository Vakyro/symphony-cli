# P00.S0 · Prevalidación

**Fecha:** 2026-09-24 · **Agente:** claude-code/opus-5.5 · **Permiso de Leo:** sí (usar suscripciones reales)
**Máquina:** Windows 11 Pro · Claude Code 2.1.281 · Codex CLI 0.154.0 · Node 22.11 · Git 2.47

---

## 1. Herramientas existentes

### 1.1 Qué se revisó y cómo

Investigación web (listas curadas y READMEs), sin instalar nada. **No se probó ninguna a mano**, por dos razones:
- Claude Squad depende de tmux: en Windows solo corre dentro de WSL, y para eso hay que instalar tmux, Go y los CLIs dentro de Ubuntu. Eso es instalar software de sistema, así que hay que preguntarle a Leo.
- La conclusión no depende de probarlas: ninguna declara hacer lo que diferencia a Symphony (ver 1.3).

Fuente principal: [awesome-agent-orchestrators](https://github.com/andyrewlee/awesome-agent-orchestrators), con decenas de herramientas en "Parallel Coding Agents — Terminal" y "Desktop & Web".

### 1.2 Comparación contra IDEA §3 y §5

| Herramienta | Worktree por agente | Varios proveedores | Handoff a **otro** proveedor | Scheduler de recursos | Windows nativo | Notas |
|---|---|---|---|---|---|---|
| [Claude Squad](https://github.com/smtg-ai/claude-squad) | ✅ | ✅ Claude, Codex, Gemini, Aider | ❌ | ❌ | ❌ (tmux) | Go, AGPL-3.0. TUI sobre tmux; cada sesión es la TUI nativa del CLI |
| [AgentBridge](https://github.com/raysonmeng/agent-bridge) | ❌ | Claude + Codex (en pareja) | ⚠️ **No**: pausa en el límite de cuota y retoma con el **mismo** proveedor cuando se renueva la ventana | ❌ | ❌ ("not officially supported") | La más cercana. Escribe un `.agent/checkpoint.md` al cortar en el límite de turno (companion `agent-quota-guard`). Los agentes conversan entre sí (Symphony lo prohíbe, IDEA §5.12) |
| [Vibe Kanban](https://vibekanban.com/) | ✅ | ✅ | ❌ | ❌ | ✅ (web UI local) | Tablero kanban + revisión de diff en navegador |
| [Conductor](https://conductor.build) | ✅ | ✅ Claude, Codex, OpenCode | ❌ | ❌ | ❌ (macOS) | App nativa de macOS |
| [Parallel Code](https://github.com/johannesjo/parallel-code) | ✅ | ✅ Claude, Codex, Gemini | ❌ | ❌ | ? | Lado a lado, un worktree por agente |
| [agent-deck](https://github.com/asheshgoplani/agent-deck), [agent-console](https://github.com/buhuipao/agent-console) | ❌ / parcial | ✅ | ❌ | ❌ | ? | Dashboards de sesiones: estado y resume nativo de cada CLI |
| [Paperclip](https://github.com/paperclipai/paperclip/issues/2014) | — | ✅ | ⚠️ Solo **propuesta** abierta: fallback Claude → Codex al agotar cuota | ❌ | — | Hoy, al agotar cuota, "registra un run fallido y se detiene" |

### 1.3 Conclusión

- **Worktrees y varios agentes en paralelo ya están resueltos** por muchas herramientas. No es diferencial; Symphony tiene que hacerlo bien, pero no lo inventa.
- **Lo que nadie hace (según lo publicado):**
  1. **Handoff a otro proveedor en caliente** con un checkpoint incremental (AGENT ≠ MODEL). Lo más cercano (AgentBridge) espera a que se renueve la cuota del mismo proveedor. En Paperclip está pedido, pero no implementado.
  2. **Scheduler de recursos** (clases 0–4 + Job Objects/cgroups) para que 3 agentes no traben la máquina. Ninguna herramienta revisada limita builds ni tests.
  3. **Soporte serio en Windows.** La mayoría depende de tmux o de macOS.
- **Gate de la parte 1: pasa.** Ninguna herramienta cubre la mayor parte de IDEA §3. El diferencial real es el handoff entre proveedores + el scheduler. Esto refuerza que P06 (handoff) y P08 (scheduler) son el corazón del producto, y que P02–P05 (infraestructura de worktrees y sesiones) debe ser lo más delgada posible.
- **Recomendación:** antes de P05, leer cómo resuelven Claude Squad y agent-console la captura de sesiones y el resume nativo (ideas para ADR-0005, modo de interacción), y cómo `agent-quota-guard` detecta el límite de cuota (ideas para `parse_error`, P10).

---

## 2. Test D manual

### 2.1 Montaje

- **Repo de prueba:** Node ESM sin dependencias, tests con `node:test` (`npm test`). Un commit inicial con un smoke test.
- **Tarea** ([`prevalidacion/task.md`](prevalidacion/task.md)): módulo de usuarios con `email.js`, `password.js` (scrypt), `users.js` (`UserStore` con `register`/`login`), 3 archivos de tests y exports en `index.js`. Son unos 7 archivos, y a un agente solo le toma de 1 a 3 minutos.
- **Modo:** headless en los dos lados. `claude -p --output-format stream-json --permission-mode acceptEdits` con una allowlist de Bash, y `codex exec --json -s workspace-write`.
- **Kill** ([`prevalidacion/runkill.ps1`](prevalidacion/runkill.ps1)): cuando el worktree tiene 3 archivos nuevos, `taskkill /F /T` sobre el shim `.cmd` de npm, sin cleanup.
- **Checkpoint** ([`prevalidacion/checkpoint.mjs`](prevalidacion/checkpoint.mjs)): el prompt del sucesor se arma **solo** con el objetivo, `git status` y `git diff`, el contenido de los archivos nuevos, el último comando con su resultado, y los TODOs y el último mensaje del asistente sacados del transcript. No lleva historial de chat, resumen del modelo ni explicación humana. Los prompts reales están en [`run1.handoff.md`](prevalidacion/run1.handoff.md) y [`run2.handoff.md`](prevalidacion/run2.handoff.md).
- **Modelos:** Claude Code → `claude-opus-5-5`. Codex → `gpt-5.6-sol` (el default de `~/.codex/config.toml`).

### 2.2 Resultados

| Corrida | Muere | Deja | Sucesor | ¿Continúa sin reexplicar? | ¿Rehace trabajo? | Tests al final | Tiempo del sucesor |
|---|---|---|---|---|---|---|---|
| 1 · Claude → Codex | Claude a los 48 s | `email.js`, `email.test.js`, `password.js` (sin test) | Codex | ✅ "Retomo desde el estado existente… sin rehacer lo que ya pasa" | No: `email.js` y su test idénticos; `password.js` con 1 línea cambiada | ✅ 32/32 | 5 min 20 s, 26 comandos |
| 2 · Codex → Claude | Codex a los 1 min 53 s | `email.js`, `email.test.js`, `password.test.js` (sin implementación) | Claude | ✅ "email hecho, `test/password.test.js` escrito pero falta `src/password.js`" | No: los 3 archivos idénticos | ✅ 28/28 | 1 min 8 s, 10 turnos |

Ningún sucesor debilitó tests. En la corrida 1, Codex **amplió** el smoke test para verificar los exports.
Ningún kill dejó procesos huérfanos: `taskkill /F /T` sobre el `.cmd` mató todo el árbol cmd → node.

**Veredicto: AGENT ≠ MODEL se sostiene en los dos sentidos** con un checkpoint que no le pide nada al modelo que muere.

**Límites de esta prueba** (P01.S6 los cubre):
- Es una sola tarea, pequeña y bien especificada. Falta probar con tareas medianas, donde el "qué seguía" pesa más que el diff.
- El kill cayó cerca de un límite de archivo. Falta un kill a mitad de una edición o durante un test largo.
- Todo fue headless. No se probó el modo interactivo.

### 2.3 Hallazgos que cambian o precisan el spec

| # | Hallazgo | Impacto |
|---|---|---|
| H1 | **Claude en `-p` no usó TodoWrite** aunque la tarea lo pedía; Codex en `exec` tampoco produjo `todo_list`. Los dos escribieron el plan como **texto** en sus mensajes | IDEA §5.5 asume una lista de TODOs en el transcript. El checkpoint tiene que tomar el **último mensaje del asistente** como fuente del "qué seguía", y usar TODOs estructurados solo si existen |
| H2 | **El transcript de Claude va detrás del disco** tras un kill: `password.js` estaba escrito, pero su `Write` no aparecía en el `.jsonl` | Confirma que `git diff` + archivos nuevos son la fuente de verdad y el transcript solo complementa. Si hay conflicto, gana git |
| H3 | **El stream de `claude -p` trae un evento de rate limit** con `five_hour.utilization`, `seven_day.utilization` y `resetsAt` | La cuota de Claude Code puede ser `KNOWN`, no solo `ESTIMATED` (IDEA §5.8, FLOW §12.3, DB `quota_certainty`). Hay que confirmarlo en P01.S2 y documentarlo en `cli-claude-code.md` |
| H4 | **Windows: lanzar el CLI desde Git Bash bloquea al padre** hasta que el hijo termina, porque el hijo hereda el pipe de stdout. Con `Start-Process -WindowStyle Hidden` + redirección a archivo se resuelve | Va a `LEARNINGS.md` en P00.S4. Relevante para P04 (`ProcessSupervisor`): los handles heredables tienen que controlarse explícitamente |
| H5 | **El formato de sesión en disco de Codex** (`~/.codex/sessions/.../rollout-*.jsonl`, con `response_item`/`event_msg`) **es distinto** del de `codex exec --json` (`item.*`) | El adapter de Codex necesita dos parsers si soporta el modo interactivo (ADR-0005) |
| H6 | **Codex gastó unos 880k tokens de entrada (835k cacheados)** para terminar una tarea pequeña; Claude, unos 243k (cacheados). Codex cargó las skills globales de `~/.agents/skills` | El costo del handoff depende de la configuración global de cada CLI, no solo del prompt. Hay que medirlo en P01.S3 y considerarlo en el Context Engine (P09) |
| H7 | La salida TAP completa de `npm test` entró cruda al handoff (unos 2.5 KB recortados) | Da evidencia temprana a favor del colapso de logs de tests (IDEA §5.6, P09.S3). No bloquea |

---

## 3. Decisión del gate P00.S0

| Condición de parada | Resultado |
|---|---|
| ¿Una herramienta existente cubre la mayor parte de IDEA §3? | **No.** Worktrees y paralelismo sí existen; el handoff a otro proveedor, el scheduler de recursos y Windows no |
| ¿El Test D manual falla en ambos sentidos? | **No.** Pasa en ambos sentidos |

**Recomendación: seguir con P00.S1.** Pendiente: **aprobación de Leo** (Verifica de P00.S0).

**Queda fuera, a propósito:** la prueba práctica de Claude Squad. Requiere instalar tmux y los CLIs dentro de WSL, y no cambiaría la decisión. Si Leo la quiere, es un paso aparte con su permiso.
