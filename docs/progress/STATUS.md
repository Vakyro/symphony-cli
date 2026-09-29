# STATUS — Symphony CLI

**Actualizado:** 2026-09-29 · por claude-code/sonnet-5.5
**Fase actual:** P07.5 · Chat general (rama `phase/p075-chat`, ADR-0006 aceptado)
**Paso actual:** P07.5.S8 · Skills en el chat (S1–S7 hechos). P08–P16 siguen provisionales
**Estado del paso:** S1 ✅ (un mensaje tras el turno retoma la sesión; live Claude y Codex ✅) · S2 ✅ (instalación global documentada y verificada) · S3 ✅ (el handoff lleva la conversación, recortada por modo; 10 turnos en `raw` ✅) · S4 ✅ (chat idempotente en `symphony/chat`, turno termina en `READY`; L2 ✅, sin live) · S5 ✅ (commit por turno del chat y `AgentStateChanged` en el bus; L2 ✅, sin live) · S6 ✅ (cambio manual con mensaje, failover del chat y umbral de tokens opcional; L1/L2 ✅, sin live) · S7 ✅ (vista Chat como inicio, `chat.get`, FLOW + Journey F; snapshots ✅; probado en terminal con live mínimo Claude→Codex ✅, 4 bugs corregidos; opinión de Leo documentada en `docs/research/uso-v0.1.md` § Día 2, pendiente de priorizar). Bitácora: `docs/phases/P07.5-chat.md`.
**En curso por:** —

## Salud del repo
- `cargo xtask check`: ✅ (224 tests; los live se omiten sin `SYMPHONY_LIVE=1`)
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
- [ ] P07.S10 Replanificación (gate por criterio de contenido, ADR-0007: cumplido para el chat; la revisión de P08–P16 va tras cerrar P07.5)

## Progreso de P07.5
- [x] P07.5.S1 Continuar la sesión después del turno (`continue_session`)
- [x] P07.5.S2 Instalación global mínima
- [x] P07.5.S3 Handoff conversacional
- [x] P07.5.S4 Agente «chat general» (worktree `symphony/chat`, `Running → Ready`)
- [x] P07.5.S5 Commit por turno y estado en el bus
- [x] P07.5.S6 Cambio de modelo/proveedor
- [x] P07.5.S7 Vista Chat como inicio
- [ ] P07.5.S8 Skills en el chat
- [ ] P07.5.S9 Cierre (gate)

## Próxima acción concreta
1. P07.5.S8: skills en el chat (ver PLAN). Antes, Leo prueba S7 (ya reinstalado en `~/.cargo/bin` con los dos binarios) y sus opiniones ya están documentadas (Día 2 de `uso-v0.1.md`: proceso en vivo, copiar, scroll, textos cortados, exportar `.md`, UX). Decidir con Leo qué entra antes de S8.
2. Para probar S1–S3 en tu máquina hay que reinstalar (`symphony daemon stop` y los dos `cargo install --force` de QUICKSTART §1). Las copias viejas de `%APPDATA%
pm` ya se borraron; hoy hay una sola instalación en `~/.cargo/bin` (con S1, sin S3).
3. ADR-0007 (aceptado): el gate de ≥ 2 semanas pasa a criterio de contenido. Antes de S8 se atienden las observaciones del Día 2 (`uso-v0.1.md`): 1) textos cortados (bug), 2) proceso del agente en vivo + medir rendimiento, 3) copiar/scroll de respuesta y exportar a `.md`. Leo sigue anotando en `uso-v0.1.md`.

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
- **Dónde retomar (2026-09-29):** rama `phase/p075-chat` (S1–S7 hechos, sin fusionar hasta cerrar la fase). `main` = ADR-0006 aceptado + PLAN con P07.5. Empieza por **P07.5.S8**; al terminar, agrégalo a la bitácora `docs/phases/P07.5-chat.md`. Corre `cargo xtask check` antes de tocar código.
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
- Mensajes a Claude después de su turno y el regreso del attach: `resume` (adenda de ADR-0005).

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
