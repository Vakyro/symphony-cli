# ADR-0013 · P08: cómo entran el plan por fases (ADR-0011) y la delegación desde el chat

- **Estado:** ACEPTADO (Leo, 2026-10-11)
- **Fecha:** 2026-10-11
- **Autor:** claude-code/sonnet-5.5 · **Aprobado por:** Leo
- **Fase/paso:** P08 (runtime multiagente) · afecta P08.S1, S3, S5, S6, S7, S8 y S9

## Contexto
P08 se diseñó (PLAN) para «3 agentes en paralelo sin que la máquina se trabe»: scheduler, DAG, validación y merge. Desde entonces hay tres entradas nuevas:

1. **ADR-0011 (aceptado):** cada agente creado planifica su tarea por fases y tareas, abre una sesión nueva por fase (equivale a `/clear`) y compacta entre tareas; automático por defecto y configurable; mínimo 2 tareas por fase.
2. **Uso real en Signalia** ([`uso-signalia.md`](../research/uso-signalia.md)): 30 agentes, 5 simultáneos; la oleada 2 agotó la cuota en 22 min; el orquestador (Claude Code) hacía a mano el plan de oleadas, el gate, el commit y el merge de cada agente.
3. **Pedido de Leo (2026-10-10):** el chat principal con dos modos, *directo* y *delegado* («haz X con N agentes y estos modelos»).

Ya hecho y verificado, sin necesidad de P08: F5 (causa real de la muerte de un run), F6 (`resume` de un `FAILED`), el broker MCP (`context_*`), skills y `mcp.toml` compartidos y OpenCode.

## Lo que P08 tiene que decidir antes de escribir código
Cada punto trae una recomendación; Leo decide.

### D1 · ¿Dónde vive el plan de un agente?
| Opción | Qué es | A favor | En contra |
|---|---|---|---|
| **A. Archivo en el worktree + checkpoint (recomendada)** | `.symphony/plan/<task_id>.md` con fases y tareas, marcadas al avanzar; `checkpoints.current_step/next_step` ya existen | Sin migración, versionable, el agente lo lee y lo edita, sobrevive a cualquier proveedor (STACK §16: «el filesystem es la fuente de verdad») | La TUI tiene que parsear el Markdown |
| B. Tablas (`plan_phases`, `plan_tasks`) | Plan en SQLite | Vistas y consultas directas | Migración más grande y dos fuentes de verdad |

**Recomendación:** A en P08, con un índice mínimo en DB solo si la vista lo pide (se decide en P08.S8).

### D2 · Cómo se implementa «sesión nueva por fase»
- Una fase termina cuando todas sus tareas están marcadas en el plan **y** hay un checkpoint. El daemon cierra el run y abre otro con el mismo proveedor y modelo y un handoff acotado (objetivo de la fase, tareas pendientes, plan, diff relevante, `balanced`).
- El run saliente se cierra con un motivo nuevo (`PHASE_DONE`) y estado `HANDED_OFF`: **cambia `RunEndReason`**, que es un CHECK de la tabla `agent_runs` → va en la migración 004 (con reconstrucción de tabla, como la 002 y la 003) y en `core::enums`. Es un cambio del core: por eso ADR.
- `/compact` entre tareas: solo si el proveedor lo admite por `continue_session`; se mide antes de dejarlo como default (como dice ADR-0011).
- **La migración de Fase 2 es la 004**, no la 002 que dice el PLAN.

### D3 · «phase + conditions» frente a los 12 estados
ADR-0006/0010 dejó esta decisión para P08.S1 «con datos del scheduler». Lo que hay: los 12 estados (`CREATED … CANCELLED`) cubrieron 5 agentes simultáneos en Signalia sin ambigüedad; la «fase» del plan (D1) **no** es un estado del agente, es el progreso dentro de su tarea.
**Recomendación:** mantener los 12 estados exclusivos y dejar la fase como dato del plan. Reabrir solo si el scheduler muestra un caso que no alcanza.

