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
- **Comparaciones de tiempo con margen 0 tienen que ser `<=`:** en Linux, crear y consultar pasa en el mismo milisegundo (el GC con `grace = 0` no borraba nada en CI).
- **Tests del CLI con un `symphonyd` viejo:** cargo no reconstruye binarios de otros paquetes para `-p symphony-cli`. `tests/common::symphonyd()` corre `cargo build -p symphony-daemon` una vez por proceso.
- **La recuperación al arrancar trabaja con IDs como texto:** una fila con un ID inesperado impedía arrancar el daemon.
- **En Unix, un CLI que aborta llega como `ExitStatus::Killed(Some(señal))`, no como exit code** (en Windows es `Exited(134)`). Una señal que Symphony no mandó es un crash: run `FAILED`. `KILLED` solo para stop/kill del usuario.

## P07

- **Un test que lanza `symphonyd` y falla deja al daemon huérfano**, y este hereda el pipe de nextest: `cargo nextest … | grep` se queda colgado para siempre, y el `symphonyd.exe` huérfano bloquea `target/debug/symphonyd.exe` («Acceso denegado» en el siguiente build). Envuelve el `Child` en un guard que lo mate en `Drop` (`KillOnDrop` en `crates/daemon/tests/daemon.rs`, `Daemon` en `crates/tui/tests/daemon.rs`). Para limpiar a mano: `tasklist | grep symphonyd`, confirmar que el ejecutable es de `target/debug` y `taskkill //PID <pid> //F`.
- **En PowerShell de esta máquina `cargo` no está en el PATH;** en Git Bash sí: `export PATH="$HOME/.cargo/bin:$PATH"`.
- **`cargo xtask check 2>&1 | tail` en segundo plano no muestra nada hasta el final:** redirige a un archivo (`> "$TEMP/check.log" 2>&1`) y filtra después.
- **ratatui 0.30:** `ratatui::init()`/`restore()` ya ponen el modo raw, la pantalla alterna y el hook de panic. crossterm se usa por el re-export `ratatui::crossterm` (no hace falta otra dependencia). Para `.add_modifier` sobre un `Span` hace falta `use ratatui::style::Stylize`.
- **Windows manda eventos `KeyEventKind::Release`:** si no se filtran, cada tecla cuenta doble.
- **Claude Code niega hasta los `Read` si su cwd es una ruta 8.3** (`C:\Users\LATITU~1\…`): «the permission system is flagging the Windows path». `%TEMP%` y `tempfile` devuelven rutas cortas en esta máquina. El daemon normaliza su home con `transport::long_path`. Para probarlo a mano no uses `cd` de bash (normaliza solo); usa `cmd //c "cd /d <ruta corta> && claude …"`.
- **`claude -p --input-format stream-json` no sale al terminar el turno:** espera otro mensaje por stdin. El executor cierra stdin al ver el `result` (adenda de ADR-0005).
- **No lances el daemon con `DETACHED_PROCESS` en Windows:** anula `CREATE_NO_WINDOW`, y cada programa de consola que lanza el daemon (git, CLIs) espera ~3 s a que Windows le cree una consola. Con `CREATE_NO_WINDOW` solo, el daemon tiene una consola oculta que heredan sus hijos. El síntoma: `agent.create` con timeout y `daemon start` de más de 10 s.
- **El daemon sigue vivo al salir de la TUI** (a propósito). Después de una prueba manual queda un `symphonyd.exe` que bloquea `target/debug/symphonyd.exe` al recompilar: `symphony daemon stop` con el mismo `SYMPHONY_HOME`.
- **Para ubicar un cuelgue dentro del daemon:** los logs `info` no alcanzan; agrega `tracing::info!("DBG …")` temporales por paso en el runtime y lee `<home>/logs/symphonyd.*.log`. Así se encontraron los 9.7 s de git.
- **Un «flake» por timing puede ser un bug:** el de `forced_kill_test_d_acceptance_test` era el watchdog, que contaba como silencioso a un run sin su primera línea todavía. Antes de subir un umbral en un test, confirma que la aserción que falla sea compatible con ese umbral (aquí el test duraba menos que el umbral nuevo y seguía fallando).

## P07.5

