# Proveedores

Symphony soporta seis CLIs de proveedores bajo un mismo contrato (`ProviderAdapter`). Cada proveedor conserva su propio inicio de sesión; Symphony nunca lee ni toca tus credenciales OAuth.

## Tabla de proveedores y capacidades

| Proveedor | Id de modelo en Symphony | Comportamiento de permisos y herramientas | Skills en el chat | Cuota (`quota_certainty`) |
|---|---|---|---|---|
| **Claude Code** | `claude/haiku`, `claude/sonnet`, `claude/opus`, … | Acceso local total por defecto (`bypassPermissions`); configurable en `[providers.permissions]`. Compatible con el harness de Symphony. | Sí (`/skill`) | `KNOWN` (eventos `five_hour` / `seven_day` del stream) |
| **Codex** | `openai/gpt-5.6-luna`, `codex/gpt-5.x`, … | Acceso local total por defecto (`danger-full-access`); configurable (`read-only`, `workspace-write`, `danger-full-access`). Compatible con el harness. | Sí (`$skill`) | `KNOWN` (leída del rollout en disco) |
| **Kimi Code** | `moonshot/default` (usa el modelo configurado en Kimi) | **Todo, sin pedir permiso**: `--print` aprueba todas las herramientas y Kimi no tiene modo sandbox. Compatible con el harness. | No | `ESTIMATED` (si configuras presupuesto) o `UNKNOWN` |
| **Antigravity** (`agy`) | `google/gemini-3.8-flash-low`, `…-medium`, `…-high`, `gemini-3.1-pro-high/low`, … | Con `antigravity_skip_permissions = true` (defecto) ejecuta sin pedir permiso; con `false`, en headless deniega los comandos que piden permiso. **No admite el harness MCP** y usa siempre sus herramientas nativas. | No | `ESTIMATED` o `UNKNOWN` |
| **GitHub Copilot CLI** | `github/auto` | Con `copilot_allow_all_tools = true` (defecto) permite todas las herramientas; con `false`, edita archivos y limita comandos a lectura. Compatible con el harness. | No | `ESTIMATED` (solicitudes premium / presupuesto) o `UNKNOWN` |
| **OpenCode** | `opencode/default`, `opencode/big-pickle`, `opencode/<proveedor>:<modelo>` | **Todo, sin pedir permiso** en `opencode run`, sin sandbox; Symphony restringe escritura a su worktree. Compatible con el harness. | No | `ESTIMATED` o `UNKNOWN` |

Para consultar en cualquier momento qué proveedores están instalados, su estado de salud y su cuota:

```bash
symphony providers
symphony providers refresh
symphony usage --days 7
```

---

## Cómo interactúa Symphony con cada CLI

### Ejecución headless, continuación de sesión y `attach` (ADR-0005, ADR-0009)

- **Un proceso por turno en modo headless:** Symphony lanza el CLI en modo no interactivo con salida JSON estructurada (`--output-format stream-json` en Claude y Antigravity, `exec --json` en Codex, `--print` en Kimi, `run --format json` en OpenCode) y construye la vista **Conversation** en tiempo real (incluyendo deltas de texto parcial en Claude con `--include-partial-messages`).
- **Fin de turno en Claude Code:** cuando Claude recibe `--input-format stream-json`, espera más mensajes por `stdin`; al detectar el evento `result` (`TurnFinished`), Symphony cierra `stdin` para que el proceso termine limpiamente con su código de salida.
- **Mensajes durante y después del turno (`symphony send <agente> "<mensaje>"` o tecla `m`):**
  - En **Claude Code**, si el turno está `RUNNING`, el mensaje se escribe directamente en su `stdin` y entra dentro del mismo turno.
  - Cuando un turno ya terminó (`COMPLETED` o `READY` en el chat), Symphony retoma la sesión nativa del proveedor (`--resume <id>` en Claude, `codex exec resume <id>` en Codex, `kimi -r <id>` en Kimi, `opencode run -s <id>` en OpenCode) para que el proveedor conserve su memoria sin reenviar todo el historial.
  - Si el agente está `FAILED` o `PAUSED`, `send` se rechaza con una pista para que primero lo reanudes (`symphony resume`) o lo recuperes.
- **Captura del ID de sesión:** Claude lo informa en `system/init`, Antigravity en `init`, Copilot en `result` (o fijado con `--session-id`), OpenCode en `step_start`, y Kimi lo emite por `stderr` (`To resume this session: kimi -r <uuid>`), que el ejecutor procesa mediante `parse_stderr_line` (ADR-0009).
- **Abrir en el CLI nativo (`symphony attach <agente>` o tecla `a`):** con el agente pausado o entre turnos, Symphony abre una terminal aparte con la sesión nativa del CLI (`claude --resume <id>`, `codex resume <id>`, etc.). Con `--print` solo muestra el comando sin abrir la ventana.

### Inyección de hooks y retención de operaciones pesadas (ADR-0003)

Symphony no escribe archivos de configuración de hooks dentro de tu repositorio:
- En **Claude Code**, inyecta los hooks por invocación mediante `--settings <json>`. Nota: nunca se usa `claude --bare` porque ignora la suscripción y exige `ANTHROPIC_API_KEY`.
- En **Codex**, pasa los hooks inline con `-c 'hooks.<Evento>=[{hooks=[{type="command",command="…"}]}]'` y `--dangerously-bypass-hook-trust`, sin tocar `CODEX_HOME` (donde vive `auth.json`). En Windows, Codex ejecuta los hooks en PowerShell, por lo que Symphony los invoca con `& 'ruta' args`.

