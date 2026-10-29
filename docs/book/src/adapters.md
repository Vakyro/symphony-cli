# Adapters de proveedores

Un *adapter* traduce un CLI oficial de IA a los tipos canónicos de Symphony. El núcleo (`symphony-core`, `symphony-daemon`) **nunca hace `if provider == "claude"`**: toda diferencia de argumentos, formato de stream, hooks, sesiones o clasificación de errores vive detrás del trait `ProviderAdapter` en `crates/adapters/common`.

---

## 1. El contrato `ProviderAdapter`

Definido en `crates/adapters/common/src/lib.rs`, todos sus métodos son síncronos y puros (devuelven un `ProcessSpec` que ejecuta `symphony-process` o transforman líneas/JSON en memoria):

```rust
pub trait ProviderAdapter: Send + Sync {
    fn provider_id(&self) -> &'static str;   // ej. "anthropic", "openai", "opencode"
    fn display_name(&self) -> &'static str;
    fn cli_name(&self) -> &'static str;      // ej. "claude", "codex", "kimi", "agy", "copilot", "opencode"

    fn detect(&self) -> Result<Detection, AdapterError>;
    fn auth_status(&self) -> AuthStatus;     // Ok | Missing | Unknown (sin leer credenciales)
    fn list_models(&self) -> Vec<ModelInfo>;
    fn supports_hooks(&self) -> bool;
    fn hooks_can_hold(&self) -> Option<bool>;

    fn spawn_spec(&self, req: &SpawnRequest) -> Result<ProcessSpec, AdapterError>;
    fn resume_spec(&self, req: &ResumeRequest) -> Result<ProcessSpec, AdapterError>;
    fn attach_spec(
        &self,
        cli_session_id: &str,
        model: &str,
        worktree: &std::path::Path,
    ) -> Result<ProcessSpec, AdapterError>;

    fn frame_prompt(&self, prompt: &str, _worktree: &std::path::Path) -> String;
    fn encode_prompt(&self, prompt: &str) -> Vec<u8>;
    fn close_stdin_after_prompt(&self) -> bool;
    fn encode_user_message(&self, text: &str) -> Option<Vec<u8>>;

    fn parse_stream_line(&self, line: &str) -> Vec<AgentEvent>;
    fn parse_stderr_line(&self, _line: &str) -> Vec<AgentEvent>;
    fn parse_hook(&self, payload: &Value) -> Vec<AgentEvent>;
    fn parse_error(&self, text: &str) -> Option<ProviderError>;
}
```

### Reglas del contrato

1. **El prompt nunca va como argumento de línea de comandos:** `spawn_spec` deja `stdin` abierto y el ejecutor escribe `encode_prompt(prompt)` por `stdin` (evita límites de longitud y roturas de comillas en wrappers `.cmd` de Windows).
2. **Eventos canónicos (`AgentEvent`):** sea por `stdout`, `stderr` o por un hook (`symphony hook emit`), el adapter emite los mismos eventos: `SessionStarted`, `TurnStarted`, `TurnFinished`, `AssistantText`, `AssistantDelta`, `UserMessage`, `ToolRequested`, `ToolFinished`, `FileModified`, `ProviderError`, `Quota` y `TurnUsage`.
3. **Inyección por invocación (CONSTRAINTS A12):** ni hooks, ni el broker MCP (`symphony mcp serve`), ni las skills compartidas escriben configuración dentro del repositorio del usuario.
4. **Credenciales intocables:** Symphony nunca lee tokens ni archivos de sesión (`auth.json`, `~/.claude`, `~/.kimi`, `~/.gemini`, `~/.copilot`, `~/.local/share/opencode`).

---

## 2. Resumen técnico de los 6 adapters

| Crate (`crates/adapters/*`) | `provider_id` / `cli_name` | Invocación headless (`spawn_spec`) | Continuación (`resume_spec`) | Hooks (`PreToolUse`) | Harness MCP (`ADR-0019`) |
|---|---|---|---|---|---|
| `claude` | `anthropic` / `claude` | `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages` | `--resume <id>` | Sí (`--settings '<json>'`, retiene hasta `hook_secs`) | Sí (`--mcp-config`, `--allowedTools` / `--disallowedTools`) |
| `codex` | `openai` / `codex` | `codex exec --json -s <sandbox> -m <model> -` | `codex exec resume <id> --json -c sandbox_mode="…" -` | Sí (`-c hooks.<Ev>=[…]` + `--dangerously-bypass-hook-trust`, máx. 60 s) | Sí (`-c mcp_servers.symphony-context=…`) |
| `kimi` | `moonshot` / `kimi` | `kimi --print --output-format stream-json -S <id>` | `kimi --print --output-format stream-json -S <id>` | No | Sí (`--mcp-config-file` + `--agent-file` sin tools nativas) |
| `antigravity` | `google` / `agy` | `agy --print= --input-format stream-json --output-format stream-json --model <m>` | `--conversation <id>` | No | **No** (usa siempre sus herramientas nativas) |
| `copilot` | `github` / `copilot` | `copilot --output-format json --no-ask-user --session-id <id>` (prompt por `stdin`, sin `-p`) | `--resume=<id>` | No | Sí (`--additional-mcp-config` + `--deny-tool` de nativas) |
| `opencode` | `opencode` / `opencode` | `opencode run --format json [-m <prov/mod>]` | `opencode run --format json -s <id>` | No | Sí (`OPENCODE_CONFIG_CONTENT` con `mcp` y `permission`) |

