# ADR-0006 · Chat general con failover entre proveedores y replanificación parcial

- **Estado:** PROPUESTO (decisiones de producto tomadas por Leo en conversación, 2026-09-28; quedan 2 puntos por confirmar, ver «Pendiente de Leo»)
- **Fecha:** 2026-09-28
- **Autor:** claude-code/sonnet-5.5 · **Aprobado por:** — (pendiente)
- **Fase/paso:** P07.S10 (replanificación adelantada, solo para el chat)

## Contexto
Tras usar v0.1 (`docs/research/uso-v0.1.md`), Leo observó que la experiencia se centra en agentes autónomos/multiagente, y que lo que quiere es un **chat agéntico simple** (como Claude Code o Codex, con sus skills) en la vista inicial, cuyo atractivo sea el **cambio de proveedor/modelo sin perder contexto**: por cuota agotada, por elección entre mensajes, o por decisión del usuario.

El gate de P07.S10 pedía ≥ 2 semanas de uso antes de replanificar. Este ADR **adelanta solo la decisión sobre el chat**, porque cambia el orden de todo lo que sigue y es barato de validar (ver Evidencia). **P08–P16 siguen siendo provisionales** y se revisarán con el gate original.

Se estudiaron además Herdr y AX como referencia (no como dependencia): `docs/research/referencias-herdr-ax.md`. El análisis completo y los spikes están en `docs/research/spike-p08-decisiones.md`.

## Opciones consideradas
1. **Mantener el orden (P08 multiagente → P09 contexto → P10 failover).** El chat quedaría al final y P09/P10 construirían infraestructura para un caso (varios agentes, router con puntuación) que Leo no está usando.
2. **Chat como entidad nueva**, separada de los agentes. Duplica runtime, checkpoints, handoff y vistas.
3. **Chat como un agente normal con worktree propio (elegida).** Reutiliza executor, `switch()`, failover, checkpoint y handoff ya existentes.

## Decisión
1. **Se inserta la fase P07.5 · Chat general antes de P08** (detalle en PLAN).
2. **El chat es un agente** llamado «chat general», uno por proyecto, con su propio worktree y una rama reservada (`symphony/chat`) basada en `main`. Es la rama más cercana a `main` («pre-main»). *(Decisión de Leo.)*
3. **Jerarquía de ramas: `main ← symphony/chat ← ramas de agentes de tareas`.** Los agentes de tareas nacen de la rama del chat, se integran de vuelta a ella y solo la rama del chat se fusiona a `main`, con acción explícita del usuario. Symphony nunca escribe en `main` directamente. *(Decisión de Leo.)*
4. **Commit automático al terminar cada turno del chat** (sin commit si no hay cambios), para que el worktree quede limpio entre turnos y la integración de agentes sea segura. *(Decisión de Leo.)*
5. **Review agent solo en chat → `main`.** La integración agente → chat no pasa por revisión por defecto. *(Decisión de Leo.)*
6. **La vista inicial es el chat**; agentes y tareas quedan como segunda pantalla. *(Petición de Leo.)*
7. **Continuar un chat después del turno** usa el `resume` nativo de cada CLI (`resume_spec`, ya implementado en los adapters). Cambiar de proveedor usa `switch()` con un **handoff que incluye la conversación** (hoy no la incluye).
8. **Estado del chat entre turnos:** se permite la transición `Running → Ready` (con `state_reason` «esperando mensaje») en lugar de crear un estado `IDLE`. No toca el esquema de la base ni la migración. `Completed` es terminal y no sirve para un chat. Un estado propio o `phase + conditions` se decide en el ADR previo a P08, con datos del scheduler.
9. **Disparadores de cambio de proveedor:** (a) **cuota/límite** (los parsers y el failover ya existen); (b) **elección manual** por mensaje; (c) **política por umbral de uso de tokens**, configurable y desactivada por defecto. **No** se promete detectar «contexto lleno» como error: los CLIs compactan por su cuenta.
10. **El cambio de estado de un agente se emite al bus** desde el punto único de escritura (`set_state`), para que «terminó el turno» no dependa del sondeo de 3 s de la TUI.
11. **P08–P16 quedan diferidas** hasta cerrar P07.5 y cumplir el gate de uso. Se reevaluará el recorte de P09 (AST, watcher, consolidación de hechos, broker MCP) y de P10 (router con puntuación, reserva de cuota), que no hacen falta para el chat.

