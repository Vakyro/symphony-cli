# Spike: decisiones antes de P08

Fecha: 2026-09-28 · Insumos: `uso-v0.1.md`, `referencias-herdr-ax.md`, PLAN P08–P10 (detalle) y código actual (`crates/daemon`, `crates/core`, `crates/store`).
**Insumo del ADR-0006 (`docs/adr/0006-chat-general-y-replanificacion.md`), que recoge las decisiones y las plasma en PLAN.md (fase P07.5).**

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

1. **Mensajes después del turno (`resume` nativo).** Hoy `send_message` falla si el proceso ya terminó (ADR-0005, adenda). Sin esto no hay chat. **Los adapters ya tienen `resume_spec`** (ver §7); falta cablearlo en el executor. Ojo con el nombre: `pause()`/`resume()` actuales son suspender/reanudar el árbol de procesos, no el `resume` del CLI. Conviene otro nombre para no confundir (p. ej. `continue_session`).
2. **Agente "chat general" con worktree propio (decisión de Leo).** El chat es un agente normal cuyo worktree/rama es el más cercano a `main` (una "pre-main"), creado una vez por proyecto. `create_agent` ya sabe crear task + worktree + branch + checkpoint (`runtime.rs` ~305–520), y `prepare_handoff` ya exige worktree, así que **no hace falta soportar agentes sin worktree**. Lo que falta: crearlo/reutilizarlo de forma idempotente (uno por proyecto), que su task no se marque como terminada tras un turno y una rama con nombre reservado (p. ej. `symphony/chat`) basada en `main`.
3. **Detectar agotamiento de contexto/cuota.** `needs_failover` solo reacciona a `ProviderError`. Ver §7: los CLIs compactan solos, así que el disparador debe ser una política de Symphony sobre el uso de tokens, no un error.
4. **Contexto de la conversación en el handoff.** El handoff v1 se diseñó para tareas con cambios en Git. En un chat sin diff, lo que hay que transferir es la conversación. Confirmado en §7: el handoff v1 **no** incluye mensajes; es trabajo real.
5. **Vista Chat como pantalla inicial** con selector de modelo por mensaje.
6. **Bloqueantes del Día 1:** instalación/invocación global y scroll (ver `uso-v0.1.md`).

## 2. Recomendación de orden (reemplaza el orden actual de P08–P10)

Hoy: P08 scheduler multiagente → P09 Context Engine → P10 salud/failover/routing.
Propuesta: insertar una fase corta **antes** de P08 (nombre provisional "P07.5 · Chat") y reducir P08–P10 a lo que el chat realmente necesite después.

**P07.5 · Chat (orden sugerido, cada paso con su test):**
1. `continue_session`: `resume` por adapter para mensajes posteriores al turno (Claude y Codex). Cierra un pendiente ya conocido.
2. Agente "chat general": se crea o reutiliza una vez por proyecto con su worktree/rama pre-main (p. ej. `symphony/chat`, basada en `main`), sin ciclo de vida de tarea (no termina tras el turno).
3. Vista Chat como home + scroll (rueda, página, posición).
4. Cambio de modelo/proveedor por mensaje reutilizando `switch()` + handoff con historial de conversación.
5. Failover reactivo por cuota/contexto agotado (empezando por lo que se pueda detectar de verdad; parsers parciales de P10.S2).
6. Instalación global (`cargo install` o binario en PATH) verificada desde otro directorio.
7. Commit automático por turno en el worktree del chat (al recibir `TurnFinished`; sin commit si no hay cambios).
8. Skills: probar que las skills nativas de cada CLI funcionan en el chat (los CLIs las cargan del cwd/home; puede no requerir código de Symphony) **(verificar)**.

**Gate de la fase:** un chat real que empieza en Claude, cambia a Codex a mitad y sigue la conversación sin reexplicar (mismo criterio que ADR-0004, pero conversacional).

### 2.1 Jerarquía de ramas (decisión de Leo)

```
main  ←  symphony/chat (pre-main)  ←  ramas de los agentes de tareas
```

- Symphony nunca escribe en `main` directamente; la rama del chat se fusiona a `main` solo con acción explícita del usuario (coherente con la regla de P08.S7: "nunca merge a main sin política explícita").
- Los agentes de tareas nacen de la rama del chat (su `base_ref` es esa rama, no `main`) y se integran de vuelta a ella.
- Lo que ya sirve: `create_agent` recibe `base` y lo guarda en `worktrees.base_ref`; el diff vivo, el checkpoint y el handoff ya usan `base_ref` (`executor.rs` ~1155, `handoff.rs` ~100, `checkpoint.rs` ~284). Cambiar la base no exige tocar esos caminos.
- Cambio de alcance en P08.S7: el destino de la cola de integración pasa a ser la rama del chat; el paso chat → `main` es un merge aparte con revisión.

