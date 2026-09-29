# Spike: decisiones antes de P08

Fecha: 2026-09-28 · Insumos: `uso-v0.1.md`, `referencias-herdr-ax.md`, PLAN P08–P10 (detalle) y código actual (`crates/daemon`, `crates/core`, `crates/store`).
**Es una recomendación para el ADR de P07.S10, no una decisión aprobada. No modifica PLAN.md.**

Límite de lo leído: del PLAN leí el detalle de P08–P10; de P11–P16 solo los títulos de pasos. Del código leí `executor.rs`, `runtime.rs`, `enums.rs` y tamaños de archivos, no todo el repo. Lo marcado **(verificar)** no se comprobó.

## 1. Hallazgo principal: el chat ya está ~70 % construido

Lo que Leo pide (chat agéntico, cambiar de modelo/proveedor entre mensajes, failover sin perder contexto) reutiliza piezas que existen desde P06:

| Pieza del chat | Ya existe | Dónde |
|---|---|---|
| Failover automático ante error de proveedor | Sí (básico) | `executor.rs` `failover()` ~433, `needs_failover()` ~75 |
| Cambio manual de modelo/proveedor en el mismo agente | Sí | `executor.rs` `switch()` ~747 |
| Handoff determinista desde checkpoint + Git vivo | Sí (v1) | `crates/context/src/handoff.rs`, `daemon/src/handoff.rs` |
| Registro del cambio de executor | Sí | `ExecutorChangeId`, mensaje "Executor changed…" |
| Mensajes durante el turno (stdin) | Sí | `send_message()` ~930 |

Lo que **falta** para que exista el chat:

1. **Mensajes después del turno (`resume` nativo).** Hoy `send_message` falla si el proceso ya terminó (ADR-0005, adenda). Sin esto no hay chat. Ojo con el nombre: `pause()`/`resume()` actuales son suspender/reanudar el árbol de procesos, no el `resume` del CLI. Conviene otro nombre para no confundir (p. ej. `continue_session`).
2. **Agente sin task/worktree.** `create_agent` siempre crea task + worktree + branch + checkpoint (`runtime.rs` ~305–520). Un chat corto en la raíz del proyecto no debería crear rama. Posible punto a favor: en `runtime.rs` ~470 el agente recibe `worktree_id: Some(...)`, lo que sugiere que el campo ya es opcional en el esquema **(verificar)**.
3. **Detectar agotamiento de contexto/cuota.** `needs_failover` solo reacciona a `ProviderError`. No sabemos si Claude/Codex emiten un error identificable al llenar el contexto **(verificar empíricamente; es el mayor riesgo del producto)**.
4. **Contexto de la conversación en el handoff.** El handoff v1 se diseñó para tareas con cambios en Git. En un chat sin diff, lo que hay que transferir es la conversación. ¿Incluye el `HandoffAssembler` el historial de mensajes? **(verificar)**. Si no, es trabajo real, no un detalle.
5. **Vista Chat como pantalla inicial** con selector de modelo por mensaje.
6. **Bloqueantes del Día 1:** instalación/invocación global y scroll (ver `uso-v0.1.md`).

## 2. Recomendación de orden (reemplaza el orden actual de P08–P10)

Hoy: P08 scheduler multiagente → P09 Context Engine → P10 salud/failover/routing.
Propuesta: insertar una fase corta **antes** de P08 (nombre provisional "P07.5 · Chat") y reducir P08–P10 a lo que el chat realmente necesite después.

**P07.5 · Chat (orden sugerido, cada paso con su test):**
1. `continue_session`: `resume` por adapter para mensajes posteriores al turno (Claude y Codex). Cierra un pendiente ya conocido.
2. Agente en modo conversación (sin worktree/branch/task visible; cwd = raíz del proyecto).
3. Vista Chat como home + scroll (rueda, página, posición).
4. Cambio de modelo/proveedor por mensaje reutilizando `switch()` + handoff con historial de conversación.
5. Failover reactivo por cuota/contexto agotado (empezando por lo que se pueda detectar de verdad; parsers parciales de P10.S2).
6. Instalación global (`cargo install` o binario en PATH) verificada desde otro directorio.
7. Skills: probar que las skills nativas de cada CLI funcionan en el chat (los CLIs las cargan del cwd/home; puede no requerir código de Symphony) **(verificar)**.

**Gate de la fase:** un chat real que empieza en Claude, cambia a Codex a mitad y sigue la conversación sin reexplicar (mismo criterio que ADR-0004, pero conversacional).

## 3. Modelo de estado: `phase + conditions` ahora o después

**Recomendación: no migrar todavía.** Las conditions rinden cuando hay varias esperas simultáneas (recurso + dependencia + proveedor), que es P08. El chat es un agente con un executor a la vez.

Lo que sí hace falta ahora, mínimo: un chat entre turnos no está `COMPLETED` (terminal). Necesita un estado "esperando al usuario". Opciones, de menor a mayor costo:
- reutilizar `READY`/`PAUSED` con una razón (`state_reason`) — barato, impreciso;
- agregar un estado (p. ej. `IDLE`) en `enums.rs` + `transitions.rs` + DB — un cambio pequeño y explícito.

