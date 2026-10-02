# Fixtures de proveedores (STACK §42, capa L1)

Salidas reales de los CLIs, capturadas en el spike de P01 (2026-09-24, Windows 11),
**sanitizadas**: rutas y nombre de usuario reemplazados por `user`, y pasadas por
el criterio del redactor (no contienen tokens ni credenciales). Un ejemplo por tipo
de evento o de hook.

| Archivo | Origen |
|---|---|
| `claude-code/stream.jsonl` | `claude -p --output-format stream-json --verbose` (Claude Code 2.1.281) |
| `claude-code/hooks.jsonl` | Payloads de hooks de Claude Code (`--settings`) |
| `codex/exec-stream.jsonl` | `codex exec --json` (codex-cli 0.154.0) |
| `codex/hooks.jsonl` | Payloads de hooks de Codex (`-c hooks.*`) |
| `kimi/stream.jsonl`, `kimi/stderr-session.txt`, `kimi/error-modelo.txt` | `kimi --print --output-format stream-json` (Kimi Code 1.44.0, 2026-10-01) |
| `copilot/stream.jsonl`, `copilot/error-modelo.txt` | `copilot -p --output-format json` (Copilot CLI 1.0.60); se omiten las skills y el razonamiento opaco |
| `copilot/stdin.jsonl`, `tools-denied.jsonl`, `tools-write.jsonl` | `copilot --output-format json` con el prompt por stdin (1.0.60): turno simple, herramientas con permiso denegado y con `--allow-tool=write`. Sin skills ni razonamiento opaco; usuario reemplazado |
| `antigravity/stream.jsonl`, `resume.jsonl`, `error-modelo.jsonl` | `agy -p --output-format stream-json` (1.2.11); `cwd` reemplazado por `<cwd>` |
| `antigravity/tools.jsonl`, `tools-denied.jsonl` | `agy --print= --input-format stream-json` (1.2.14) con herramientas; con `--dangerously-skip-permissions` y con `--mode accept-edits` (shell denegado). Rutas y usuario reemplazados |
| `kimi/tools.jsonl`, `tools-error.jsonl` | `kimi --print` con herramientas, sin y con fallos (1.44.0) |

Los errores de rate limit y cuota que no aparecieron en vivo se prueban con las
formas documentadas (`docs/research/cli-*.md`), marcadas como sintéticas en los tests.