- **`codex exec resume` no acepta `-s`:** el sandbox va con `-c sandbox_mode="…"` (el adapter ya lo hace). `codex exec` sí acepta `-s`.
- **Un prompt de prueba «Recuerda el código 7431» con Claude Code no sirve:** lo interpreta como guardar en su memoria persistente y se niega. Para probar que `--resume` conserva contexto usa un juego: «el código del juego es 7431, repítelo» y luego «dentro del juego, ¿cuál era?».
- **Los CLIs compactan solos** (`claude --help` muestra `--autocompact`; ambos documentan hooks `PreCompact`/`PostCompact`): «contexto lleno» no llega como error. El cambio de proveedor por contexto es una política de Symphony sobre el uso de tokens (`turn.completed.usage` en Codex, `result` en Claude). Inferido de la ayuda, no probado con un contexto real.
- **El prompt de un handoff queda guardado como mensaje `USER`** (el primero de cada run que arrancó desde un checkpoint). Al armar una conversación hay que descartarlo o los handoffs se anidan. `repo::handoff_run_ids` da esos runs.
- **`Completed` y `Done` eran terminales sin salida:** un mensaje tras el turno necesitó `Completed → Ready` y `Done → Ready`. `Running → Ready` sigue sin existir (S4). `switch` rechaza hoy a un agente `COMPLETED` (S6).
- **En Git Bash de Windows, un `PATH="C:/…:$PATH"` se rompe** (los dos puntos de la unidad parten la ruta) y `which symphony` muestra otro ejecutable. Para PATH usa PowerShell (`$env:PATH = "C:\ruta\bin;$env:PATH"`) o rutas `/c/…`, y comprueba con `Get-Command symphony -All`.
- **Copias viejas de `symphony.exe`/`symphonyd.exe` en `%APPDATA%\npm` (antes que `~/.cargo/bin` en el PATH)** taparían un `cargo install` nuevo. Ya se borraron; si reaparecen, `Get-Command symphony -All` las muestra. `symphony` busca `symphonyd` junto a su ejecutable, así que deben venir de la misma carpeta.
- **`cargo nextest run <filtro>` filtra por nombre de test, no de archivo:** `live_resume` no encontró nada; el filtro correcto fue `remembers_after_a_message`.
- **Para editar con scripts largos desde el shell,** escribe el `.js` a un archivo y ejecútalo con `node`; un heredoc con comillas y backticks rompió el parser del shell.
- **Un `grep -r` desde la raíz del repo recorre `target/` y tarda minutos.** Acota a `crates/` o usa la herramienta de búsqueda.
- **Un `tokio::spawn` que retiene un `broadcast::Sender` cuelga a los lectores:** el reenviador de estados del bus mantenía vivo el canal y `slow_subscriber_never_blocks…` esperaba un `Closed` que nunca llegaba. Usa `Sender::downgrade()` (`WeakSender`) en tareas de fondo que solo reenvían.
- **Un `cargo xtask check` colgado deja `cargo-nextest.exe` y el `.exe` del test vivos** y el siguiente build falla con `link.exe` 1104. Míralos con `tasklist | grep -iE "nextest|bus-|runtime-"` y ciérralos con `taskkill //F //PID`.
- **Cambios de estado de agentes:** `repo::set_agent_state` los anota en una cola del hilo escritor y se difunden solo tras el `commit` (`WriterHandle::state_changes`). No los emitas desde el runtime: se pierde el rollback.
- **Formatos reales de uso (fixtures):** Claude `result.usage` = `input_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens`, `output_tokens`; Codex `turn.completed.usage` = `input_tokens` (con `cached_input_tokens` aparte). El stream de Codex no emite `TurnFinished`, solo `TurnUsage`: el fin de turno es el exit 0.
- **El uso de contexto es por run:** filtrar `TurnUsage` por `run_id`. Si se toma «el último del agente», un run sin uso hereda el de su antecesor y el chat rebota entre proveedores.
- **Coste de un handoff con conversación (S9, estimación ~4 car./token):** `raw` crece ~205 tokens por mensaje (200 mensajes ≈ 41k); `safe`, `balanced` y `aggressive` tienen tope (~30k, ~10k, ~2,8k). Tabla y cómo regenerarla en la bitácora P07.5 (S9). El coste real facturado por cada proveedor no se midió (pide permiso de Leo).
- **Test intermitente bajo carga:** `chat_switches_provider_mid_conversation_with_a_new_message` falló una vez en `cargo xtask check` (faltaba el commit «chat: turno 2» con el agente ya `READY`) y no se reprodujo en 7 corridas más. Si reaparece: `commit_chat_turn` solo registra con `warn` un fallo de git, así que mira los logs del daemon antes de tocar el test.
- **Coste real de un cambio de proveedor (S9, live):** lo domina el arranque del CLI de destino, no el handoff. Claude ~27,5k tokens de entrada en su primer turno; Codex ~18k (por diferencia); el handoff de Symphony añade ~2,4k–2,9k con ~12–14 mensajes. `TurnUsage` mezcla caché y entrada nueva: sirve para comparar contextos, no para facturar. Dato sin explicar: Codex reporta 48k en el turno siguiente a un cambio (20,7k en el del cambio). Mídelo con `live_cost_of_provider_switches` (`crates/tui/tests/live_cost.rs`).
- **Dos causas de los tests intermitentes de `runtime.rs` en Linux (resueltas):** (1) varios `git` sobre el mismo worktree a la vez se pisan el índice (Windows: «Permission denied»; Linux: `index.lock`); ahora todo git corre con `GIT_OPTIONAL_LOCKS=0` y `commit_all` toma un candado de escritura por worktree (`crates/git`). (2) Un CLI que falla rápido sale antes de que el daemon escriba el prompt y la escritura da `EPIPE`; eso no es un fallo de arranque (`launch_inner` tolera `BrokenPipe`). Los errores de commit del turno solo se registran con `warn`: si un turno queda sin commit, mira primero los logs.
- **Cómo diagnosticar un test que solo falla en el CI:** `nextest` muestra la salida de los tests fallidos; empuja una rama `phase/**` desechable con `eprintln!` y relanza con `gh run rerun` hasta que falle. Ahorró instalar Rust en WSL. No te fíes de una sola pasada en verde: un fallo con ~50 % de probabilidad pasa solo la mitad de las veces.

