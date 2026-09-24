# Test B · Event bus de hooks (P01.S4)

- **Fecha:** 2026-09-24 · **Agente:** claude-code/opus-5.5
- **Máquina:** la de Test A (Windows 11, i7-8650U, 16 GB).
- **CLIs:** Claude Code 2.1.281 (`--model haiku`) y codex-cli 0.154.0 (`-m gpt-5.6-luna`).
- **Herramienta:** `spikes/spike-hook`. `spike-hook collect <events.jsonl>` escucha en un socket local de `interprocess` (named pipe en Windows) y agrega líneas. `spike-hook hook <proveedor>` es el comando del hook: lee el payload de stdin, lo normaliza a los nombres de IDEA §5.3 y lo envía. **Sin `SYMPHONY_AGENT_ID` no hace nada** y nunca devuelve error al CLI.

## Resultado

**Verifica: ✅.** Una sesión de cada CLI produce `ToolRequested`, `CommandFinished`, `FileModified` y `TurnFinished` en `events.jsonl`.

| CLI evento | Normalizado | Claude | Codex |
|---|---|---|---|
| `SessionStart` | `AgentStarted` | ✅ | ✅ |
| `UserPromptSubmit` | `TurnStarted` | ✅ | ✅ |
| `PreToolUse` (Bash) | `CommandRequested` (tipo de `ToolRequested`) | ✅ | ✅ |
| `PreToolUse` (Write / `apply_patch`) | `ToolRequested` | ✅ | ✅ |
| `PostToolUse` (Bash) | `CommandFinished` | ✅ | ✅ |
| `PostToolUseFailure` (Bash) | `CommandFinished` (fallido) | ✅ | — (Codex no tiene este evento) |
| `PostToolUse` (Write / `apply_patch`) | `FileModified` | ✅ | ✅ |
| `Stop` | `TurnFinished` | ✅ | ✅ |
| `SessionEnd` | `AgentStopped` | ✅ | no probado |

Latencia del hook con envío por IPC: **mediana 20 ms, máximo 30 ms** (20 llamadas). Sin `SYMPHONY_AGENT_ID`: 0 eventos, exit 0.

## Cómo inyectar los hooks sin tocar el repo

| CLI | Mecanismo que funciona | Qué no funcionó |
|---|---|---|
| Claude | `claude -p … --settings <archivo.json>` con `{"hooks": {...}}`. En `-p` no hay diálogo de confianza. **No escribe nada en el worktree** | — |
| Codex | Hooks **inline con `-c`** en cada ejecución: `-c 'hooks.PreToolUse=[{hooks=[{type="command",command="…",timeout=30}]}]'` + `--dangerously-bypass-hook-trust`. Tampoco escribe en el worktree | `<worktree>/.codex/hooks.json` **no se cargó**, ni siquiera con `-c projects."<ruta>".trust_level="trusted"`. Queda sin explicar; no vale la pena seguir, porque `-c` es mejor |

## Hallazgos

1. **El shell del hook depende del CLI en Windows.** Claude corre el `command` en un shell tipo bash: `"C:/ruta con espacios/x.exe" args` funciona. **Codex lo corre en PowerShell**: el mismo string entre comillas es solo una expresión y no ejecuta nada (falla en silencio). Hay que usar `& 'C:/ruta con espacios/x.exe' args`, o una ruta sin espacios. En la máquina de Leo `%USERPROFILE%` **tiene un espacio** (`Latitude 7390`). **El adapter de Codex debe generar el comando con la sintaxis de PowerShell en Windows** (o usar `commandWindows`).
2. **Codex manda el modelo activo en cada evento** (`model`). Claude no: el modelo sale de `system/init` en el stream.
3. **Cuota:**
   - **Claude:** el stream de `-p` trae `rate_limit_event` (H3 confirmado):
     ```json
     {"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1790273400,"rateLimitType":"five_hour",
      "overageStatus":"rejected","isUsingOverage":false,
      "unifiedWindows":{"five_hour":{"utilization":0.57,"resetsAt":1790273400},"seven_day":{"utilization":0.48,"resetsAt":1790344800}}}}
     ```
     → `quota_certainty = KNOWN`, con porcentajes de 5 h y 7 días y los resets.
   - **Codex:** `exec --json` **no** trae `rate_limits`. Solo están en el rollout de disco (`event_msg` / `token_count`, ver `docs/research/cli-codex.md` §7). El adapter tiene que leer el rollout (ruta desde `transcript_path` del hook) para tener cuota KNOWN.
4. **Los shims `.cmd` rompen los prompts con comillas** (`codex.cmd exec "… \"x\" …"` → `unexpected argument`). Hay que pasar el prompt por **stdin** (`codex exec … -`) o llamar al `codex.exe` nativo (`node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe`).
5. `--dangerously-bypass-hook-trust` agrega dos eventos `item.completed` de tipo `error` con un aviso al stream. El parser del adapter los tiene que tratar como avisos, no como fallos.

## Qué cambia en el plan

- **P05 (adapters):** Claude inyecta con `--settings`, Codex con `-c hooks.*` + bypass. Comando del hook con la sintaxis del shell de cada CLI. Cuota de Codex desde el rollout.
- **P04 (process):** resolver el ejecutable nativo en lugar del shim `.cmd`. Mandar el prompt por stdin.