## Evidencia
Ver `docs/research/spike-p08-decisiones.md` §7 (pruebas del 2026-09-28):
- **`resume` en otro proceso conserva el contexto** en Claude Code (`--resume`) y en Codex (`exec resume`, con `-c sandbox_mode=…`, no `-s`). Los adapters ya tienen `resume_spec`; falta cablearlo en el executor.
- **El handoff v1 no incluye mensajes** (solo objetivo, plan, comando, fallos, archivos, `git status` y diff). Los mensajes sí están en la base. Es trabajo nuevo, no un ajuste.
- **Parsers de error existentes** cubren cuota, rate limit, auth, red y modelo no disponible. No hay patrón de contexto lleno, y `claude --help` (`--autocompact`) y los hooks `PreCompact`/`PostCompact` indican compactación automática (inferencia, no probado con un contexto real).
- **Coste de cambiar de proveedor:** una sesión nueva en Codex gasta ~20k tokens de entrada solo por abrirse; `resume` en Claude subió de ≈0,031 a ≈0,048 USD en el segundo turno. Son cifras de 1–2 turnos; hay que repetirlas con una conversación larga.
- **Código:** el chat reutiliza `failover()` (`executor.rs` ~433), `switch()` (~747), `send_message()` (~930) y `worktrees.base_ref`, que el diff, el checkpoint y el handoff ya usan, así que cambiar la base a la rama del chat no obliga a tocar esos caminos.
- **Transiciones:** `Running → Ready` no está permitida hoy y `Completed` es terminal (`crates/core/src/transitions.rs`).

## Consecuencias
- **PLAN:** nueva fase P07.5 (9 pasos) y nota de rediseño pendiente en P08–P10. Tag de la fase: `p075-done`.
- **P08.S7:** el destino de la cola de integración pasa a ser la rama del chat; chat → `main` es un merge aparte con review agent.
- **`core`:** cambio en la tabla de transiciones (`Running → Ready`) con su proptest; sin migración.
- **`context`/`daemon`:** `HandoffInput` gana la conversación (recortada por modo `raw/safe/balanced/aggressive`).
- **`daemon`:** executor usa `resume_spec` para mensajes tras el turno; evento de estado al bus; commit por turno.
- **`tui`:** vista Chat como home, scroll (rueda, página, posición), selector de modelo por mensaje.
- **Distribución:** instalación global mínima e invocación desde cualquier carpeta se adelanta desde P13 (bloqueaba el uso básico en el Día 1).
- **CONSTRAINTS:** añadir la regla «camino barato por evento × agente» (diff, DB y contexto se procesan en lote, no por evento).
- **Sigue abierto para P08 (§2.1 del spike):** cuándo se fusiona la rama del chat hacia un agente, y serializar la integración entre turnos del chat.
- **Riesgos:** el coste en tokens de cada cambio de proveedor y qué información se pierde en el handoff conversacional; se miden en el gate de P07.5.

## Pendiente de Leo
1. **Confirmar que P08 se difiere hasta cerrar P07.5** (y que P08–P16 se reevalúan con el gate de uso original).
2. **Confirmar que el failover por umbral de tokens** entra como política opcional (apagada por defecto) y que el disparo por «contexto lleno» no se promete.

Mientras no estén confirmados, el estado del ADR es PROPUESTO y P07.5 no debe empezar más allá de S1–S2 (que no dependen de estas dos decisiones).
