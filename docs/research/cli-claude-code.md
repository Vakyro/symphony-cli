# Contrato del CLI: Claude Code

- **Versión instalada:** Claude Code 2.1.281 (Windows 11)
- **Fecha:** 2026-09-24 · **Agente:** claude-code/opus-5.5 · **Paso:** P01.S2
- **Fuentes:** [hooks](https://code.claude.com/docs/en/hooks), [headless](https://code.claude.com/docs/en/headless), [cli-reference](https://code.claude.com/docs/en/cli-reference), [model-config](https://code.claude.com/docs/en/model-config), `claude --help`, prevalidación P00.S0.

Todo lo que dice **(docs)** está sacado de la documentación y todavía no se probó. Test B, C y D lo verifican.

## 1. Eventos de hooks

Hay tres cadencias (docs): por sesión, por turno y por cada tool call.

| Evento | Cuándo | ¿Bloquea? | Uso en Symphony (IDEA §5.3) |
|---|---|---|---|
| `SessionStart` | Inicio o resume (matcher: `startup`/`resume`/…) | No | `AgentStarted` |
| `UserPromptSubmit` | Antes de procesar un prompt | Sí (`decision: block`) | `TurnStarted` |
| `PreToolUse` | Antes de cada tool call | **Sí**: `permissionDecision` `allow`/`deny`/`ask`/`defer`, o exit 2 | `ToolRequested` + **retención del scheduler** |
| `PostToolUse` | Después de un tool call exitoso | Parcial (`decision: block` realimenta al modelo) | `CommandFinished`, `FileModified` |
| `PostToolUseFailure` | Después de un tool call fallido | — | `CommandFinished` (fallido) |
| `Stop` | Cuando Claude termina de responder | Sí (puede forzar a continuar) | `TurnFinished` |
| `StopFailure` | El turno termina por un error de API | No | **`ProviderError` tipado** (ver §7) |
| `SubagentStop`, `PreCompact`/`PostCompact`, `Notification`, `SessionEnd` | … | — | Opcionales |

También existen `PreModelSwitch`/`PostModelSwitch`, `PermissionRequest`/`PermissionDenied`, `TaskCreated`/`TaskCompleted`, `WorktreeCreate`/`WorktreeRemove`, `CwdChanged`, `ConfigChange` y `Elicitation`.

**Campos comunes del stdin** (docs): `session_id`, `transcript_path`, `cwd`, `hook_event_name` (y `permission_mode` en la mayoría). `PreToolUse` agrega `tool_name`, `tool_input` y `tool_use_id`.

Hay cinco tipos de handler: `command`, `http`, `mcp_tool`, `prompt` y `agent`. Symphony usa **solo `command`**, que apunta a `symphony hook emit`.

## 2. ¿Un hook puede bloquear o esperar? Timeout

- **Timeout por handler:** campo `timeout` en segundos. Por defecto: **600 s** para `command`/`http`/`mcp_tool`, pero **30 s** en `UserPromptSubmit`/`PreModelSwitch`, **10 s** en `MessageDisplay` y **1.5 s** en `SessionEnd` (docs). La documentación no publica un máximo. Test C (P01.S5) lo mide.
- **Qué pasa si se vence** (docs, §Timeouts): el hook `command` se cancela, su salida se descarta y **en `PreToolUse` la herramienta sigue** por el flujo normal de permisos. **Retener por hook falla en modo "abierto"**: un hook que se cuelga no sirve de compuerta.
  - Consecuencia para P08: si el scheduler retiene un comando, tiene que devolver una decisión **antes** del timeout. Symphony debe fijar `timeout` explícito en el hook, mayor que la espera máxima del scheduler, y devolver `deny` con razón (o `allow`) antes de que se venza.
- `async: true` corre el hook en segundo plano sin bloquear (no sirve para retener).

## 3. Dónde se configuran e inyección por worktree sin commitear

Orden de fuentes: `~/.claude/settings.json` (user), `.claude/settings.json` (project), `.claude/settings.local.json` (local, no se commitea) y managed.

Para Symphony, la mejor opción es **`--settings <archivo-o-json>`**: sobrescribe claves para esa sesión sin escribir nada en el worktree (máximo 2 MiB). Así no hace falta tocar `.gitignore` ni `settings.local.json`. Test B verifica que los hooks pasados por `--settings` corren en `-p`.

Plan B: `<worktree>/.claude/settings.local.json` + `.git/info/exclude`.

**Confianza del workspace** (docs): en `-p` y el SDK, la carpeta **se trata como confiable** y los hooks de settings corren sin diálogo. En modo interactivo, los hooks esperan a que se acepte el diálogo de confianza. En modo interactivo sobre un worktree nuevo, Symphony tiene que contemplar que aparezca ese diálogo.

`--setting-sources user,project,local` permite limitar qué settings se cargan.

## 4. Headless y streaming

- `claude -p "<prompt>" --output-format stream-json --verbose` produce JSONL. Primer evento: `system/init` (modelo, tools, MCP, plugins). Con `--include-partial-messages` también llegan los tokens parciales.
- `--input-format stream-json` (solo con `-p`) **acepta mensajes de usuario por stdin mientras la sesión corre**. Si un turno está en curso, el mensaje queda en cola y abre un turno nuevo (docs de `--max-turns`). Esto responde "cómo le habla Leo a un agente headless a media tarea" sin reiniciar el proceso.
- `--replay-user-messages` hace eco de los mensajes de stdin (para confirmar que llegaron).
- `--permission-mode acceptEdits|plan|bypassPermissions|…`, `--allowedTools`, `--max-turns`.
- **`--bare` no sirve para Symphony:** omite la autodetección de hooks, skills, MCP y CLAUDE.md, pero **no usa el login de la suscripción** (exige `ANTHROPIC_API_KEY`). Symphony corre sobre la suscripción de Leo, así que usa `-p` sin `--bare` y controla los hooks con `--settings`.
- H4 (LEARNINGS): en Windows, el padre tiene que controlar qué handles heredan los hijos.

## 5. Transcript y sesión

- Ruta: `~/.claude/projects/<slug-del-cwd>/<session_id>.jsonl`. Llega en `transcript_path` de cada hook.
- Formato: JSONL de mensajes. **No es una interfaz estable.**
- H2 (prevalidación): tras un kill, el transcript va **detrás del disco**. Fuente de verdad: git.
- Resume: `--resume <session_id | ruta.jsonl>` o `--continue`. `--session-id <uuid>` fija el ID al crear la sesión, así Symphony lo conoce de antemano. `--fork-session` crea un ID nuevo al reanudar.
- Tras un SIGTERM, al reanudar, Claude continúa el turno que quedó a medias (docs).

## 6. Modelos

- `--model <alias | nombre completo>`. Alias: `default`, `best`, `fable`, `opus`, `sonnet`, `haiku`, `sonnet[1m]`, `opus[1m]`, `opusplan`.
- No hay un comando que liste los modelos. El modelo real se lee de `system/init` (stream) y de `PostModelSwitch`.
- `--fallback-model <model>` es un fallback nativo cuando el modelo principal está sobrecargado (solo `-p`).

## 7. Errores, rate limit y cuota

- **`StopFailure`** (hook) y **`system/api_retry`** (stream) traen `error` tipado: `rate_limit`, `overloaded`, `authentication_failed`, `billing_error`, `oauth_org_not_allowed`, `account_on_hold`, `invalid_request`, `model_not_found`, `server_error`, `max_output_tokens`, `cloud_credential_error`, `unknown`. `api_retry` también trae `attempt`, `max_retries`, `retry_delay_ms` y `error_status`. **`parse_error` (P10) puede mapear esto directo, sin regex.**
- **Cuota (H3):** el stream de `-p` emite un evento de rate limit con `five_hour.utilization`, `seven_day.utilization` y `resetsAt`. Entonces la cuota de Claude Code puede ser **`KNOWN`** (DB `quota_certainty`). Falta capturar el payload exacto en Test B y guardarlo como fixture.

## 8. Modo de interacción (insumo para ADR-0005)

| Modo | Cómo | Pros | Contras |
|---|---|---|---|
| (a) Headless + vista propia | `-p --input-format stream-json --output-format stream-json` | Symphony ve todos los eventos; se puede mandar mensajes a media tarea por stdin; no hace falta PTY | Hay que reimplementar la vista de conversación; las confirmaciones de permisos se resuelven con `--permission-mode` y hooks |
| (b) PTY embebida | TUI de Claude dentro de ratatui | Experiencia nativa | Colores, redimensionado y teclas: frágil; sin eventos estructurados salvo hooks |
| (c) Headless + attach | Claude trae **sesiones en segundo plano**: `claude --bg`, `claude attach <id>`, `claude logs <id>`, `claude stop <id>` y un supervisor propio (`claude daemon status`) | Leo abre la TUI oficial sobre la misma sesión cuando quiere | Symphony compite con el supervisor de Claude por el ciclo de vida del proceso; hay que ver si `--bg` emite hooks igual |

Cambiar de modo en la misma sesión: `--resume <id>` en interactivo retoma una sesión creada en `-p` (misma sesión en disco). Se verifica en P01.S8.

## 9. Pendiente de verificar en el spike

- [ ] Hooks inyectados con `--settings` corren en `-p` (Test B).
- [ ] Payload exacto del evento de rate limit del stream (Test B → fixture).
- [ ] Máximo real de `timeout` en `PreToolUse` y comportamiento del modelo tras una espera larga (Test C).
- [ ] `--input-format stream-json` acepta un mensaje a media tarea (P01.S8, ADR-0005).
