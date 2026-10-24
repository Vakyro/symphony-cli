# Harness de Symphony por proveedor (verificado en vivo, 2026-10-23)

ADR-0019: todos los modelos trabajan con las mismas herramientas (`read`, `write`, `edit`, `glob`, `grep`, `shell` + `context_*`), que expone el broker `symphony mcp serve` cuando el agente corre con `SYMPHONY_HARNESS=1`. Se activa con `[harness] mode = "symphony"` en `~/.symphony/config.toml` (por defecto `native`).

Prueba: en una carpeta vacía, «crea `hola.txt` con la palabra PRUEBA y léelo». 1 a 2 mensajes por proveedor, dentro del tope que autorizó Leo.

| Proveedor | Cómo se apagan sus herramientas propias | Resultado en vivo |
|---|---|---|
| Claude Code 2.1.310 | `--tools ""` (apaga todas las nativas; los MCP quedan) + `--allowedTools mcp__symphony-context__*` + `--append-system-prompt` | ✅ usó `write` y `read` del broker; solo ve esas. **$0,0024** (10,3 k tokens de caché creados) frente a **$0,0074** (32,8 k) con sus herramientas nativas: −68 %, mismos 3 turnos |
| Codex 0.164.0 | `--disable shell_tool --disable unified_exec --disable view_image` | ✅ `mcp: symphony-context/write` y `/read`, con `-s danger-full-access` y aprobación «never» (las herramientas MCP corren sin preguntar). Codex llama a las herramientas desde su ejecutor de código (`functions.exec`); su intento de usar `exec_command` falló, como se quería |
| OpenCode 2.1.4 | `OPENCODE_CONFIG_CONTENT` con `permission.{bash,edit,read,glob,grep,list,webfetch,task} = "deny"` + el MCP | ✅ usó `write` y `read` con los argumentos del broker (`path`, no `filePath` de OpenCode); modelo `opencode/big-pickle` |
| Copilot CLI 1.0.103 | `--available-tools symphony-context` + `--additional-mcp-config` | ✅ lista exactamente las 9 herramientas del broker (más el servidor github propio, que `--available-tools` deja fuera) y crea el archivo |
| Kimi Code 1.44.0 | `--agent-file` con `extend: default`, `tools: []`, `subagents: {}` + `--mcp-config` | ✅ el modelo ve exactamente las 9 herramientas del broker y las usa |
| Antigravity (`agy` 1.4.7) | ninguna: no hay MCP ni lista de herramientas por invocación (`agy mcp add` escribe config persistente del usuario) | ❌ **harness no disponible**; sigue con sus herramientas nativas. Habría que escribir en su config o en el worktree, y Symphony no toca ninguna de las dos |

## Qué falta (no se midió)
- Tareas largas (refactor de varios archivos, correr pruebas): lo medido es una tarea mínima. Lo que sí dice es que ningún CLI se niega a usar las herramientas del broker y que Claude gasta menos.
- La cola de recursos de P08 (comandos pesados) hoy se aplica a `shell` solo donde el CLI manda hooks que lo clasifican. Pasar `shell` por el daemon queda para después de que Leo use el harness unos días.
- El permiso de OpenCode `external_directory` del worktree sigue; no afecta al broker (es un proceso aparte).

## Decisión
El modo por defecto sigue siendo `native`: cambiarlo es de Leo (STATUS § Para Leo). Con `mode = "symphony"` Claude, Codex, OpenCode, Copilot y Kimi comparten interfaz; Antigravity queda como excepción documentada.
