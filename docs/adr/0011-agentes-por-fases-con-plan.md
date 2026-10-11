# ADR-0011 · Agentes con sesiones cortas: plan por fases y tareas antes de ejecutar

- **Estado:** ACEPTADO (2026-10-10, con las respuestas de Leo a las preguntas abiertas)
- **Fecha:** 2026-10-10
- **Autor:** claude-code/haiku-5.5 · **Aprobado por:** Leo (2026-10-10)
- **Fase/paso:** P09 (en curso) · afecta a P08 (scheduler y ciclo del agente) y al handoff de P06/P09

## Contexto
Una sesión larga de un agente acumula contexto: cada turno reenvía toda la conversación al modelo. Eso consume más tokens por turno, agota antes los límites diarios y semanales de cada proveedor y degrada la calidad cuando el contexto crece. La evidencia ya la tenemos: el arranque de un CLI de destino cuesta ~18k–27k tokens (ADR-0010), y en la evaluación de handoff (P09.S10) el prompt RAW llega a ~21k tokens solo por el checkpoint.

Hoy un agente recibe la tarea completa y trabaja en una sola sesión larga, sin un plan explícito que divida el trabajo.

Leo pide que el agente, al recibir una tarea (en un agente creado, no en el chat principal):
1. Primero construya un **plan** de la tarea: qué hacer, en qué orden, cuándo y con qué herramientas, siguiendo la documentación y el contexto del proyecto.
2. Que el plan sea **por fases y tareas**.
3. Que cada fase empiece con un contexto limpio (`/clear`), y que, opcionalmente, cada tarea completada dentro de una fase termine con `/compact`.

## Opciones consideradas
1. **Sesión única larga (estado actual).** Simple, pero el coste crece con cada turno y no hay puntos de corte.
2. **Plan + fases con sesión nueva por fase (elegida).** Cada fase arranca con un handoff acotado (objetivo de la fase, estado, archivos tocados, siguiente paso) y no arrastra la conversación anterior.
3. **Plan + `/compact` en cada tarea, sin `/clear`.** Reduce el tamaño, pero el contexto sigue creciendo dentro de la sesión y `/compact` pierde detalle que luego hay que re-explicar.
4. **Subagentes por tarea.** Aísla el contexto, pero multiplica el arranque de CLI por tarea y el coste de arranque domina (ADR-0010). Se descarta como mecanismo por defecto.

## Decisión (propuesta)
1. **Plan obligatorio al inicio.** Al crear un agente con una tarea, el primer paso es una sesión de **planificación**. Produce un documento de plan con:
   - **Fases** (cada una con un objetivo verificable).
   - **Tareas** dentro de cada fase (con criterio de terminado y comando de verificación).
   - **Herramientas y referencias** (skills, comandos de test, docs del proyecto que aplican).
   - **Orden y dependencias.**
   El plan se lee de `docs/` y de `AGENTS.md`/`CLAUDE.md` del proyecto cuando existan; si no existen, el plan lo dice y no inventa convenciones.
2. **El plan se guarda en el worktree del agente** (`.symphony/plan/<task_id>.md`) y se registra como checkpoint, para que sobreviva a un corte.
3. **Ejecución por fase con sesión nueva.** Cada fase arranca en una sesión nueva del proveedor activo (equivale a `/clear`), con un handoff que contiene solo: objetivo de la fase, tareas pendientes, estado del plan y el diff relevante (modo `balanced` de P09.S6). No se arrastra la conversación de la fase anterior.
   - **Automático por default.** Para el usuario debe parecer una sola sesión: Symphony hace el cambio de fase sin preguntar.
   - **Configurable:** el usuario puede elegir que Symphony le pregunte antes de arrancar cada fase nueva.
4. **`/compact` entre tareas dentro de una fase.** Solo si el proveedor lo soporta y si la sesión sigue en la misma fase.
   - **Automático por default.**
   - **Configurable:** se puede desactivar o hacer que Symphony pregunte antes de compactar.
   - Se mide en la bitácora el efecto real en tokens antes de cerrar P09 o P08.
5. **Quién ejecuta `/clear` y `/compact`.** Un agente no puede escribir slash commands sobre su propia sesión. Symphony lo implementa al cerrar el proceso de la fase y arrancar el siguiente (o mandando `/compact` por el canal de `continue_session` si el CLI lo admite). Esto se verifica por proveedor antes de prometerlo.
6. **Puntos de control.** Al terminar cada fase, el agente marca sus tareas en el plan y deja un checkpoint. Si una tarea falla dos veces, el agente se detiene y lo reporta; no sigue con la siguiente fase.

## Consecuencias
- **Positivas:** menos tokens por turno, más distancia entre límites diarios y semanales, planes auditables en el worktree y puntos de reanudación claros.
- **Costos:** cada fase paga el arranque del CLI (~18k–27k tokens según proveedor). Por eso las fases deben ser grandes; una fase por tarea trivial sería peor que una sesión única. Regla inicial: una fase agrupa varias tareas relacionadas.
- **Riesgo:** el plan puede estar mal desde el inicio. Mitigación: el plan se puede reescribir al cerrar una fase, y se registra el cambio.
- **Impacto en P08:** el scheduler trabaja con fases como unidad de ejecución, lo que encaja con el modelo de estados de P08. Esto se debe revisar cuando se haga el diseño de P08 (ADR-0010 §3).
- **Impacto en P09:** el handoff por fase es el caso de uso principal del assembler v2; el modo `balanced` se vuelve el default para este flujo.
- **Pendiente de medir:** coste real de una fase (arranque + handoff) frente a una sesión larga equivalente, con al menos una tarea de tamaño medio, en live con permiso de Leo.

## Respuestas de Leo (2026-10-10)
1. **Arranque de fase:** automático por default, para que parezca una sola sesión. Configurable para que Symphony pregunte al usuario antes de cada fase nueva.
2. **`/compact`:** automático por default. Configurable para desactivarlo o para que Symphony pregunte.
3. **Tareas por fase:** mínimo **2**. Una fase con una sola tarea se junta con la siguiente.

## Configuración propuesta
| Opción | Valores | Default |
|---|---|---|
| Arranque de fase nueva | `auto` / `preguntar` | `auto` |
| `/compact` entre tareas | `auto` / `preguntar` / `desactivado` | `auto` |
| Tareas mínimas por fase | entero | `2` |

Se configura por proyecto (y el valor global como base). La forma exacta en `config.toml` se define en la implementación.

## Seguimiento
- Si se acepta: actualizar PLAN (fases de P08 y P09 que tocan el ciclo del agente) y STATUS. Según CLAUDE.md, los cambios al PLAN van por ADR, así que este documento es el registro del cambio.
