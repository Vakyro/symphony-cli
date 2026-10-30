# ADR-0022 · Streaming del chat y ediciones en la TUI

- **Estado:** ACEPTADO
- **Fecha:** 2026-10-28
- **Autor:** agente (opencode/deepseek-v4.1-flash-free) · **Aprobado por:** Leo, por petición
  explícita en el backlog (2026-10-27: «la respuesta aparece poco a poco, no al terminar» y
  «mostrar pensamientos, ediciones de código con líneas agregadas/eliminadas»). El detalle técnico
  lo decide esta ADR.
- **Fase/paso:** P17.S2

## Contexto

P17.S2 pide el chat «estilo Claude Code»: la respuesta se escribe poco a poco, se ven los
pensamientos, las ediciones con `+N −M` y los archivos tocados.

Lo que **ya existe** (comprobado antes de tocar nada, que es lo que evita inventar un mecanismo
nuevo):

- Los adaptadores emiten `AssistantDelta { text }` por cada trozo y `AssistantText { text }` al
  terminar. El bus guarda los deltas **solo en memoria** (`Bus::partial`, tope de 64 KiB) y el
  mensaje completo en la base (P11.5.S3).
- `agent.logs` devuelve `{ messages, partial }`, y la TUI ya pinta `partial` con un cursor `▍`
  (`AgentView.partial`, `ui::partial_lines`). Cada evento del bus marca la vista como sucia y el
  tick de 1 s (o el siguiente mensaje) vuelve a pedir el estado.
- Conclusión: **el streaming existe**; lo que no hay es más resolución que el tick de 1 s, y eso
  se nota poco al leer.

Lo que **no existe**:

- **Pensamientos.** Los adaptadores los descartan a propósito (`claude`: `thinking_delta` se
  ignora; `kimi`: `thinking_alone_is_not_assistant_text`). No hay variante canónica en
  `AgentEvent`, y añadirla toca `events.type` (CHECK de la tabla) → migración.
- **`+N −M` por edición.** `AgentEvent::FileModified` dice qué archivo cambió, pero no cuántas
  líneas. El diff completo sí está en `agent.diff`; nadie lo cuenta por archivo en la TUI.

## Opciones consideradas (streaming)

1. **Dejarlo como está:** `partial` + tick de 1 s. Una petición IPC por segundo como mucho,
   ningún cambio en el daemon.
2. **Publicar el `AgentEvent` serializado dentro de `agent.event`** y pintar cada delta al llegar
   (sin IPC por trozo). Más fluido (decenas de ms), pero añade un segundo camino para el mismo
   texto y un payload que hoy nadie mira.
3. **Refrescar en cada evento sin esperar al tick.** Multiplica las peticiones IPC por turno
   (`agent.logs` + `agent.inspect` + `agent.activity` + `agent.diff`) y satura el writer.
4. **Canal aparte fuera de IPC** (socket propio o fichero compartido). Rompe la regla de que la
   TUI habla solo IPC y añade superficie local.

## Decisión

- **Streaming: opción 1.** No se toca el daemon. El tick de 1 s es suficiente para leer y mantiene
  un solo camino (estado pedido por IPC) y un solo sitio donde vive el texto en curso. Si más
  adelante Leo quiere sub-segundo, la opción 2 queda documentada aquí: es una línea en
  `stream_events` (`"data": ev.event`) y cambiar `Msg::Event(String)` por `Msg::Event { topic,
  payload }`; no hay que rediseñar nada.
- **Pensamientos: no se muestran en P17.S2.** Mostrarlos exige `AgentEvent::Thinking` y una
  migración del CHECK de `events.type`; se anota como trabajo aparte (backlog de P18/P19, donde ya
  se toca el contexto) en vez de colarlo en un paso de interfaz.
- **Ediciones: sí, pero calculadas en la TUI.** Con `agent.diff` (que ya se pide) se cuentan las
  líneas `+`/`-` por archivo y se muestran junto a los archivos tocados que llegan en la actividad
  (`FileModified`). Sin IPC nuevo y sin eventos nuevos.

## Consecuencias

- P17.S2 queda dentro de `crates/tui`: streaming verificado con un test (un `agent.logs` con
  `partial` se pinta con el cursor y desaparece cuando llega el mensaje completo), y resumen de
  ediciones por archivo a partir del diff.
- Los snapshots existentes no cambian por el streaming (mismo texto, mismo sitio).
- El chat sigue mostrando el texto en curso aunque la TUI se reconecte: `partial` vive en el
  daemon, no en la TUI.
- Deuda anotada: pensamientos (evento nuevo + migración) y streaming sub-segundo (opción 2).
