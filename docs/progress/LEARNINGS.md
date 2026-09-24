# LEARNINGS

Trampas descubiertas y comandos útiles. Anota en el momento, no al final.

## Prevalidación (P00.S0) — detalle en `docs/research/prevalidacion.md` §2.3

- **H1 · Los CLIs headless no siempre dejan TODOs estructurados.** `claude -p` no usó TodoWrite y `codex exec` no produjo `todo_list`; ambos escribieron el plan como texto. El checkpoint toma el **último mensaje del asistente** como fuente del "qué seguía" y usa TODOs estructurados solo si existen.
- **H2 · El transcript de Claude va detrás del disco tras un kill.** Un `Write` ya aplicado no aparecía en el `.jsonl`. Fuente de verdad: `git diff` + archivos nuevos. Si hay conflicto, gana git.
- **H3 · `claude -p --output-format stream-json` emite un evento de rate limit** con `five_hour.utilization`, `seven_day.utilization` y `resetsAt`. La cuota de Claude Code puede ser `KNOWN`. Confirmar en P01.S2.
- **H4 · Windows: lanzar un CLI desde Git Bash bloquea al padre** hasta que el hijo termina (hereda el pipe de stdout). `Start-Process -WindowStyle Hidden` + redirección a archivo lo evita. Para P04 (`ProcessSupervisor`): controlar explícitamente los handles heredables.
- **H5 · Codex tiene dos formatos de eventos:** en disco (`~/.codex/sessions/.../rollout-*.jsonl`, `response_item`/`event_msg`) y en `codex exec --json` (`item.*`). Soportar el modo interactivo implica dos parsers.
- **H6 · El costo del handoff depende de la config global del CLI.** Codex gastó ~880k tokens de entrada (835k cacheados) en una tarea pequeña porque cargó las skills de `~/.agents/skills`; Claude ~243k. Medir en P01.S3, considerar en P09.
- **H7 · La salida TAP de `npm test` entró cruda al handoff** (~2.5 KB). Evidencia a favor del colapso de logs de tests (P09.S3).
- **Kill en Windows:** `taskkill /F /T` sobre el shim `.cmd` de npm mata todo el árbol cmd → node sin dejar huérfanos.

## Entorno (P00.S1)

- **Rust en Windows necesita el workload "Desarrollo para el escritorio con C++" de Visual Studio** (link.exe + Windows SDK). Tener VS 2022 instalado no basta: sin ese workload `cargo run` falla con `linker link.exe not found`.
- **`git config core.autocrlf=true` en la máquina de Leo.** El repo fija `eol=lf` en `.gitattributes` para que rustfmt y la CI no vean diffs de fin de línea.
- **`cargo install --locked cargo-nextest cargo-deny` tarda más de 10 min en la laptop de Leo.** En CI se usan binarios precompilados (`taiki-e/install-action`, `cargo-deny-action`).
- **Git Bash + `python -`** abre el stub de la Microsoft Store y se cuelga. No uses python en scripts de shell en esta máquina.

## Contratos de CLIs (P01.S2) — detalle en `docs/research/cli-*.md`

- **Un `PreToolUse` vencido NO bloquea** ni en Claude ni (probablemente) en Codex: la herramienta sigue. El scheduler tiene que responder antes del `timeout` (600 s por defecto) y fijar `timeout` explícito en el hook.
- **`claude --bare` no usa la suscripción** (exige `ANTHROPIC_API_KEY`). No usarlo. Inyectar hooks con `claude -p --settings '<json>'`.
- **Codex exige confianza por hash para cada hook.** Para hooks generados por Symphony: `--dangerously-bypass-hook-trust`. No tocar `CODEX_HOME` (ahí vive `auth.json`).
- **`codex exec --json` y el rollout en disco son formatos distintos**, y la cuota (`rate_limits`) al menos está en el rollout.
- En Git Bash, `curl` está redirigido por un hook de context-mode: usa `ctx_fetch_and_index` o `ctx_execute` para bajar páginas.

## Test A (P01.S3) — detalle en `spikes/results/test-a.md`

- **No pases a los CLIs rutas con nombres cortos 8.3** (`C:\Users\LATITU~1\...`, que es lo que devuelve `%TEMP%` en esta máquina). Codex falló leyendo archivos y tardó 3.5× más. Usa rutas largas, sin `\?\`.
- **Los shims `.cmd` de npm** agregan `cmd.exe` + `conhost.exe` por agente y obligan a escapar los argumentos como batch. En Rust, `Command::new("claude")` no encuentra el `.cmd`: hay que pasar `claude.cmd` o resolver el exe real.
- **Codex hace `git fetch` (`git-remote-https`) al arrancar** en un repo con remoto.
- **En PowerShell, `@arr` con un solo elemento** llega distinto al exe nativo: la corrida N=1 del bucle falló con `NotADirectory`. Pasa rutas explícitas.

## Test B (P01.S4) — detalle en `spikes/results/test-b.md`

- **Codex corre los hooks en PowerShell en Windows.** `"C:/ruta con espacios/x.exe" args` no ejecuta nada y falla en silencio. Usa `& 'ruta' args`. Claude usa un shell tipo bash, donde las comillas sí funcionan.
- **Codex no carga `<worktree>/.codex/hooks.json`**, ni con `trust_level` pasado por `-c`. Lo que sí funciona: hooks inline `-c 'hooks.<Evento>=[{hooks=[{type="command",command="…"}]}]'` + `--dangerously-bypass-hook-trust`.
- **Claude:** `--settings <json>` inyecta hooks en `-p` sin tocar el worktree.
- **`codex exec --json` no trae `rate_limits`.** Están en el rollout de disco.
- **Prompts con comillas:** pásalos por stdin (`codex exec -`), no como argumento de un `.cmd`.
- **En el tool Bash de Claude Code, un `Remove-Item` de PowerShell dentro de un comando largo** puede ser bloqueado por el guard de rutas del sistema (malinterpreta `'\'` o `/c`). Borra en un comando aparte.

