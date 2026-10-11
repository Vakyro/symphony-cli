# OpenCode (CLI) — lo verificado para el adapter

Versión 1.18.34 (npm `opencode-ai`), Windows 11, 2026-10-11. Todo salió de ejecuciones reales con modelos **gratuitos** del proveedor propio de OpenCode (`opencode/big-pickle`, `ling-3.1-flash-free`, `mimo-v2.6-flash-free`), sin gastar la cuota de nadie. Decisión: [ADR-0012](../adr/0012-opencode-como-proveedor.md). Bitácora: [P11 § S7](../phases/P11-proveedores.md). Observaciones previas: [opencode-y-calidad-de-proveedores.md](opencode-y-calidad-de-proveedores.md).

## Invocación

| Necesidad | Forma |
|---|---|
| Turno headless | `opencode run --format json [-m proveedor/modelo]`; prompt por **stdin** hasta EOF (también acepta argumento) |
| Continuar una sesión | `-s <ses_…>` (también `-c`, `--fork`) |
| Abrir la sesión en la TUI | `opencode -s <ses_…>` |
| Versión / modelos | `opencode --version` / `opencode models` (una línea `proveedor/modelo`) |
| Ejecutable nativo (Windows) | `<npm>/node_modules/opencode-ai/bin/opencode.exe`; el shim `opencode.cmd` solo lo invoca |
| Config por invocación | variable `OPENCODE_CONFIG_CONTENT` (JSON, se **suma** a la config del usuario) |

## Stream (`--format json`)

Un JSON por línea, todos con `sessionID`: `step_start`, `text`, `tool_use`, `step_finish`, `error`. Un turno son varios pasos; el último cierra con `part.reason = "stop"` y el proceso sale con 0.

- `tool_use` llega **ya terminado**: `state.status` es `completed` o `error`. Herramientas vistas: `write`, `edit`, `read`, `glob`, `bash` y las MCP (`<servidor>_<herramienta>`, p. ej. `symphony-context_context_search`).
- `bash` termina `completed` aunque el comando falle: el éxito sale de `state.metadata.exit`.
- Tokens por paso en `step_finish.part.tokens` (`input`, `output`, `reasoning`, `cache.read/write`).
- `error`: `{error: {name, data: {message, statusCode?, responseBody?}}}`. Vistos: `UnknownError` (modelo o proveedor inexistente, mensaje genérico) y `APIError` con `statusCode: 429` («Upstream request failed: Endpoint is unavailable») de un modelo gratuito caído.

## Sesión y contexto

El id lo asigna OpenCode y sale en el primer evento (no por stderr). **`-s` continúa con el historial completo**: verificado, el segundo turno recordó el archivo y el texto del primero. El handoff entre dos modelos de OpenCode (`symphony switch` en vuelo) también funcionó: el segundo terminó la tarea.

## Lo que hay que saber (trampas)

1. **La raíz del proyecto es el repo principal, no el worktree.** OpenCode usa `--git-common-dir`. El directorio de trabajo sí es el worktree (cwd), pero el prompt de sistema del modelo menciona la raíz del workspace y un modelo puede escribir en el repo con una ruta absoluta. Con rutas relativas escribe bien.
2. **El worktree cuenta como «directorio externo».** En headless un permiso que se pregunta se rechaza solo («The user rejected permission…»): el `write` con ruta absoluta al worktree falla. `OPENCODE_CONFIG_CONTENT` con `{"permission":{"external_directory":{"<worktree>/**":"allow"}}}` lo resuelve; el patrón tiene que usar la **ruta larga** (`C:/Users/Latitude 7390/…`), no la 8.3 (`LATITU~1`). Sirven `/` y `\`.
3. En `run` las herramientas se ejecutan **sin pedir permiso** y no hay sandbox.
4. **MCP por invocación:** `{"mcp": {"<nombre>": {"type": "local", "command": ["exe", "arg"], "environment": {…}, "enabled": true}}}` o `{"type": "remote", "url": …}`, dentro de `OPENCODE_CONFIG_CONTENT`.
5. Sin `-m`, `run` usa el modelo configurado o el último usado y funciona.
6. Los modelos gratuitos son inestables (429 de upstream en `ling-3.1-flash-free`).
7. **Es lento de arrancar:** `opencode --version` tarda ~2,5 s y `opencode models` ~6,5 s (arrancan todo el runtime). Symphony lee la versión del `package.json` del paquete y pide la lista de modelos **suelta, a un archivo** (`~/.symphony/cache/opencode-models.txt`, vigencia de 24 h): el arranque del daemon no espera, y la lista completa aparece en el siguiente `symphony providers refresh`. Antes de esto, cada arranque del daemon (y cada prueba) pagaba ~9 s y las pruebas con 40 daemons se pasaban de tiempo.

## Sin verificar

- Cuota agotada y auth caída reales (la clasificación es sintética: `quota`, `usage limit`, `insufficient`, `ProviderAuthError`, `401`).
- Handoff desde y hacia Claude o Codex.
- Skills: OpenCode lee sus propios directorios; Symphony no traduce las compartidas.
- La credencial de GitHub Copilot por OAuth que OpenCode ya tiene en esta máquina: revisar [tos.md](tos.md) antes de usarla.
