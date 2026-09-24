# Contrato del CLI: Codex

- **Versión instalada:** codex-cli 0.154.0 (Windows 11). Modelo por defecto en `~/.codex/config.toml`: `gpt-5.6-sol`.
- **Fecha:** 2026-09-24 · **Agente:** claude-code/opus-5.5 · **Paso:** P01.S2
- **Fuentes:** [hooks](https://developers.openai.com/codex/hooks), [non-interactive](https://developers.openai.com/codex/noninteractive), [config-reference](https://developers.openai.com/codex/config-reference), `codex --help`, `codex exec --help`, `codex features list`, un rollout local de la prevalidación. **No se leyó `auth.json`** (PLAN §2.9).

Todo lo que dice **(docs)** está sacado de la documentación y todavía no se probó.

## 1. Eventos de hooks

`codex features list` → `hooks  stable  true`. Están activos por defecto.

| Evento | ¿Bloquea? | Uso en Symphony (IDEA §5.3) |
|---|---|---|
| `SessionStart` (matcher `startup`/`resume`) | No | `AgentStarted` |
| `UserPromptSubmit` | Sí | `TurnStarted` |
| `PreToolUse` | **Sí**: `permissionDecision: deny`, `decision: block` (legado) o exit 2. Admite `updatedInput` con `allow` | `ToolRequested` + retención del scheduler |
| `PermissionRequest` | Sí | — |
| `PostToolUse` | — | `CommandFinished`, `FileModified` |
| `Stop` | Sí (`decision: block` = **seguir** con `reason` como prompt nuevo) | `TurnFinished`. Trae `last_assistant_message`: **fuente directa del "qué seguía" para el checkpoint (H1)** |
| `Interrupt` | — | Interrupción manual del turno |
| `SubagentStart`/`SubagentStop`, `PreCompact`/`PostCompact`, `SessionEnd` | — | Opcionales |

**Campos comunes** (docs): `session_id`, `transcript_path` (puede ser `null`), `cwd`, `hook_event_name` y **`model`**, que es una extensión de Codex (el modelo activo llega en cada evento). Los hooks de turno traen `turn_id`; `PreToolUse` trae `tool_name`, `tool_use_id` y `tool_input`.

**Cobertura de `PreToolUse`** (docs): shell y `exec_command` (matcher `Bash`), `apply_patch` (matcher `apply_patch`/`Edit`/`Write`), MCP y tools locales. **No cubre** las herramientas alojadas, como `WebSearch`. La documentación advierte: "a useful guardrail, not a complete enforcement boundary". Para P08, la capa del SO sigue siendo obligatoria.

`permissionDecision: "ask"`, `continue: false` y `stopReason` en `PreToolUse` "se parsean pero todavía no se soportan": el hook se marca como fallido y **el tool call continúa**.

## 2. ¿Puede esperar? Timeout

- `timeout` en segundos. Por defecto: **600 s** (docs).
- No está documentado qué pasa con un `PreToolUse` que se vence. Todo indica que falla en modo "abierto", como los errores ("Errors … don't block the operation" en MCP hooks). **Test C lo mide.**

## 3. Dónde se configuran e inyección por worktree

- Fuentes (docs): `~/.codex/hooks.json`, `[hooks]` en `~/.codex/config.toml`, `<repo>/.codex/hooks.json` y `<repo>/.codex/config.toml`. **Se suman, no se reemplazan.**
- Los hooks del proyecto cargan **solo si la capa `.codex/` del proyecto es de confianza** (`projects.<path>.trust_level`).
- **Confianza por hash:** cada hook que no es managed hay que revisarlo y aprobarlo (`/hooks`). Codex guarda la aprobación contra el hash de la definición: si el hook cambia, se salta hasta que se vuelva a aprobar. Para automatización está `--dangerously-bypass-hook-trust`, que corre los hooks habilitados sin confianza persistida en esa invocación.
- **Opción para Symphony:** `<worktree>/.codex/hooks.json` (excluido con `.git/info/exclude`) + `--dangerously-bypass-hook-trust` en cada ejecución que lanza Symphony. Es seguro porque Symphony genera el archivo. Alternativa sin archivo: `-c 'hooks.PreToolUse=[…]'` (el override `-c` parsea TOML), pero hay que probar si `-c` acepta tablas de hooks. **Test B decide.**
- **No usar `CODEX_HOME` alternativo:** ahí vive `auth.json`, y moverlo o copiarlo rompe la regla de credenciales.
- Aviso de H6: Codex carga las skills globales de `~/.agents/skills`. `--ignore-user-config` / `--ignore-rules` sirven para aislar una corrida (medir en Test A).

## 4. Headless y streaming

- `codex exec --json "<prompt>"` produce JSONL en stdout. Tipos (docs): `thread.started` (con `thread_id`), `turn.started`, `turn.completed` (con `usage`: `input_tokens`, `cached_input_tokens`, `output_tokens`, `reasoning_output_tokens`), `turn.failed`, `item.started`/`item.updated`/`item.completed` (`command_execution`, `agent_message`, `file_change`, `todo_list`, …) y `error`.
- Flags útiles: `-m <model>`, `-s read-only|workspace-write|danger-full-access`, `-C <dir>`, `--add-dir`, `--skip-git-repo-check`, `--ephemeral` (no guarda el rollout), `-o/--output-last-message <file>` y `--output-schema`.
- Resume: `codex exec resume <SESSION_ID | --last> "<prompt>"`.
- **Mensajes a media tarea:** `codex queue --thread <id> --message "<texto>"` encola un mensaje para una sesión existente. También está `codex app-server` (experimental), un servidor JSON-RPC que usan el IDE y la app de escritorio.

## 5. Transcript y sesión

- En disco: `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`, con líneas `session_meta`, `turn_context`, `response_item` y `event_msg`. **Es un formato distinto del stream de `exec --json`** (H5).
- La documentación dice que el formato del transcript **no es una interfaz estable**.
- Índice de sesiones: `~/.codex/session_index.jsonl`.

## 6. Modelos

- `-m <slug>` o `-c model="<slug>"`.
- No hay un subcomando para listar modelos. `~/.codex/models_cache.json` tiene la lista que ve la cuenta de Leo: `gpt-6-astra`, `gpt-reserve`, `gpt-5.6-sol`, `gpt-5.6-terra`, `gpt-5.6-luna`, `gpt-5.5` y `codex-auto-review`. Es una caché interna, así que sirve como pista y no como contrato.

## 7. Errores, rate limit y cuota

- **Cuota `KNOWN`:** el rollout en disco trae `event_msg` de tipo `token_count` con:
  ```json
  "rate_limits": {"limit_id":"codex","primary":{"used_percent":6.0,"window_minutes":300,"resets_at":1790275539},
                  "secondary":{"used_percent":32.0,"window_minutes":10080,"resets_at":1790479002},
                  "credits":{"has_credits":false,"unlimited":false,"balance":null}}
  ```
  Son la ventana de 5 h y la de 7 días, con porcentaje usado y reset. **Falta confirmar si `exec --json` emite lo mismo** en el stream. Si no, el adapter lee el rollout (Test B).
- Errores: `turn.failed` / `error` en el stream. Falta capturar un 429 o cuota agotada real como fixture. No se va a forzar a propósito: se captura si aparece durante el spike.

## 8. Modo de interacción (insumo para ADR-0005)

| Modo | Cómo | Nota |
|---|---|---|
| (a) Headless + vista propia | `codex exec --json` + `codex queue` para mensajes a media tarea | Hay que verificar que `queue` funcione sobre una sesión de `exec` activa |
| (b) PTY embebida | TUI de Codex dentro de ratatui | Mismas dudas que con Claude |
| (c) Headless + attach | `codex resume <id>` abre la TUI sobre la sesión | Solo después de que termine el `exec`: no hay un attach en vivo documentado |

## 9. Pendiente de verificar en el spike

- [ ] `hooks.json` en el worktree + `--dangerously-bypass-hook-trust` hacen que corran los hooks en `exec` (Test B).
- [ ] ¿`exec --json` trae `rate_limits`? (Test B).
- [ ] Comportamiento de un `PreToolUse` retenido 120 s y al vencer el timeout (Test C).
- [ ] `codex queue` sobre un `exec` en curso (P01.S8).