### Detalles de implementación por proveedor

- **Claude Code (`symphony-adapter-claude`):**
  - En Windows resuelve directamente `node_modules/@anthropic-ai/claude-code/bin/claude.exe` cuando detecta `claude.cmd` para evitar árboles huérfanos de `cmd.exe`.
  - Nunca pasa `--bare` (ignora la suscripción OAuth y exige `ANTHROPIC_API_KEY`).
  - `close_stdin_after_prompt()` es `false`: permite inyectar mensajes a mitad de turno por `stdin` y el runtime cierra `stdin` al recibir `TurnFinished` (`result`).
- **Codex (`symphony-adapter-codex`):**
  - En Windows resuelve el binario nativo `codex.exe` dentro de `vendor/<triple>/bin/codex.exe` detrás del shim `codex.cmd`.
  - Los hooks en Windows corren bajo PowerShell, por lo que el comando se formatea como `& 'ruta\symphony.exe' 'hook' 'emit'`.
  - La cuota `KNOWN` se extrae de los archivos de *rollout* en disco (`quota_from_rollout`).
- **Kimi Code (`symphony-adapter-kimi`):**
  - Emite el identificador de sesión por `stderr` (`To resume this session: kimi -r <id>`), capturado con `parse_stderr_line` (ADR-0009).
  - Define `PYTHONIOENCODING=utf-8` y `PYTHONUTF8=1` en el `ProcessSpec` para evitar que Python falle con `UnicodeEncodeError` en pipes de Windows (`cp1252`).
- **Antigravity (`symphony-adapter-antigravity`):**
  - Requiere `--print=` vacío junto con `--input-format stream-json` para leer el mensaje JSON desde `stdin`.
  - Expone 13 modelos de tres familias (`google/gemini-*`, `anthropic/claude-*`, `openai/gpt-oss-120b-medium`).
- **GitHub Copilot CLI (`symphony-adapter-copilot`):**
  - Se invoca sin `-p` (pasar `-p -` envía `-` como prompt literal; con `stdin` conectado a un pipe lee hasta EOF).
  - Clasifica errores de modelo no disponible o de autenticación desde `stderr` (`parse_stderr_line`).
- **OpenCode (`symphony-adapter-opencode`, ADR-0012):**
  - Normaliza los identificadores `proveedor/modelo` de `opencode models` a `opencode/<modelo>` (para el proveedor propio `opencode`) o `opencode/<proveedor>:<modelo>` (ej. `opencode/github-copilot:gpt-5`).
  - Cachea la salida de `opencode models` en `~/.symphony/cache/` (TTL de 24 h, hasta 200 modelos) actualizándola en segundo plano.
  - Como OpenCode toma la raíz del repo principal y no el worktree como directorio del proyecto, `frame_prompt` antepone la ruta del worktree y `run_config` inyecta `OPENCODE_CONFIG_CONTENT` permitiendo `external_directory` únicamente en `<worktree>/**`.

---

## 3. Suite de contrato y pruebas de un adapter

Todo adapter debe pasar la suite compartida `symphony_adapter_common::contract::check` (L1, sin red ni suscripciones reales) y una prueba de propiedades (`proptest`):

1. **Basura (`GARBAGE`):** líneas vacías, JSON truncado, secuencias ANSI y tipos desconocidos en `parse_stream_line`, `parse_hook` y `parse_error` nunca hacen `panic!` ni inventan eventos.
2. **Fixtures reales sanitizados (`fixtures/<proveedor>/`):** líneas de stream, payloads de hooks y mensajes de error verificados contra la secuencia exacta de `AgentEvent::type_name()` y `FailureType`, comprobando que secretos (`sk-`, `Bearer `) salgan redactados.
3. **Invariantes de `spawn_spec`:** el `ProcessSpec` corre con `cwd == worktree`, incluye el modelo pedido, propaga `SYMPHONY_AGENT_ID`, deja `stdin` habilitado y **no** contiene el prompt en `args`.

Ejecutar las pruebas de un adapter:

```bash
cargo nextest run -p symphony-adapter-claude
cargo nextest run -p symphony-adapter-opencode
```
