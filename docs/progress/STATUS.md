# STATUS — Symphony CLI

**Actualizado:** 2026-10-10 · por claude-code/opus-5.5
**Fase actual:** P09 · Context Engine (ADR-0010: P10 → P09 → P08, completas). P10 cerrada (`p10-done`, `v0.5.0`); P11 cerrada; P07.S10 cerrada por ADR-0010
**Paso actual:** P09.S8 · Broker MCP. Hechos en `phase/p09-context-engine`: S1 (migración 003), S2 (`ctx://`, chunks + FTS5), S3 (compresores), S4 (AST L0–L5 con tree-sitter), S5 (file watcher), S6 (handoff v2, `symphony context`), S7 (project facts + `symphony fact`), S10 (evaluación L2: BALANCED = 6,6 % de RAW sin perder la tarea). Faltan S8, S9 y S11. Después: OpenCode (ADR-0012), correcciones de Signalia y P08 con ADR-0011 (ver «Próxima acción concreta»). P12–P16 siguen provisionales
**Cierre de P07.5 (S9):** commit `5c6c6a4`, tabla de coste del handoff por modo (estimación) y coste real medido en vivo (`claude/haiku` + `openai/gpt-5.6-luna`, 10 turnos, n = 1), `ponytail-review` con alcance limitado, `health` no corrido (sin skill). Detalle y decisiones en `docs/phases/P07.5-chat.md` § S9.
**Estado del paso:** S1 ✅ (un mensaje tras el turno retoma la sesión; live Claude y Codex ✅) · S2 ✅ (instalación global documentada y verificada) · S3 ✅ (el handoff lleva la conversación, recortada por modo; 10 turnos en `raw` ✅) · S4 ✅ (chat idempotente en `symphony/chat`, turno termina en `READY`; L2 ✅, sin live) · S5 ✅ (commit por turno del chat y `AgentStateChanged` en el bus; L2 ✅, sin live) · S6 ✅ (cambio manual con mensaje, failover del chat y umbral de tokens opcional; L1/L2 ✅, sin live) · S8 ✅ (skills nativas de Claude y Codex verificadas en el chat; corregido `/skill` tras cambio de proveedor; live L3 ✅, ver bitácora) · S7 ✅ (vista Chat como inicio, `chat.get`, FLOW + Journey F; snapshots ✅; probado en terminal con live mínimo Claude→Codex ✅, 4 bugs corregidos; opinión de Leo documentada en `docs/research/uso-v0.1.md` § Día 2, pendiente de priorizar). Bitácora: `docs/phases/P07.5-chat.md`.
**En curso por:** nadie (cola lista para la próxima sesión)

## Salud del repo
- `cargo xtask check`: ✅ (427 tests en `phase/p09-context-engine`; los live se omiten sin `SYMPHONY_LIVE=1`). Los tests intermitentes del chat y de cuota (fallaban en Linux en 4 de 5 corridas del CI) tenían dos causas, ya corregidas (LEARNINGS P07.5): choque de git sobre el índice y `EPIPE` al escribir el prompt a un CLI que ya salió.
- `cargo deny check`: ✅ (solo avisos de duplicados)
- CI en main: ✅ (ubuntu, windows, macos, msrv, deny). La rama `phase/p07-tui` y el tag `v0.1.0` se pushean en P07.S8.
- Tests conocidos en rojo: ninguno.

## Progreso de la fase
- [x] P07.S1 Arquitectura TUI (suscripción al bus + métodos IPC de vistas)
- [x] P07.S2 Launch, First-run y Provider Setup
- [x] P07.S3 Home
- [x] P07.S4 New Agent y Model Picker básico
- [x] P07.S5 Vista de agente (+ «Abrir en el CLI», attach de ADR-0005)
- [x] P07.S6 Providers y Recovery Center
- [x] P07.S7 Snapshots y E2E (terminal real ✅ Leo; Journey A live con Claude Code ✅ 17 s)
- [x] P07.S8 Release v0.1.0 (tag `v0.1.0`, CHANGELOG, build release: daemon idle 16.5 MB)
- [x] P07.S9 Cierre (revisión de código: 1 hallazgo corregido, `spawn src/main.rs …` ya no se toma como modelo; `health` no corrido)
- [x] P07.S10 Replanificación (cerrada por ADR-0010: P10 → P09 → P08 completas)

