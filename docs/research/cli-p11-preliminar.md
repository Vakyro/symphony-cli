# CLIs de P11 — reconocimiento preliminar (2026-10-01)

Evidencia **solo de `--version`, `--help` y `agy models`** (no gastan cuota; no se leyeron credenciales ni se ejecutó ningún prompt). Es el punto de partida de P11.S1; los puntos marcados «sin verificar» exigen pruebas en vivo con permiso de Leo.

Los tres CLIs ya están instalados y con sesión iniciada en la máquina de Leo, igual que OpenCode (`1.15.13`, fuera del PLAN).

| | Kimi Code | Copilot CLI | Antigravity (`agy`) |
|---|---|---|---|
| Versión | 1.44.0 (`kimi`) | 1.0.60 (`copilot`) | 1.2.11 (`agy`) |
| Turno headless | `--print -p <texto>` | `-p <texto>` | `--print` / `-p` |
| Salida estructurada | `--output-format stream-json` (+ `--final-message-only`) | `--output-format json` (JSONL) | `--output-format json` o `stream-json` |
| Entrada multi-turno por stdin | `--input-format stream-json` | no visto | `--input-format stream-json` (un NDJSON por línea, un turno cada uno) |
| Resume | `-S/-r <id>`, `-C` (último) | `--resume[=id]`, `--session-id <id>` (**permite fijar el UUID de una sesión nueva**), `--continue` | `--conversation <id>`, `-c` (última) |
| Directorio de trabajo | `-w <dir>` | cwd + `--add-dir` | cwd + `--add-dir` |
| Permisos sin preguntar | `--yolo` | `--allow-all-tools` / `--yolo` / `--no-ask-user` | `--dangerously-skip-permissions`, `--mode accept-edits` |
| Modelo | `-m` | `--model` (`auto` existe) | `--model`, `--effort`; `agy models` lista los modelos |
| MCP / skills | `--mcp-config(-file)`, `--skills-dir` | `--additional-mcp-config`, `~/.copilot/skills` | `agy mcp`, `~/.gemini/skills` |
| Otros | servidor ACP (`kimi acp`), `export` de sesión | `--share` a `.md`, `--log-dir` | `--json-schema`, `--print-timeout` |
| Estado local | `~/.kimi/sessions` | `~/.copilot/session-state` | `~/.gemini/antigravity-cli` |

## Lectura para el contrato `ProviderAdapter`
- Los tres cubren `spawn_spec`, `resume_spec` y `encode_prompt` con flags documentados en `--help`. Ninguno exige PTY para un turno.
- **Hooks / Test C (`hooks_can_hold`):** el `--help` de los tres no menciona hooks. Sin verificar. Solo importa para el scheduler (P08); el chat no lo necesita (`supports_hooks() = false` y `hooks_can_hold() = None` ya son valores válidos del trait).
- `agy models` incluye modelos de Google, **Claude Sonnet/Opus 4.6 y GPT-OSS**: revisar ToS y cómo cuenta la cuota antes de ofrecerlo como proveedor separado de Claude Code (`tos.md`).

## Sin verificar (requiere live, con permiso de Leo)
1. Esquema real de eventos de `stream-json` / `json` (mensajes, tool calls, uso de tokens, id de sesión).
2. De dónde sale el id de sesión para `resume` (evento, archivo o flag como `--session-id` de Copilot).
3. Texto de error por cuota agotada / rate limit / auth, para `parse_error`.
4. Comportamiento en Windows (cierre de stdin, EPIPE, salida de procesos hijos).
5. Costo en tokens de abrir una sesión nueva (en Codex fueron ~18k; en Claude ~27k).
6. ToS de cada uno (`tos.md`; Leo confirma).