Cuando un agente solicita una operación pesada (clase 3 o 4, como compilar o correr tests) y no hay slots libres en el scheduler:
- **Claude Code** retiene el comando en el hook `PreToolUse` hasta el `timeout` configurado.
- **Codex** retiene en el hook hasta un máximo de **60 segundos** (si esperase 120 s, el tool call fallaría y el modelo reintentaría duplicando la espera). Si la espera supera el límite, el hook responde `deny` explicando qué operación está delante en la cola para que el agente reintente más tarde.
- En todos los proveedores, la segunda capa del scheduler aplica los límites del sistema operativo (`ProcessSupervisor` sobre `ProcessKit`, ADR-0002).

### Harness de Symphony (ADR-0019)

Con `[harness] mode = "symphony"` (valor por defecto en `~/.symphony/config.toml`), Symphony conecta su servidor MCP (`symphony mcp serve`) y apaga las herramientas nativas de Claude Code (`--allowedTools`/`--disallowedTools`), Codex (`-c` de tools y sandbox), Copilot (`--deny-tool`), OpenCode y Kimi. Así todos los modelos operan con las mismas herramientas:
- `read`, `write`, `edit`, `glob`, `grep`, `shell`
- `context_search`, `context_retrieve`, `context_lines`

Si prefieres que cada CLI utilice sus herramientas propias, configura `[harness] mode = "native"` y reinicia el daemon (`symphony daemon stop`).

---

## Particularidades por proveedor

- **OpenCode (ADR-0012):**
  - OpenCode toma como raíz del proyecto el repositorio principal y no el worktree del agente. Symphony enmarca el prompt indicándole su directorio de trabajo y configura permisos solo para `<worktree>/**`, aunque un modelo podría intentar escribir en el repositorio principal si usa una ruta absoluta.
  - El catálogo de modelos (`opencode models`) se consulta en segundo plano la primera vez y tarda unos ~7 segundos: al principio solo verás `opencode/default`; ejecuta `symphony providers refresh` unos segundos después para listar todos los modelos (`opencode/big-pickle`, `opencode/<proveedor>:<modelo>`).
- **GitHub Copilot CLI:**
  - Con `github/auto`, Copilot puede alternar de modelo internamente entre turnos y no reporta tokens por turno, por lo que el cambio automático del chat por umbral de tokens (`switch_at_tokens`) no le aplica.
- **Antigravity y Copilot:**
  - Ambos CLIs se actualizan solos. Si algún modelo no está habilitado en tu cuenta, el CLI falla al instante con `not available` y Symphony aplica la política de failover configurada.

---

## Routing, profiles, salud y sugerencias (ADR-0010, ADR-0016)

### Selección por profile o modelo exacto

Puedes crear o cambiar un agente indicando un **modelo exacto** o un **profile**:

```bash
symphony spawn "Arreglar el login" --model claude/sonnet
symphony spawn "Arreglar el login" --profile @fast
symphony switch 1 codex/gpt-5.x
symphony switch 1 --profile @reasoning
symphony explain-route 1
```

- **Un modelo exacto nunca se sustituye en silencio:** si pides `--model claude/sonnet`, Symphony lo usa aunque su cuota esté dentro del 20 % de reserva. Solo cambiará a otro si el modelo falla y la política `--failover` del agente lo permite (`any` o `same-provider`).
- **El router descarta candidatos no elegibles y explica el motivo** (`symphony explain-route <agente>` o tecla `?` en la TUI):
  - Sin sesión activa (`AUTH`).
  - Cuota agotada (`EXHAUSTED`).
  - En enfriamiento tras un límite temporal (`COOLDOWN`).
  - Ventana de contexto insuficiente para el tamaño del handoff (`needed_context`) o falta de capacidades requeridas.
  - Dentro de la reserva de cuota (`quota_reserve`, 20 % por defecto para profiles automáticos).
- **Un error 429 no es cuota agotada:** se clasifica como límite temporal (`RATE_LIMITED`), entra en espera (`COOLDOWN`) y al cumplirse el plazo pasa a `PROBING` para volver a probarse automáticamente.

### Desempeño aprendido y sugerencias de cambio de modelo

- **Ajuste aprendido (ADR-0016):** cada vez que un run iniciado con un profile termina (`COMPLETED` o `CRASH`), Symphony registra el resultado en `~/.symphony/learning.db`. Cuando un par `(profile, modelo)` acumula **10 o más muestras**, el router aplica un ajuste determinista acotado a `±0,10` sobre su puntaje base.
- **Sugerencias de modelo:** si durante el trabajo de un agente cambia el tipo de tarea (por ejemplo, de redactar documentación a un refactor de múltiples archivos) y existe un modelo con mejor puntaje, Symphony emite una sugerencia (nunca cambia el modelo por su cuenta):
  - En el **chat**: `Ctrl+T` cambia al modelo sugerido, `Ctrl+K` mantiene el actual, `Ctrl+N` silencia futuras sugerencias para ese agente.
  - En la **vista de agente**: teclas `S`, `K` y `N`.
  - En la **terminal**:
    ```bash
    symphony suggestion list
    symphony suggestion switch <id>
    symphony suggestion keep <id>
    symphony suggestion mute <id>
    ```