## Progreso de P07.5
- [x] P07.5.S1 Continuar la sesión después del turno (`continue_session`)
- [x] P07.5.S2 Instalación global mínima
- [x] P07.5.S3 Handoff conversacional
- [x] P07.5.S4 Agente «chat general» (worktree `symphony/chat`, `Running → Ready`)
- [x] P07.5.S5 Commit por turno y estado en el bus
- [x] P07.5.S6 Cambio de modelo/proveedor
- [x] P07.5.S7 Vista Chat como inicio
- [x] P07.5.S8 Skills en el chat
- [x] P07.5.S9 Cierre (gate)

## Próxima acción concreta
Cola de trabajo, en orden. Base verificada el 2026-10-10: `cargo xtask check` ✅ (427 tests). La rama `phase/p09-context-engine` no está pusheada (13 commits sobre `main`). Cada paso: leer su sección del PLAN o su ADR, bitácora, commit `[PNN.SX]`, STATUS. **Los puntos marcados con ⛔ son de parada:** se documentan y se sigue con el siguiente que no dependa de ellos.

1. **P09.S8 · Broker MCP** (PLAN P09.S8): `symphony mcp serve` por stdio con `rmcp` 3.4 (ya aprobado en ADR-0001, falta en `[workspace.dependencies]`), `context.retrieve`, `context.search` y `context.lines` reenviados al daemon por IPC; registro en `context_retrievals` (`found = 0` = miss); los adapters lo registran por worktree en cada CLI que lo permita (Claude y Codex seguro; Kimi, Antigravity y Copilot: investigar y documentar si no se puede). Verifica: test de protocolo MCP. Live opt-in (que un CLI llame `context.search`): permitido solo con un modelo gratuito (p. ej. OpenCode, cuando exista); con Claude, Codex u otro que gaste cuota sigue ⛔.
2. **P09.S9 · Skills y MCP compartidos** (PLAN P09.S9): skills canónicas en `~/.symphony/skills` y `.symphony/skills` traducidas por adapter, registro `mcp.toml`, vistas 20, 21, 32, 33 y 35 (parte de contexto) con snapshots. Aplicar de paso el cambio menor pendiente: `scan_plugins` en `skills.rs` no necesita el parámetro `commands`.
3. **P09.S11 · Cierre** (PLAN §4.6): revisión de código, push de la rama, CI verde en ubuntu/windows/macos/msrv/deny, `merge --no-ff` a `main` y tag `p09-done` (Leo aprobó merge/tag/push de fase sin PR). Anotar como pendientes: live de S10 (RAW contra BALANCED) y live de S8.
4. **OpenCode como proveedor** (ADR-0012, rama `phase/p11-opencode`): agregar el paso `P11.S7 · OpenCode` al PLAN; crate `symphony-adapter-opencode` según ADR-0012 § Decisión (ids `opencode/<modelo>` y `opencode/<proveedor>:<modelo>`, failover automático, `resume` con `-s`); fixtures recapturados con un modelo gratuito y sanitizados; suite de contrato; registro del broker MCP de S8 si OpenCode lo admite; QUICKSTART (permisos: sin sandbox). Lo observado del CLI está en `docs/research/opencode-y-calidad-de-proveedores.md` § 2. Prueba live con modelos gratuitos de OpenCode (`opencode/…-free`, `opencode/big-pickle`): **permitida** por Leo (2026-10-10). No usar la credencial de Copilot dentro de OpenCode sin revisar antes `docs/research/tos.md`.
5. **Correcciones de Signalia sin ADR** (`docs/research/uso-signalia.md` § 6, rama `phase/p10-fixes`): **F5** un run que muere poco después de un rate limit o una cuota registrada termina con esa causa y no como `NO_HEARTBEAT`/`CRASH`; **F6** `resume` reanuda un agente `FAILED` con el mismo modelo (la transición `FAILED → READY` ya existe) sin pasar por `switch`. Si alguna exige tocar enums o transiciones del core: ⛔ ADR primero.
6. ⛔ **P08 · Decisiones de Leo antes de empezar.** Con lo de arriba hecho, se puede dejar listo, sin código: (a) una propuesta de ADR (estado PROPUESTO) que integre ADR-0011 (plan por fases y tareas, sesión nueva por fase, `/compact` entre tareas, configuración `auto`/`preguntar`, mínimo 2 tareas por fase) en el diseño de P08; (b) qué features candidatas de Signalia entran en P08 (F1 modo delegado del chat, F2 `agent.*` por MCP, F3 plan de lote, F4 concurrencia según cuota, F7 gate y commit por agente, F8 preparación del worktree, F9 capacidades por proveedor, F10 lista de modelos permitidos, F11–F13); (c) la decisión «phase + conditions» frente a los 12 estados (P08.S1); (d) la migración de Fase 2 será la 004, no la 002 que dice el PLAN. P08 no empieza sin la aprobación de Leo.