### D4 · Qué entra en P08 de lo aprendido en Signalia
| # | Feature (de `uso-signalia.md` § 6) | Dónde encaja | Recomendación |
|---|---|---|---|
| F1 | Modo delegado del chat (crear, vigilar, integrar, reportar) | P08.S5–S7 + chat | **Sí**, es el pedido de Leo; después del scheduler |
| F2 | `agent.*` por MCP para que otra sesión orqueste | P09.S8 (broker) | **Sí**, barato: ya hay broker y IPC |
| F3 | Plan de lote declarativo (tareas, modelo, rutas propias, dependencias, relevos, máximo simultáneo) | P08.S5 (DAG) | **Sí**; el formato del plan de ADR-0011 y este deben ser uno |
| F4 | Concurrencia según cuota restante | P08.S3 (scheduler) + P10 | **Sí**; sin esto se repite la oleada 2 |
| F7 | Gate y commit por agente configurables, merge solo si pasa | P08.S6–S7 | **Sí** |
| F8 | Comando de preparación del worktree (`pnpm install`) | P08.S6 | **Sí**, es pequeño |
| F9 | Capacidades por proveedor que el router entienda (puede ejecutar comandos, tiene sandbox) | P10 router | **Sí**, para no mandar tareas de shell a Antigravity |
| F10 | Lista de modelos permitidos por proyecto, aplicada | P10 profiles | **Sí** (hoy «Sin Copilot» es solo un documento) |
| F11 | Vista de «pendiente de integrar» | P08.S8 | **Sí** |
| F12 | Contabilidad de tokens utilizable | P10 | Aparte: sin esto F4 es ciega; medir antes de P08.S3 |
| F13 | El chat sabe lo que delegó (resumen desde Symphony, no desde un doc) | P08.S8 | **Sí**, con F1 |

### D5 · Qué NO hace P08 (para acotarla)
- Una sola instancia MCP compartida (`share_mode = shared`): exige que el daemon haga de proxy MCP.
- Indexar por worktree de cada agente (hoy el índice cubre la raíz del proyecto).
- Merge a `main` automático: sigue siendo «nunca sin política explícita» (PLAN P08.S7); la integración va a la rama del chat (ADR-0006/0010).

## Orden propuesto de P08
1. **S1** diseño (D1–D3 con Leo) + migración 004 (tablas de Fase 2 + `PHASE_DONE`).
2. **S2** clasificador de operaciones y Resource Manager.
3. **S3** scheduler **con concurrencia por cuota (F4)**.
4. **S4** enforcement a nivel OS.
5. **S5** DAG + plan de lote (F3) + plan por fases de cada agente (ADR-0011).
6. **S6** validación con gate configurable y preparación del worktree (F7, F8).
7. **S7** merge, conflictos, review agent y modo delegado del chat (F1).
8. **S8** vistas de Fase 2 + «pendiente de integrar» (F11, F13).
9. **S9** benchmark principal (gate, opinión de Leo) con el live que Leo autorice.
10. **S10** cierre.

## Decisión de Leo (2026-10-11)
1. **D1: A (archivo de plan)**, con estas reglas: `.symphony/plan/<task_id>.md` es **temporal**; mientras la tarea corre documenta lo hecho y lo que falló; **al terminar la tarea se borran las instrucciones** (aunque algo haya fallado). Lo hecho y los fallos pasan antes al checkpoint y a la bitácora del agente, porque el archivo no sobrevive.
2. **D2: `PHASE_DONE` aprobado** (migración 004).
3. **D3: se mantienen los 12 estados.**
4. **D4: entra todo lo marcado «Sí»**; F12 va aparte, medido antes de P08.S3.
5. **Benchmark live de P08.S9: se pueden usar todos los proveedores**, con solo 2 mensajes sencillos por agente y el menor gasto de tokens posible.

## Preguntas que había (resueltas arriba)
1. ¿D1: archivo de plan (A) o tablas (B)?
2. ¿D2: apruebas `PHASE_DONE` (cambio de core con migración 004)?
3. ¿D3: se mantienen los 12 estados?
4. ¿D4: entra todo lo marcado «Sí»? Si no, ¿qué se pospone?
5. ¿Permiso para el benchmark live de P08.S9 con qué proveedores (cuesta cuota; los gratuitos de OpenCode no sirven para medir carga real)?

## Consecuencias
- Aceptado este ADR: PLAN P08 se reescribe con el orden de arriba, ADR-0011 pasa a «implementado en P08.S5» y STATUS apunta a P08.S1.
- Rechazado o cambiado: se anota la decisión y P08 sigue el PLAN original; ADR-0011 queda como diseño pendiente.
- Riesgo: P08 crece (plan por fases + delegación + F4). Mitigación: cada paso cierra con su commit y su bitácora, y S9 es un gate real.