**(verificar en `transitions.rs`)** qué transiciones permite `RUNNING → …` al terminar el turno, antes de elegir. Mi voto: agregar un estado `IDLE`, y dejar `phase + conditions` como decisión del ADR previo a P08 con datos del scheduler.

## 4. Event bus

`STATUS.md` ya reconoce que los cambios de estado no pasan por el bus (la TUI sondea cada 3 s). Para el chat importa: "el turno terminó" debe verse al instante, no en 3 s.
**Recomendación:** emitir un evento `AgentStateChanged` desde el único punto de escritura de estado (`set_state` en `executor.rs` / `repo::set_agent_state`). Es un cambio pequeño y de bajo riesgo; el bus completo dirigido por dominio (scheduler, validación) queda para P08.

## 5. Herdr y AX: qué adoptar y cuándo

| Idea | Cuándo | Motivo |
|---|---|---|
| Restauración por niveles (resume nativo antes de handoff) | **Ahora (P07.5 paso 1)** | Es exactamente lo que el chat necesita |
| Server owns runtime / clientes solo presentan | Ya cumplido | `symphonyd` + TUI cliente |
| Regla "camino barato por evento × agente" | Ahora, en `CONSTRAINTS.md` | Una línea, evita deuda |
| Evento de cambio de estado en el bus | Ahora | Ver §4 |
| Suspend/idle-suspend de executors | P08 | Ahorra RAM con muchos agentes; el chat ya termina un proceso por turno, así que hay poco que ahorrar hoy |
| `Workspace` como entidad | P08 | Valor real solo con varios agentes que comparten entorno |
| `phase + conditions`, `TaskSpec/Revision` | ADR previo a P08 | Ver §3 |
| Health vs readiness | P10 | Depende del router |
| Manifests de detección por pantalla | P11 y solo si un CLI no tiene hooks | Mantenimiento continuo |
| Fork de agente, modo declarativo, plugins tipo Herdr | Después de v1.0 / P12 | Fuera del foco |
| K8s, Redis, gRPC, contenedores, telemetría externa | Descartado | Contra "ligero y local" |

## 6. Qué recortaría de P08–P16 (posible sobre-ingeniería)

Basado solo en los títulos/descripciones leídos; validar con el ADR:

- **P09 Context Engine completo** (AST L0–L5 con tree-sitter, file watcher, consolidación de hechos, broker MCP): no se necesita para el chat. Diferir hasta medir cuántos tokens cuesta el handoff v1 en uso real. Mantener solo lo que el chat exige (historial en el handoff).
- **P10 router determinista y cuota** (`provider_health`, `routing_decisions`, reserva): el chat necesita cambio manual + failover reactivo, no un router con puntuación. Reducir a P10.S2 parcial (parsers de errores) y diferir el resto.
- **P08 completo** (DAG, milestones, validation engine, merge/review agent, benchmark de 3 agentes): sigue siendo valioso pero pasa a producto secundario. Rehacer su alcance tras el chat.
- **P14 ML opcional, P15 GUI, P12 plugins**: sin cambios de orden; después.

## 7. Riesgos a validar antes de comprometer el diseño (mini-spikes, ~1 h cada uno)

1. **¿Qué emiten Claude y Codex cuando se llena el contexto o se acaba la cuota?** Sin una señal fiable no hay failover automático; solo manual. Probar con fixtures reales (con permiso de Leo por la cuota).
2. **¿`claude --resume <id>` y `codex resume <id>` aceptan un mensaje nuevo en modo headless y conservan el contexto?** ADR-0005 lo marca pendiente.
3. **¿El handoff v1 puede transportar una conversación sin cambios en Git?** Leer `handoff.rs` y probar con un chat de 10 turnos y cambio de proveedor.
4. **Costo en tokens de reanudar/re-inyectar** en cada cambio de proveedor (afecta la promesa de "sin perder contexto" vs cuota gastada).

## 8. Preguntas para Leo

1. ¿El chat es un agente sin worktree (mi recomendación, reutiliza todo) o una entidad nueva?
2. ¿Aceptas que el failover automático de contexto/cuota salga **después** del cambio manual de modelo si el mini-spike 1 no encuentra una señal fiable?
3. ¿Se difiere P08 (multiagente) hasta terminar el chat, o se mantiene en paralelo como opción avanzada?
4. ¿Quieres que la vista inicial sea el chat y los agentes/tareas queden como segunda pantalla?

## 9. Qué cambiaría en el PLAN si se aprueba

- Nueva fase P07.5 (o renumerar) con los 7 pasos de §2.
- P08–P10 marcadas como "rediseño pendiente tras P07.5".
- `CONSTRAINTS.md`: regla de camino barato por evento.
- ADR único "Referencias Herdr/AX + chat" con esta tabla como base.