**Permiso de Leo (2026-10-10):** pruebas live (`SYMPHONY_LIVE=1`) **solo con modelos gratuitos** (hoy, los gratuitos de OpenCode). **Siempre ⛔ (necesitan a Leo):** cualquier prueba live que gaste cuota (Claude, Codex, Kimi, Antigravity, Copilot); la re-validación de Kimi, Antigravity y Copilot (criterios en `docs/research/opencode-y-calidad-de-proveedores.md` § 4; Leo: solo Claude Code y Codex le han funcionado bien); ToS de los proveedores de P11; permisos de proveedores en `config.toml`.

**Datos que siguen sirviendo:** un cambio de proveedor cuesta el arranque del CLI de destino (Claude ~27,5k, Codex ~18k tokens) más el handoff (~2,4k–2,9k), medido el 2026-09-30 con n = 1; falta una conversación larga (≥ 100 mensajes) y entender por qué Codex «continuar» sube a 48k.

## Propuesta clave para S10: Chat agentico con failover entre proveedores

**Descubierto en pruebas de Leo.** El atractivo principal de Symphony no es orquestar múltiples agentes autónomos complejos, sino ofrecer un **chat simple y agentico (como Claude Code o Codex) que cambia automáticamente entre proveedores cuando uno agota su contexto, sin perder historial**.

| Aspecto | Descripción |
|---|---|
| **Experiencia** | Chat familiar: escribe tareas, obtén respuestas con tools/skills. Selecciona proveedores disponibles (ej: Claude Code + Codex). |
| **Killer feature** | Cuando un proveedor agota tokens/contexto, pasa automáticamente al siguiente con el historial intacto. El usuario puede cambiar manualmente también. |
| **Contexto en failover** | Historial local en `.symphony/`; al cambiar, resume la sesión con lo acumulado: "aquí está todo hasta ahora, continuemos con...". Cada proveedor adapta a sus límites. |
| **Skills** | Hereda las skills del proyecto (build, test, review, etc.); se invocan según el proveedor activo. |
| **Ubicación en plan** | Decisión en S10; implementación en P08 si se aprueba. Recicla event bus, adapters y runtime de P05–P07. |

Véase `docs/research/uso-v0.1.md` § "Recomendación clave" para detalles y validaciones pendientes.