## Test C (P01.S5) — detalle en `spikes/results/test-c.md`

- **Retener con `PreToolUse`: Claude aguanta hasta el `timeout`; Codex solo ~60 s.** Con 120 s, el tool de Codex falla y el modelo reintenta, lo que duplica la espera. Para esperas largas, `deny` con razón.
- **Codex no mata el proceso de un hook vencido** (sigue vivo después de la sesión). El hook de Symphony necesita su propio límite interno.
- Los modelos no notan la retención: reportan tiempos normales.

## ProcessKit (P01.S7) — detalle en `spikes/results/test-processkit.md`, ADR-0002

- **Features:** `processkit = { version = "3.3", features = ["limits", "stats"] }`. Sin ellas no existen `max_memory`, `cpu_quota` ni `stats()`. `ProcessGroup::output_string` necesita `use processkit::ProcessRunner`.
- **Linux sin cgroup v2 delegado** (runners de GitHub): los límites dan `ResourceLimit { reason: Unenforceable }` al crear el grupo. **macOS:** `Unsupported`. Maneja el `Err`; no hagas `?`.
- **`LimitEvidence.memory` = `Unknown`** aunque el límite bloqueó (Windows). Infiere el OOM por exit code.
- **Contar procesos en Linux con `sysinfo`:** filtra `thread_kind().is_none()` y los zombies.
- **Codex en un corte de red** emite `{"type":"error","message":"Reconnecting... n/5 …"}` y luego "waiting for network", y **espera indefinidamente** (65 min en Test D). Hace falta un watchdog de inactividad por agente (P10).

## Test D (P01.S6) — detalle en `spikes/results/test-d.md`

- **6/6 handoffs OK** con un checkpoint incremental por hook + git vivo. Git es lo esencial; el plan y el último mensaje ayudan.
- **Ningún CLI usa TodoWrite / `update_plan` en headless.** El "qué seguía" sale del último mensaje del asistente en la cola del transcript.
- **Sandbox `workspace-write` de Codex en Windows:** `node --test` da `EPERM` al leer `C:\Users\<usuario>`. Codex lo termina esquivando, pero pierde minutos.
- **Mensajes a media tarea:** Claude `-p --input-format stream-json` los procesa dentro del turno. `codex queue` los acepta, pero ni `exec` ni `exec resume` los consumen.

## P02

- **`cargo deny` no revisa las licencias de dev-dependencies:** una crate que pasa de dev a normal puede traer licencias nuevas (pasó con `interprocess` → 0BSD).
- **En scripts de verificación usa `set -o pipefail`:** `cargo deny check | tail -1` devuelve 0 aunque deny falle.
- **`allow-unwrap-in-tests` de clippy solo cubre funciones `#[test]`**, no los helpers de `tests/*.rs`. Esos archivos llevan `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
- **En `ulid` 3, `Ulid::new()` pasó a llamarse `Ulid::generate()`.**
- **macOS no soporta `fchmod` en sockets:** `interprocess` `ListenerOptionsExt::mode` devuelve `Unsupported`. La garantía es el directorio `0700`.
- **`sun_path` es de 104 bytes en macOS:** el `$TMPDIR` del runner (`/var/folders/…`) lo supera. Hay fallback a `/tmp/symphony-<hash>/`, verificando dueño y permisos.
- **`Path::starts_with` compara componentes, no texto:** `"/tmp/symphony-abc".starts_with("/tmp/symphony-")` es `false`.
- **nextest corta en la primera falla (fail-fast):** que un test no aparezca en el log de CI no significa que pasó.
- **H4 en la práctica:** el daemon lanzado por el CLI heredaba el pipe de stdout de quien lanzó al CLI, y `trycmd` esperaba para siempre. Solución: `SetHandleInformation(…, HANDLE_FLAG_INHERIT, 0)` sobre los std handles propios antes del spawn (`crates/cli/src/client.rs`).
- **"Socket cerrado" no significa "daemon terminado":** el daemon borra el socket y después cierra la base y suelta el lock. Un `stop` seguido de un autoarranque chocaba con el lock (visto en CI de macOS). La señal confiable es el **lock de instancia libre** (`transport::daemon_lock_held`).
- **`git merge-tree --write-tree` devuelve exit 1 con conflictos y también con una rama inexistente.** Distinguirlos por el OID del tree al comienzo del stdout.
- **PELIGRO: en Windows, `git worktree remove --force` sigue las junctions y borra su destino** (p. ej. el `node_modules` del repo base enlazado con la estrategia LINK). `Repo::worktree_remove` suelta los enlaces del primer nivel antes de llamar a git. Hay un test.
