# P13 · Endurecimiento y pre-release

Rama `phase/p13-endurecimiento`. Pasos en PLAN.md § P13. Desde 2026-10-24 parte del trabajo se
delega con Symphony mismo (dogfooding, `docs/progress/DOGFOOD.md`).

## S0 · Decisiones de Leo (2026-10-24)
- El harness de Symphony pasa a ser el defecto (`[harness] mode = "symphony"`).
- Antigravity ofrece sus 13 modelos (Google, Anthropic, OpenAI), con el proveedor en el id y en
  el nombre. Copilot solo tiene `auto`.
- La validación por tipo de proyecto (recortada de ADR-0020) va a P16.S0.
- El arreglo de tokio (RUSTSEC-2026-0386) entra a `main` al cerrar P13.
- Live sin tope, solo para mejorar Symphony y con plan por agente; preferir Kimi, OpenCode y
  Antigravity.

## S1 · Seguridad
Revisado: IPC, `hook emit`, `ctx://`, plugins, redacción, permisos del socket y el harness nuevo
(P15.5).
- **IPC:** ya estaba bien (P02.S8). Windows: DACL solo del dueño (`D:P(A;;GA;;;OW)`) y el pipe
  rechaza clientes remotos (`interprocess` pone `PIPE_REJECT_REMOTE_CLIENTS` por defecto; visto
  en su fuente 2.4.5). Unix: directorio `0700` propio y socket `0600`. El cliente verifica el
  pid del servidor. Frames de 8 MB como máximo.
