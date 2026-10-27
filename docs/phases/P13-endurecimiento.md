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

## S6 · Documentación (delegado)
- Libro mdBook en `docs/book/` (Antigravity), `cargo xtask er` con el ER (Kimi), QUICKSTART al
  día (OpenCode, modelo gratis). Revisados e integrados.
- Capítulos completos en `docs/book/src/` (`adapters.md`, `plugins.md`, `configuracion.md`, `comandos.md`, `troubleshooting.md`, `remoto-ssh.md`) e índice `SUMMARY.md` actualizado (`mdbook` no está instalado en el entorno local).

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