## Handoff para el siguiente agente
- **Dónde retomar (2026-09-30):** P07.5 cerrada y fusionada a `main` (tag `p075-done`). Empieza por **P07.S10** (replanificación de P08–P16) en una rama nueva; la bitácora de P07.5 (`docs/phases/P07.5-chat.md`) tiene el cierre, las decisiones de S9 y lo que quedó sin medir. Corre `cargo xtask check` antes de tocar código.
- **Contexto de esta sesión:** `docs/research/spike-p08-decisiones.md` y `docs/adr/0006-chat-general-y-replanificacion.md` tienen las decisiones y su evidencia; `docs/progress/SESSIONS.md` y `LEARNINGS.md` (sección P07.5) resumen lo hecho y aprendido.
- **Pistas para S4 (agente «chat general»)** — verificar en el código antes de fiarse:
  - `runtime.rs` `create_agent` crea task + worktree + branch + checkpoint + run; la rama sale de `symphony_git::agent_branch(...)`. El chat necesita rama fija `symphony/chat` basada en `main` y ser idempotente (uno por proyecto): decidir cómo se identifica (¿`tasks.kind`? revisar los CHECK de DB §3.B y `docs/spec/symphony_database.md`).
  - `executor.rs` `finish_natural`: con exit 0 hoy pone `Completed` + task `Done`. Para el chat debe dejar `Ready` («esperando mensaje») y la task abierta. Requiere `Running → Ready` en `core/src/transitions.rs` (tabla exacta + tests, sin migración).
  - `continue_session` ya admite agentes `Ready`; `switch()` ya admite `Ready` pero no `Completed` (S6).
  - S6: al cambiar de modelo con un mensaje nuevo, guardar ese mensaje como `USER` aparte (el primer `USER` de un run con handoff se descarta al armar la conversación).
- **Instalación local:** hay una sola copia de `symphony`/`symphonyd` en `~/.cargo/bin` (con S1, sin S3). Para probar lo nuevo: `symphony daemon stop` y los dos `cargo install --path … --locked --force` (QUICKSTART §1).
- **Ramas remotas:** `spike/p08-decisiones` ya está fusionada en `main` y puede borrarse.
- La TUI está en `crates/tui`: `app.rs` tiene el estado y la lógica pura, `ui.rs` el render, `io.rs` el IPC y `lib.rs` el loop de terminal. Lee «Decisiones tomadas» en la bitácora P07.
- Para regenerar los snapshots: `INSTA_UPDATE=always cargo nextest run -p symphony-tui`. Revísalos antes de commitear.
- Si un test deja un `symphonyd.exe` huérfano, el build falla con «Acceso denegado» (LEARNINGS P07).

## Bloqueos y preguntas para Leo
- (ninguno)

## Pendientes arrastrados
- P06–P07: repositorios de executor_changes, messages, provider_failures, tool_calls, checkpoints, checkpoint_refs, handoffs y recovery_items (bitácora P03.S3).
- Config de proveedores: `--permission-mode` de Claude (default `acceptEdits`) y sandbox de Codex (default `workspace-write`) deberían salir de `config.toml`.
- (Resuelto en P07.5.S1: mensajes a Claude después de su turno, `resume`.) Decisión del 2026-09-30: los dos pendientes de arriba se **difieren**; ningún criterio de salida de P07.5 depende de ellos.

## Fases
| Fase | Estado | Tag |
|---|---|---|
| P00 | ✅ | p00-done |
| P01 | ✅ | p01-done |
| P02 | ✅ | p02-done |
| P03 | ✅ | p03-done |
| P04 | ✅ | p04-done |
| P05 | ✅ | p05-done |
| P06 | ✅ | p06-done |
| P07 | ✅ (S10 pendiente, diferido por Leo) | p07-done, v0.1.0 |
| P07.5 | ✅ | p075-done |
| P11 (adelantada por ADR-0008) | ✅ | p11-done |
| P10 | ✅ | p10-done, v0.5.0 |
| P09 | 🟡 en curso | |
| P08 | ⏳ (tras P09) | |
| P12–P16 | ⏳ (provisionales) | |