- **`hook emit`:** stdin acotado a 4 MB.
- **`ctx://`:** se rechazan `..`, `.`, `%`, `\`, rutas absolutas y letras de unidad (test de
  intentos de escape).
- **Plugins:** corren con los permisos del usuario, como cualquier programa que instala.
- **Harness (hallazgos, corregidos):** `shell` juntaba toda la salida en memoria antes de
  recortarla (un comando que escupe gigas tumbaba el broker) y `read` cargaba el archivo entero.
  Ahora `shell` guarda solo cabeza y cola y `read` lee línea a línea. Tests:
  `a_huge_output_keeps_only_head_and_tail_in_memory`, `read_streams_lines_and_still_rejects_binaries`.
- **tokio 1.54.1** por RUSTSEC-2026-0386 (`cargo deny check` verde).

## S2 · Fuzzing (targets y propiedades, parte 1)
- `fuzz/` (cargo-fuzz, fuera del workspace, `[workspace]` vacío) con 4 targets:
  `frame_decoder` (FrameDecoder), `ctx_uri` (CtxUri::parse), `redact` (redact) y
  `claude_stream` (ClaudeAdapter::parse_stream_line). Necesita nightly; en Windows/MSVC
  no corre — WSL o job de CI en ubuntu (README de `fuzz/`). No se compiló aquí.
- Propiedad proptest equivalente en el crate dueño (corre en stable/nextest/CI):
  `frame::tests::arbitrary_bytes_never_panic`, `uri::tests::parsing_never_panics_and_…`
  (ya existía), `redact::tests::redact_never_panics_and_is_idempotent` (+ bytes
  arbitrarios) y `tests/stream_proptest.rs` (línea/JSON arbitrarios, determinismo).
- Ningún proptest encontró panic real: los 4 parsers ya devolvían Result / ignoraban
  basura. 145 tests de los 4 crates en verde; clippy `-D warnings` limpio.
- Pendiente para cerrar S2: correr cada target 30 min con nightly y agregar corpus a
  CI (corto), o documentar ⛔ si no hay nightly disponible.

## S3 · Recuperación (parte 1, delegado)
- Checkpoint corrupto (FLOW §16, `openai/gpt-6-astra` + integrador): antes de un handoff y al
  arrancar (agentes vivos, best effort) se valida el último checkpoint (resumen JSON legible y
  blob del diff presente y sano). Uno corrupto se marca `is_valid = 0`, abre un recovery item
  `CHECKPOINT_INVALID` (uno solo por checkpoint) y se usa el anterior; si no queda ninguno, el
  handoff sale del objetivo de la tarea y del estado vivo del worktree. El checkpointer tampoco
  arrastra objetivo ni paso siguiente de uno inválido. Tests: blob roto, JSON roto al arrancar,
  sin checkpoints válidos y `writer_crash` (matar el proceso a media transacción: integridad ok,
  ni run ni checkpoint a medias).
- `crates/daemon/tests/soak.rs` (Antigravity con Claude Sonnet 5.5): tandas de fake-agents,
  paradas a media tarea y reinicios del runtime; comprueba runs abiertos, `integrity_check` y
  RSS. 60 s: 22 tandas, 2 reinicios, RSS 25 → 25 MB. Falta la corrida de 4 h (Journey E) y el
  checkpoint corrupto.

## S5 · Calidad (parte 1, delegado)
- `_typos.toml`, `lychee.toml` y job `docs` en CI (OpenCode con GLM 5.3 Flash gratis), con
  `continue-on-error` hasta revisar la primera corrida.
- **Auditoría del CLI (`crates/cli/src/harness.rs`, `hook.rs`) — verificada y corregida:**
  - `write`/`edit` ahora usan `write_atomic` (temporal en misma carpeta, `sync_all`, `rename`): fallo a mitad deja la versión previa intacta. Test `replacing_a_file_does_not_truncate_the_previous_version`.
  - `read` lee línea a línea y se detiene al completar la página (`fill_buf` para saber si hay más sin recorrer el resto). Test `read_does_not_parse_beyond_the_requested_page`.
  - `grep` recorre archivos en flujo, se detiene al alcanzar el máximo y marca búsqueda parcial. Tests `grep_reports_a_partial_search_at_the_requested_limit`, `grep_caps_the_requested_number_of_results`.
  - `glob` corta el recorrido en `GLOB_RESULTS` y marca `partial`. Test `glob_reports_a_partial_search_at_its_limit`.
  - `GREP_MAX = 1000` como tope duro de `max_results` (`clamp`). Test `grep_caps_the_requested_number_of_results`.
  - `shell` retiene salida en búfer compartido (`Captured`/`drain`/`grab`); al vencer la espera devuelve lo leído en vez de descartarlo. Tests `shell_keeps_output_when_a_descendant_holds_the_pipe_open`, `a_huge_output_keeps_only_head_and_tail_in_memory`.
  - Todos los tests de unidad de `symphony-cli` pasan (17/17); clippy `-D warnings` y `fmt --check` limpios.
  - Seguimiento completo en `docs/research/auditoria/cli.md` § Seguimiento.
  - `hook.rs:12-39` (baja) queda **diferido**: `emit()` trunca con `stdin().take(MAX_PAYLOAD)` y descarta sin informar. Reportar el exceso tocaría el contrato "sin salida / nunca rompe al CLI" (hook.rs:7-8); requiere revisar el contrato del hook antes de tocarlo (candidato a ADR).
- **Auditoría del motor de contexto (`crates/context`, `crates/daemon`, `crates/store`) — verificada y corregida:**
  - `live_git` lee los archivos nuevos con un presupuesto agregado (`NEW_FILES_TOTAL_MAX` = 8 MB): pasado el tope el archivo se nombra sin contenido. Tests `untracked_files_share_one_total_budget`, `a_huge_new_file_never_enters_the_budget`.
  - `context_index`: `prepare` (chunks + compresión) fuera del hilo único del writer y `apply` solo escribe dentro de la transacción; lo usan `recorder`, `checkpoint` y `watcher`. Test `prepare_no_toca_la_base_y_apply_escribe_chunks_y_compresión`.
  - `watcher`: el callback filtra ocultos/gitignore (`wanted`), la cola es acotada (`WATCH_QUEUE`) con `rescan` al desbordarse, el debounce tiene tope (`MAX_BURST`) y un borrado de directorio limpia por prefijo de URI. Tests `the_callback_filter_drops_hidden_and_ignored_paths`, `deleting_a_directory_uri_removes_its_indexed_files`.
  - `handoff`: `message_sources` deja bajo el cerrojo solo filas y hashes; los blobs se leen sin el lock del lector.
  - `render_diff` reporta `CLIPPED` cuando además del resumen recorta. Test `a_reduced_diff_that_still_does_not_fit_reports_clipped`.
  - Quedan fuera: estimar el RAW sin construirlo, caché de los `.gitignore` anidados, `HandoffError` tipado y `file_under` sin llamador. El `stale` por milisegundos toca el esquema del checkpoint: **requiere ADR**.
  - Seguimiento completo en `docs/research/auditoria/context.md` § Seguimiento.

## S4 · Rendimiento y disco (adelantado por pedido de Leo: «Symphony debería ser super ligero»)
- RAM: el daemon en reposo usa 26 MB.
- Disco: worktrees que se borran solos (cancelado, integrado o completado hace 7 días; nunca
  con cambios sin commitear), `target/` de Cargo compartido entre agentes con tope de 15 GB,
  índice de contexto que suelta lo borrado o ignorado. Detalle en DOGFOOD.md.
- Diagnóstico: la cola de stderr de un CLI caído queda con el agente.
- Retención (DB §8): al arrancar el daemon se purgan en lotes de 5 000 (por el writer
  único, sin hilos nuevos) los `events` de agentes terminales/archivados más viejos que
  `[retention].events_days` (30) y las `resource_samples` más viejas que
  `[retention].samples_days` (14). Checkpoints, runs y eventos referenciados por
  checkpoints o `project_facts` no se tocan. Config nueva en `crates/core` y QUICKSTART §13.
- Harness E2E completo (STACK §25.2, `crates/daemon/tests/benchmark.rs` — `cargo nextest run -p symphony-daemon --test benchmark --no-capture`).
  Nota: medición tomada con la máquina cargada con otros agentes compilando y corriendo en paralelo (incluido el soak de 4 h). En los escenarios `10k/100k events`, `cola p*` mide la latencia de lectura de vistas (`list_agents_rows` + `home_rows` + `views::tasks` + `views::activity`) y `evt p*` la latencia de persistencia por evento amortizada en lotes:

```text
escenario                  agent   tiempo    RAM Δ  RAM proc CPU avg CPU p50 CPU p95 hijos  startup cola p50 cola p95  evt p50  evt p95  evt p99 ckpt p95  handoff failover
1 agente                       1   2162ms      0MB      30MB      0%      0%      0%     0    745ms       —        —       1ms     18ms     18ms    382ms       —        — 
3 agentes / 3 worktrees        3   4543ms    154MB      32MB    100%    100%    100%     4   2276ms       —        —       1ms     10ms     18ms   1768ms       —        — 
3 agentes + prueba pesada      3  28828ms    166MB      34MB     85%     86%     99%     4   2772ms  10133ms  20776ms      0ms     10ms     32ms   1024ms       —        — 
forced failover                1   2113ms     13MB      34MB     91%     88%    100%     2    664ms       —        —       1ms      2ms     15ms    861ms    176ms    432ms
daemon restart                 2   4311ms     14MB      35MB     87%     88%     98%     1     25ms       —        —       1ms      2ms     51ms   1101ms    190ms       — 
large logs (4 MB)              1   9582ms    569MB      59MB     94%     95%    100%     2    568ms       —        —     304ms    432ms    451ms   1342ms   2140ms       — 
10k events (write/views)       1   1261ms     73MB      41MB     92%     92%     95%     0       —       2ms     30ms   0.11ms   0.21ms   0.21ms       —        —        — 
100k events (write/views)      1  10079ms      3MB      55MB     78%     80%     96%     0       —       1ms     16ms   0.09ms   0.17ms   0.18ms       —        —        — 
```

## S6 · Documentación (delegado)
- Libro mdBook en `docs/book/` (Antigravity), `cargo xtask er` con el ER (Kimi), QUICKSTART al
  día (OpenCode, modelo gratis). Revisados e integrados.
- Capítulos nuevos: adapters, referencia de comandos (verificada contra `--help`) y cómo escribir un plugin (gemini-4-argon-high); solución de problemas suma 5 hallazgos del dogfooding y la guía SSH ya existía (`uso-remoto-ssh.md`). El libro compila con mdbook (WSL y CI).

## S7 · Pipeline de release (dry run)
- cargo-dist 0.32 (`dist-workspace.toml`): `symphony` y `symphonyd` como dos apps (los
  instaladores las dejan en la misma carpeta), zip/tar.xz para Linux x64, macOS arm64/x64 y
  Windows x64, instaladores shell y PowerShell, fórmula de Homebrew como artefacto (sin tap:
  publicarla es aparte), sha256 y attestations de GitHub. `release.yml` generado por
  `dist generate`: solo publica al pushear un tag `v*` (⛔ Leo).
- Dry run: job `release-dry-run` en CI (push a `phase/**` o a mano) con `dist build` local por
  OS y global en Linux, artefactos del workflow, sin release. Local: `dist plan` y
  `dist build` en Windows (zip de 3,4 MB y 7,1 MB, sha256 verificado, `symphony --version` del
  zip). Ojo: `dist` falla si se corre desde `desktop` en minúsculas (compara rutas con
  mayúsculas); usar la ruta real `Desktop`.
- Bundles de la GUI: `gui-release.yml` los adjunta a un release ya publicado; en fase siguen
  saliendo como artefactos del job `gui bundle`.
- CHANGELOG con git-cliff 2.14 (`cliff.toml` existente): se genera al etiquetar en S8.
- El libro se compila en CI (`mdbook build` en el job `docs`); verificado en WSL.

### S5 · Auditoría del executor (gpt-6.1-sol → claude-sonnet-5-5 → gemini-4-argon-medium)
- Corregidos los hallazgos 5 (el pump mata el árbol y drena la salida con `abort_unpublishable`
  cuando el bus/base deja de aceptar escrituras), 7 (el pump recuerda `suspended` y sigue
  latiendo mientras el agente está pausado para que el watchdog no lo mate), 13 (`tracing::warn!`
  cuando `config.toml` es ilegible al leer `os_limits` y `harness`) y 15 (comentario de `kill`
  aclarado como alias de `stop`).
- Tests de regresión añadidos en `crates/daemon/tests/runtime.rs`:
  `a_paused_executor_is_not_killed_by_the_watchdog` y
  `a_pump_that_cannot_publish_kills_the_cli_instead_of_hanging`.
- Los hallazgos 2, 3, 4, 6 y 8–12 quedan diferidos con justificación detallada en la tabla de
  `docs/research/auditoria/executor.md`.
- Verificación: `cargo nextest run -p symphony-daemon`, `cargo clippy -p symphony-daemon --all-targets -- -D warnings` y `cargo fmt --all` en verde.

## S8 · Cierre (2026-10-27, noche de dogfooding)
- **Revisión de código** de la rama (subagente claude-code/sonnet, solo lectura,
  `docs/research/auditoria/revision-p13.md`): 1 alta (el GC borraba worktrees con archivos nuevos
  sin commitear) y 3 medias corregidas; el resto diferido con su razón.
- **ponytail-audit** (`docs/research/auditoria/ponytail.md`): `now_ms` unificado en el daemon y una
  función muerta fuera; una propuesta era falsa (`FrameDecoder` lo usa el fuzzing).
- **Hallazgos del dogfooding arreglados en esta fase:** `build-dir` compartido, comando de las
  llamadas `shell` del harness registrado, aviso de disco casi lleno, variables internas fuera del
  entorno de los CLIs.
- **Gate:** `cargo xtask check` verde con 717 tests (680 al empezar la noche); CI verde en los tres
  sistemas, `deny`, `docs` (bloqueante), cobertura, MSRV y release en dry run.
- **Versión 0.9.0** y CHANGELOG con git-cliff.
- **Soak de 4 h (S3):** 591 tandas, 441 reinicios en el mismo proceso, `integrity_check` ok y ningún run abierto de más, pero el RSS creció de 25 a 171 MB (tope del test: +100 MB). El primer intento cayó a las 2 h por disco lleno. **Causa:** el watchdog de `Runtime::new` era un bucle infinito con un clon fuerte del `Runtime`, así que cada reinicio dejaba uno vivo (con su lector de SQLite, bus y worker de checkpoints; ~0,25 MB cada uno). En el daemon real solo ocurre una vez, pero el soak reabre el runtime 441 veces. **Arreglo:** el watchdog guarda un `Weak` y termina al soltarse el runtime. Medido (200 s, reinicio cada 5 s): antes 29 → 36 MB, ahora 25 → 25 MB; sin reinicios ya era plano (25 → 27 MB). Falta repetir las 4 h antes de cerrar la fase.
- **Queda para después:** guardia de aislamiento (agentes que escriben fuera de su worktree), tope
  absoluto del watchdog con CPU, arreglos de `server.md` (agente #39, sin integrar), hallazgos
  diferidos de las auditorías.
