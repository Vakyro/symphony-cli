# Modelos de las seis suscripciones (sondeo local, 2026-10-23)

Sondeo sin gastar cuota (salvo 1 mensaje a Kimi). Versiones: Claude Code 2.1.310, Codex 0.164.0, agy 1.4.7, Kimi 1.44.0, OpenCode 2.1.4, Copilot CLI 1.0.103.

| Proveedor | De dónde sale la lista | Qué hay | Qué hace Symphony |
|---|---|---|---|
| Claude Code | alias del CLI (`--model opus\|sonnet\|haiku\|fable`) | los alias siempre apuntan al último | sin cambios |
| Codex | `~/.codex/models_cache.json` (visibilidad `list`) | `gpt-6.1-sol` (ctx 500k), `gpt-6-astra`, `gpt-6-sol`, `gpt-6-luna`, `gpt-5.6-{sol,terra,luna}`; niveles de razonamiento hasta `ultra`; ocultos: `gpt-reserve`, `codex-auto-review` | lista fija actualizada; `gpt-5.5` ya no existe |
| Antigravity | `agy models` | Google: `gemini-4-argon-{low,medium,high}`, `gemini-3.8-flash-{low,medium,high}`. Otros: `claude-opus-5-5-*`, `claude-sonnet-5-5-*`, `gpt-oss-120b-medium` | solo los de Google (los otros siguen fuera hasta revisar los ToS); `gemini-3.1-pro-*` ya no existe |
| Kimi Code | `[models.*]` de `~/.kimi/config.toml` | `kimi-for-coding` (1M), `-highspeed`, `k3`, `k3-256k`, `k3.1-pro`, `k3.1-pro-highspeed` (1M) | `default` + los seis; `-m kimi-code/<modelo>` verificado en vivo con `k3.1-pro` |
| OpenCode | `opencode models` (86 líneas) | dos proveedores: `opencode/…` (Claude, GPT-6, Gemini 4 Argon, Kimi K3, GLM, Qwen, DeepSeek, y gratuitos `*-free`) y `opencode-go/…` | dinámico, sin cambios en el adaptador; `symphony providers refresh` para renovar el caché |
| Copilot | ninguna: el CLI no tiene comando de listado (solo `/model` interactivo); `--model auto` | desconocidos | sigue en `auto` (⛔ sondear a mano) |

Router (`crates/daemon/src/routing.rs`): familias nuevas `astra` (cima de OpenAI), `argon` (cima de Google) y `gpt-oss`; el resto ya cae en `sol`, `luna`, `opus`, `kimi`… por subcadena.

Nota: `opencode models` falló una vez con `ServiceUnavailableError: Location plugins did not finish activating` y funcionó al repetir; el adaptador ya lo pide en segundo plano y conserva la lista anterior.
