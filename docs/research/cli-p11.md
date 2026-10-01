# CLIs de P11 · Kimi Code, Copilot CLI y Antigravity (P11.S1)

- **Fecha:** 2026-10-01 · **Agente:** claude-code/sonnet-5.5 · **Entorno:** Windows 11, los tres CLIs ya instalados y con sesión iniciada. No se leyeron credenciales.
- **Pruebas en vivo:** autorizadas por Leo el 2026-10-01 (un turno mínimo, un `resume` y un modelo inexistente por CLI, más un `--session-id` en Copilot). Prompt: «Responde solo con la palabra: ok», en un directorio vacío. Salidas sanitizadas en `fixtures/providers/{kimi,copilot,antigravity}/`.
- Desviación del PLAN: un solo archivo en vez de tres `cli-*.md`, porque los tres comparten el mismo contrato.

OpenCode (`1.15.13`) también está instalado; queda fuera del alcance (ADR-0008).

## Comparativa

| | Kimi Code 1.44.0 | Copilot CLI 1.0.60 | Antigravity `agy` 1.2.11 |
|---|---|---|---|
| Turno headless | `kimi --print -p <t> --output-format stream-json` | `copilot -p <t> --output-format json --no-ask-user` | `agy -p <t> --output-format stream-json` |
| Duración del turno mínimo | 20,9 s | 20,6 s | 10,1 s (modelo: 3,2 s) |
| Formato | **un** JSON al final del turno (`role`/`content[]`: `think`, `text`); sin deltas | JSONL de eventos con `type` (`user.message`, `assistant.turn_start`, `assistant.message_delta`, `assistant.message`, `assistant.turn_end`, `result`…) | NDJSON con `event`: `init`, `step_update` (con `text_delta`), `result` |
| **Id de sesión** | **solo en stderr**: `To resume this session: kimi -r <uuid>` | solo en el evento final `result.sessionId`; **o se fija antes con `--session-id <uuid>`** (verificado: el `result` devuelve el mismo UUID) | en el primer evento `init.conversation_id` |
| Resume | `kimi -r <id> --print -p …` ✅ conserva contexto | `copilot --resume=<id> -p …` ✅ conserva contexto | `agy --conversation <id> -p …` ✅ conserva contexto |
| Uso/tokens | no aparece | solo `premiumRequests` (0,33 por turno) y duración; sin tokens | `usage` por paso y en `result` (22.001 entrada abriendo; 27.983 al retomar) |
| Herramientas / permisos | `--yolo`; no probado | `--allow-all-tools`, `--no-ask-user`; no probado | `permission_mode: request-review` por defecto; `--mode accept-edits`, `--dangerously-skip-permissions`; no probado |
| Lista de modelos | no probado (`-m`, valor desde config) | `--model` (hay `auto`) | `agy models` (Gemini 3.x, Claude Sonnet/Opus 4.6, GPT-OSS 120B) |
| Modelo inexistente | stdout `LLM not set`, id de sesión en stderr; el exit code no se capturó | stderr `Error: Model "x" from --model flag is not available.` | `result.status = "ERROR"` con el texto y la lista de modelos disponibles; el id de conversación viene vacío |
| Cierre de stdin | no hizo falta (con `</dev/null`) | idem | idem |

## Lo que cambia para el adapter
1. **Kimi** necesita leer stderr para el id de sesión (no sale por stdout); `parse_stream_line` no basta. Sin deltas: el chat verá la respuesta de golpe.
2. **Copilot**: usar `--session-id <uuid>` generado por Symphony; así el id se conoce antes del primer evento y no hace falta parsear el `result`.
3. **Copilot cambió de modelo al retomar** (`claude-haiku-4.5` → `gpt-5.4-mini`) porque el default es `auto`: el adapter debe pasar siempre `--model` explícito, o Symphony pierde el principio «modelo exacto nunca sobrescrito».
4. **Antigravity** es el más parecido a Claude (`init` con id, deltas, uso por turno). `--input-format stream-json` permite varios turnos con un solo proceso, pero no se probó.
5. Los tres respetan `AGENT ≠ MODEL`: el modelo se pasa por flag en cada llamada.
6. **Coste de abrir sesión**: Antigravity gastó ~22k tokens de entrada solo en abrir (Codex ~18k, Claude ~27k, STATUS). Copilot no reporta tokens; Kimi tampoco.

## Kimi: hallazgos de P11.S2 (2026-10-01)
- **Prompt por stdin:** `kimi --print --output-format stream-json` lee el prompt hasta EOF (sin `-p`). Es lo que exige el contrato (el prompt nunca va como argumento).
- **`-S <uuid>` crea la sesión con ese id** (verificado con un UUID nuevo) y la retoma si existe: sirve para `spawn` y para `resume`. El id sigue saliendo solo por stderr (`To resume this session: kimi -r <uuid>`); `--verbose` no lo mueve a stdout.
- **`--print` auto-aprueba las herramientas** (el `--help` lo dice) y Kimi no tiene sandbox: el agente puede ejecutar cualquier comando. Riesgo a decidir antes de ofrecerlo en el chat.
- **Un JSON por mensaje**, no solo al final: `assistant` (`content` texto o lista `think`/`text`, más `tool_calls[].function{name,arguments}`) y `tool` (`tool_call_id`, `content`). Herramientas vistas: `WriteFile`, `Shell`, `ReadFile`. Un fallo llega como `<system>ERROR: …</system>` (`Command failed with exit code: 3.`, `` `x` does not exist.``). Sin tokens de uso ni evento de fin de turno.
- Fixtures: `fixtures/providers/kimi/tools.jsonl` y `tools-error.jsonl`.

## Hooks y Test C (`hooks_can_hold`)
Sin verificar y no hace falta para el chat: el trait admite `supports_hooks() = false` y `hooks_can_hold() = None`. Se retoma si P08 vuelve. Los `--help` de los tres no mencionan hooks.

## Sin verificar
1. **Errores de cuota, rate limit y auth** (no se pueden provocar sin agotar la cuota). `parse_error` empezará con los textos de modelo inexistente de arriba y quedará con fixtures sintéticas, como Claude y Codex (`fixtures/providers/README.md`).
2. Eventos de herramientas (`tool_call`, edición de archivos) en los tres: el prompt mínimo no usó ninguna. Falta un turno que edite un archivo, probablemente en el live de S2–S4.
3. Exit code de Kimi con modelo inválido; comportamiento con Ctrl+C / kill del árbol en Windows.
4. Skills nativas: dónde están (`~/.copilot/skills`, `~/.gemini/skills`, `--skills-dir` en Kimi) y qué prefijo usa cada una para invocarlas.
5. **ToS (`tos.md`)**: no cubre a estos tres. Pendiente de Leo; `agy` ofrece modelos de Anthropic y OpenAI, lo que hace más importante revisar cómo cuenta la cuota.
6. Copilot y Antigravity consumen «solicitudes premium» o cuota propia: medir con uso real antes de ofrecerlos como destino de failover.
