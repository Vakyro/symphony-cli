# ADR-0005 · Modo de interacción: cómo ve e interviene Leo en cada agente

- **Estado:** ACEPTADO
- **Fecha:** 2026-09-24
- **Autor:** claude-code/opus-5.5 · **Aprobado por:** Leo (2026-09-24)
- **Fase/paso:** P01.S8

## Contexto
PLAN P01.S8 pide elegir entre: (a) headless + vista Conversation propia de Symphony; (b) PTY embebida en la TUI; (c) headless por defecto con "attach", que abre el CLI oficial en su propia terminal sobre la misma sesión. La decisión fija qué asumen P04 (PTY en `ProcessSupervisor`), P05.S4–S5 (`spawn`/`resume`) y P07.S5 (vista de agente).

## Opciones consideradas
1. **(a) Headless + vista propia.**
2. **(b) PTY embebida.** La TUI del CLI dentro de ratatui.
3. **(c) Headless + attach.**

## Decisión
**(a) como modo normal, con (c) como vía de escape. (b) queda fuera de v0.1.**

| Necesidad | Claude Code | Codex |
|---|---|---|
| Correr y observar | `claude -p --input-format stream-json --output-format stream-json --verbose --session-id <uuid>` | `codex exec --json …` (prompt por stdin) |
| Eventos estructurados | Hooks (`--settings`) + stream | Hooks (`-c hooks.*`) + stream + rollout (cuota) |
| **Mensaje de Leo a media tarea** | ✅ Se escribe en stdin como mensaje `user`. Probado: entra en el mismo turno | ⚠️ Solo **entre turnos**: `codex exec resume <id> "<mensaje>"`. `codex queue` acepta el mensaje, pero `exec` no lo consume (probado). Para hablar a media tarea: interrumpir (`Interrupt`) + `resume` |
| Handoff / reanudar | `--resume <id>` | `codex exec resume <id>` |
| **Attach** (abrir el CLI oficial) | Con el agente pausado: `claude --resume <id>` en una terminal aparte | Con el agente pausado: `codex resume <id>` en una terminal aparte |

Reglas:
- Symphony es dueño del proceso en headless. El attach **no** se hace en vivo sobre un proceso que corre: Symphony pausa o termina el turno, suelta el proceso y abre el CLI oficial con `resume` sobre la misma sesión. Al volver, Symphony retoma con `resume`.
- No se usan `claude --bg` ni `claude attach`. Traen su propio supervisor y competirían con `ProcessSupervisor` por el ciclo de vida del proceso.
- La vista Conversation (P07.S5) se construye con el stream + hooks. Los permisos se resuelven con `--permission-mode` / `-s` y hooks, no con diálogos del CLI.

## Evidencia
- Mensaje a media tarea en Claude: enviado a los 5.5 s, recibido (`replay`) a los 12.1 s y ejecutado en el mismo turno (`second.txt` creado). Script: `spikes/scripts/midtask-claude.mjs`.
- `codex queue --thread <id>` → "Queued message …", pero ni el `exec` en curso ni un `exec resume` posterior lo procesaron.
- Test B/D: los dos CLIs dan en headless todos los eventos que necesita la vista (`docs/research/cli-*.md`, `spikes/results/test-b.md`).
- ToS (`docs/research/tos.md`): si Anthropic vuelve a separar la facturación de `-p`, el attach en modo interactivo queda como alternativa sin rediseñar.

## Consecuencias
- **P04.S3:** `ProcessSupervisor` **sin PTY** en v0.1 (no se activa la feature `pty` de ProcessKit). Stdin tiene que quedar abierto y escribible (`keep_stdin_open`) para Claude.
- **P05.S4 (Claude):** `spawn` con `--session-id` generado por Symphony, `--input-format stream-json`, `send_message` = escribir en stdin, `resume` = `--resume`.
- **P05.S5 (Codex):** `spawn` = `exec --json`; `send_message` = encolar en Symphony y entregarlo con `exec resume` al terminar el turno (o interrumpir si Leo lo pide). La UI tiene que dejar claro que en Codex el mensaje entra en el próximo turno.
- **P07.S5:** vista Conversation propia + acción "Abrir en el CLI" (attach con pausa).
- Se revisa si Codex estabiliza `app-server` (JSON-RPC) con soporte para dirigir el turno en curso: sería la vía para hablarle a media tarea sin interrumpir.

## Adenda 2026-09-25 (P07.S7, claude-code/opus-5.5): fin de turno de Claude

**Problema (visto en la primera corrida live de la TUI):** con `--input-format stream-json`, Claude no sale al terminar su turno: se queda esperando el siguiente mensaje por stdin. El runtime tomaba el exit del CLI como fin de la tarea (P06), así que el agente quedaba `RUNNING` («trabajando») indefinidamente, hasta que el watchdog lo mataba como `NO_HEARTBEAT` (15 min). El fake-agent salía solo al terminar su guion, por eso ningún test lo detectó.

**Decisión:** el `result` del stream de Claude se traduce a `TurnFinished`. Al verlo, el executor cierra stdin en los CLIs que lo dejan abierto (`close_stdin_after_prompt() == false`), y el CLI sale con su exit code real. También se cierra ante un error fatal de proveedor. Queda igual que Codex: un proceso por turno, y los mensajes después del turno van con `resume`.

**Consecuencias:**
- Un mensaje de Leo **durante** el turno sigue entrando por stdin, en el mismo turno. Uno **después** del turno encuentra el proceso terminado: hace falta `resume` (pendiente; hoy `agent.send` devuelve un error claro).
- `fake-agent run --stdin stream` (lo pasa el fake adapter) imita a Claude: no sale hasta que le cierran stdin. Así los tests cubren este camino.