**Decisiones de Leo sobre los puntos 1 y 4:** commit automático al terminar cada turno del chat, y revisión (review agent) solo en chat → `main`. Los puntos 2 y 3 siguen abiertos.

**Puntos delicados (no bloquean la fase de chat, sí P08):**
1. **Git no permite la misma rama en dos worktrees.** Integrar un agente en `symphony/chat` debe hacerse dentro del worktree del chat. Si el chat tiene cambios sin commitear en ese momento, el merge se complica o falla. **Decidido:** commit automático al terminar cada turno, para que el worktree del chat quede limpio entre turnos (el checkpoint ya captura el estado, así que es coherente). Falta definir el mensaje de commit (p. ej. `chat: turno N` con el modelo usado) y si se omite el commit cuando el turno no cambió archivos.
2. **La base se mueve.** Mientras el chat avanza, los agentes de tareas parten de un punto que queda atrás. Decidir si se fusiona la rama del chat hacia el agente antes de integrar (más limpio, más conflictos tempranos) o solo al final (más simple).
3. **Qué pasa con el chat mientras integra.** El chat es un agente con executor; si un agente de tareas se integra mientras el chat responde, el worktree cambia bajo el CLI. Conviene serializar: integrar solo entre turnos del chat.
4. **Qué revisa el review agent:** cada integración agente → chat, o solo el chat → `main`. **Decidido:** revisión solo en chat → `main`; la integración agente → chat no pasa por review por defecto (queda como política opcional futura).

## 3. Modelo de estado: `phase + conditions` ahora o después

**Recomendación: no migrar todavía.** Las conditions rinden cuando hay varias esperas simultáneas (recurso + dependencia + proveedor), que es P08. El chat es un agente con un executor a la vez.

Lo que sí hace falta ahora, mínimo: un chat entre turnos no está `COMPLETED` (terminal). Necesita un estado "esperando al usuario". Opciones, de menor a mayor costo:
- reutilizar `READY`/`PAUSED` con una razón (`state_reason`) — barato, impreciso;
- agregar un estado (p. ej. `IDLE`) en `enums.rs` + `transitions.rs` + DB — un cambio pequeño y explícito.

**Verificado en `transitions.rs`:** `Completed` es terminal y `Running → Ready` no está permitida. Opción elegida en ADR-0006: permitir `Running → Ready` con `state_reason` «esperando mensaje» (solo cambia la tabla de transiciones; sin migración). Un estado `IDLE` propio o `phase + conditions` se deja para el ADR previo a P08, con datos del scheduler.

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

## 7. Resultados de los mini-spikes (2026-09-28)

Pruebas en un directorio temporal, con prompts mínimos (Claude Code 2.1.284 con `--model haiku`; Codex CLI 0.157.1). Coste total: unos 0,16 USD en Claude y unas 65k tokens de entrada en Codex.

### Spike 2 · Continuar una sesión con un mensaje nuevo: ✅ funciona en ambos
- **Claude:** turno 1 con `--session-id <uuid>`, turno 2 en **otro proceso** con `--resume <uuid>`. Misma sesión y recordó el dato (`7431`).
- **Codex:** `exec --json` devuelve `thread_id`; `exec resume <thread_id> "<mensaje>"` en otro proceso recordó el dato.
- **Detalle que rompe si se ignora:** `codex exec resume` **no acepta `-s`**; el sandbox va con `-c sandbox_mode="…"`. El adapter ya lo hace así (`crates/adapters/codex/src/lib.rs` `resume_spec`).
- **Corrección a §1:** los adapters **ya implementan `resume_spec`** para Claude y Codex. Lo que falta es que el executor del daemon lo use cuando llega un mensaje después del turno (hoy `send_message` devuelve error). Es cableado, no investigación.

### Spike 3 · ¿El handoff transporta una conversación? ❌ no, tal como está
- `HandoffInput` (`crates/context/src/handoff.rs`) solo tiene objetivo, plan, último comando, fallos, archivos, `git status` y diff. **No incluye mensajes.** El historial solo se cuenta en caracteres para estimar el coste `raw` (`daemon/src/handoff.rs` ~97).
- `prepare_handoff` **exige** worktree y checkpoint (`ok_or("el agente no tiene worktree")`). Un chat sin worktree fallaría ahí.
- `worktree_id` es `Option` en el agente, pero **no se aprovechará**: el chat tendrá worktree propio (decisión de Leo), así que este requisito de `prepare_handoff` deja de ser un problema.
- Los mensajes sí están en la base (`repo::conversation_chars` los lee), así que el dato existe.
- **Trabajo real que sale de aquí:** un handoff de conversación (transcript recortado por modo + último estado). Con el chat en su propio worktree, `prepare_handoff` **no** necesita cambios para aceptar agentes sin worktree; los cambios de archivos del chat sí viajan por el diff y el `git status` que ya usa.

