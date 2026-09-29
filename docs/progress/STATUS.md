# STATUS — Symphony CLI

**Actualizado:** 2026-09-28 · por claude-code/sonnet-5.5
**Fase actual:** P07.5 · Chat general (rama `phase/p075-chat`, ADR-0006 aceptado)
**Paso actual:** P07.5.S4 · Agente «chat general» (S1–S3 hechos). P08–P16 siguen provisionales
**Estado del paso:** S1 ✅ (un mensaje tras el turno retoma la sesión; live Claude y Codex ✅) · S2 ✅ (instalación global documentada y verificada) · S3 ✅ (el handoff lleva la conversación, recortada por modo; 10 turnos en `raw` ✅). Bitácora: `docs/phases/P07.5-chat.md`.
**En curso por:** —

## Salud del repo
- `cargo xtask check`: ✅ (199 tests; los live se omiten sin `SYMPHONY_LIVE=1`)
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
- [ ] P07.S10 Replanificación (gate: ≥ 2 semanas de uso real; adelantada solo para el chat por ADR-0006)

## Progreso de P07.5
- [x] P07.5.S1 Continuar la sesión después del turno (`continue_session`)
- [x] P07.5.S2 Instalación global mínima
- [x] P07.5.S3 Handoff conversacional
- [ ] P07.5.S4 Agente «chat general» (worktree `symphony/chat`, `Running → Ready`)
- [ ] P07.5.S5 Commit por turno y estado en el bus
- [ ] P07.5.S6 Cambio de modelo/proveedor
- [ ] P07.5.S7 Vista Chat como inicio
- [ ] P07.5.S8 Skills en el chat
- [ ] P07.5.S9 Cierre (gate)

## Próxima acción concreta
1. P07.5.S4: agente «chat general» (worktree y rama `symphony/chat` basada en `main`, uno por proyecto, idempotente) y transición `Running → Ready` con «esperando mensaje».
2. Para probar S1–S3 en tu máquina hay que reinstalar (`symphony daemon stop` y los dos `cargo install --force` de QUICKSTART §1). Las copias viejas de `%APPDATA%
pm` ya se borraron; hoy hay una sola instalación en `~/.cargo/bin` (con S1, sin S3).
3. Leo sigue usando v0.1 y anotando en `docs/research/uso-v0.1.md`; el gate de ≥ 2 semanas aplica a la revisión de P08–P16 tras cerrar P07.5.

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
- La TUI está en `crates/tui`: `app.rs` tiene el estado y la lógica pura, `ui.rs` el render, `io.rs` el IPC y `lib.rs` el loop de terminal. Lee «Decisiones tomadas» en la bitácora P07.
- Para regenerar los snapshots: `INSTA_UPDATE=always cargo nextest run -p symphony-tui`. Revísalos antes de commitear.
- Si un test deja un `symphonyd.exe` huérfano, el build falla con «Acceso denegado» (LEARNINGS P07).

## Bloqueos y preguntas para Leo
- (ninguno)

## Pendientes arrastrados
- P06–P07: repositorios de executor_changes, messages, provider_failures, tool_calls, checkpoints, checkpoint_refs, handoffs y recovery_items (bitácora P03.S3).
- Config de proveedores: `--permission-mode` de Claude (default `acceptEdits`) y sandbox de Codex (default `workspace-write`) deberían salir de `config.toml`.
- Mensajes a Claude después de su turno y el regreso del attach: `resume` (adenda de ADR-0005).
- Los cambios de estado de un agente no pasan por el bus: la TUI sondea cada 3 s (`ponytail:` en `crates/tui/src/app.rs`).

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
| P07.5 | ⏳ aprobada (ADR-0006), sin empezar | p075-done |
| P08–P16 | ⏳ (provisionales; P08–P10 con rediseño pendiente) | |