## P11
- **Prompt por stdin en los tres CLIs nuevos:** Kimi `--print` sin `-p`; Copilot sin `-p` (stdin entubado = no interactivo); Antigravity `--print= --input-format stream-json` con `{"event":"user","message":{"content":…}}`. `agy --print` exige el prompt como valor y `copilot -p -` toma `-` como texto.
- **El id de sesión llega de tres formas:** Kimi por stderr (ADR-0009), Copilot en el `result` final (o fijado con `--session-id`), Antigravity en `init`. Kimi `-S <uuid>` crea la sesión con ese id.
- **En headless los tres deniegan o aprueban solos:** Kimi `--print` aprueba todo; Antigravity y Copilot niegan lo que pide permiso (Antigravity además devuelve `SUCCESS` con respuesta vacía y `denied_actions`; el paso de la herramienta denegada sale `DONE`). `agy --sandbox` se cuelga en Windows.
- **`agy` y `copilot` se actualizan solos** (agy 1.2.11 → 1.2.14 en una sesión; Copilot 1.0.59/1.0.60 según el flag). Las fixtures guardan la versión con la que se grabaron.
- **`copilot --version` tarda 1,7–3,4 s** (`kimi` 0,5 s, `agy` 0,3 s). `detect_all` va en paralelo; al añadirlo en serie, cada arranque de daemon pasó de ~0,5 s a ~5 s.
- **Carrera de tests en Windows (existente, ahora más visible):** tras compilar el workspace con nextest, el helper `common::symphonyd()` (`cargo build -p symphony-daemon`) vuelve a compilar el daemon, y si otro test ya tiene `target/debug/symphonyd.exe` en marcha, falla con «Acceso denegado (os error 5)». Pasa o falla según el solape de los tests; con el arranque más lento fallaba siempre en `cli_commands` y `cli_restarts_a_daemon_that_died_without_cleanup`. Paralelizar la detección lo evitó, pero la causa de fondo sigue (los helpers deberían copiar el `.exe` antes de usarlo).
- **Test de contrato y redacción:** el chequeo busca «bearer » en el mensaje; un ejemplo sintético debe llevar el encabezado `Authorization:` para que la regla de redacción lo consuma entero (como el de Codex).