### Spike 1 · Señal de contexto agotado: ⚠️ el supuesto era incorrecto
- **Cuota:** ya hay parsers para "usage limit", rate limit, auth, red y modelo no disponible en ambos adapters, con fixtures reales. Esa señal existe y funciona para el failover.
- **Contexto:** ninguno de los dos parsers tiene un patrón de "contexto lleno". Y probablemente no lo necesiten: `claude --help` muestra `--autocompact <auto|tokens>` y los dos CLIs documentan hooks `PreCompact`/`PostCompact`. Es decir, **el CLI compacta por su cuenta en vez de fallar**. (No se llenó un contexto real; es una inferencia de la ayuda y la documentación, no una prueba.)
- **Consecuencia de diseño:** "salto por contexto agotado" no es un error que esperar, sino una **política de Symphony**: leer el uso de tokens del stream (`turn.completed.usage` en Codex; el `result` en Claude) y decidir cambiar de proveedor antes de que el CLI compacte y pierda detalle, o cuando el usuario lo pida. Opcional: usar `PreCompact` como aviso.
- **Sigue pendiente (requiere gastar cuota):** provocar un límite de cuota real y comprobar que el parser lo detecta con el texto actual. Los fixtures ya cubren el formato conocido.

### Spike 4 · Coste de cambiar de proveedor: medido a pequeña escala
- **Codex:** una sesión nueva con un prompt trivial ya usa **~19,7k tokens de entrada** (11,4k en caché): es el sobrecoste fijo del CLI. Al continuar con `resume` la entrada sube a ~45,9k (30,9k en caché): el CLI vuelve a leer el historial.
- **Claude (haiku):** turno 1 ≈ 0,031 USD; turno 2 con `--resume` ≈ 0,048 USD (+55 %). Parte de ese coste base viene del entorno de Leo (muchas herramientas MCP y hooks cargados).
- **Lectura:** cambiar de proveedor implica pagar el sobrecoste fijo de una sesión nueva (~20k tokens en Codex) más el transcript inyectado; `resume` en el mismo proveedor cuesta mucho menos gracias a la caché. Por eso conviene cambiar de proveedor solo por cuota, por elección del usuario o por umbral, y preferir `resume` cuando el CLI simplemente terminó. Son cifras de conversaciones de 1–2 turnos; hay que repetirlas con una conversación larga antes de fijar umbrales.

### Efecto en las recomendaciones
1. **P07.5 paso 1** baja de riesgo: solo hay que cablear `resume_spec` en el executor.
2. **Nuevo paso** en P07.5: handoff de conversación (transcript) antes de cambiar de proveedor en el chat.
3. **P07.5 paso 5** cambia de nombre: "failover reactivo por cuota" (funciona) + "política de cambio por uso de tokens" (nueva), no "detección de contexto lleno".
4. El riesgo principal pasa de "¿hay señal?" a "¿qué información se pierde en el handoff de conversación y cuánto cuesta?".

## 8. Preguntas para Leo

1. ~~¿El chat es un agente sin worktree o una entidad nueva?~~ **Resuelta por Leo:** agente "chat general" con worktree propio, la rama más cercana a `main` (pre-main). Queda una consecuencia por decidir, ver la pregunta 5.
2. ¿Aceptas que el failover automático de contexto/cuota salga **después** del cambio manual de modelo si el mini-spike 1 no encuentra una señal fiable?
3. ¿Se difiere P08 (multiagente) hasta terminar el chat, o se mantiene en paralelo como opción avanzada?
4. ¿Quieres que la vista inicial sea el chat y los agentes/tareas queden como segunda pantalla?
5. ~~¿Jerarquía `main ← chat ← agentes`?~~ **Resuelta por Leo:** sí. Las ramas de los agentes salen de la rama del chat, se integran de vuelta a ella, y la rama del chat es la que se fusiona a `main`. Ver §2.1.

## 9. Qué cambiaría en el PLAN si se aprueba

- Nueva fase P07.5 (o renumerar) con los 7 pasos de §2.
- P08–P10 marcadas como "rediseño pendiente tras P07.5".
- `CONSTRAINTS.md`: regla de camino barato por evento.
- ADR único "Referencias Herdr/AX + chat" con esta tabla como base.
