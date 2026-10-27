# Anexo: desviaciones respecto al spec (as-built, P16.S5)

El spec (`idea.md`, `symphony_database.md`, `Symphony_CLI_User_Flow_and_Views.html`,
`Symphony_CLI_Ideal_Technology_Stack.md`) describe lo que se quería construir. Esto es lo que
cambió al construirlo y por qué. Cuando los documentos discrepan manda el orden
ADR > PLAN > DB > STACK > FLOW > IDEA.

## Arquitectura y estado

| Spec decía | Quedó así | Por qué | Referencia |
|---|---|---|---|
| Un daemon y una base de datos globales en `~/.symphony` | Un daemon y su estado **por proyecto** en `<proyecto>/.symphony/` (base, objetos, worktrees, logs, socket); en `~/.symphony` solo lo del usuario (config, caché de modelos, plugins, skills) | Se veían sesiones de otros proyectos y la base global crecía sin límite; `symphony migrate split` reparte una base vieja | ADR-0018 |
| Cada CLI de proveedor con sus herramientas propias | El **harness de Symphony**: un broker MCP (`symphony mcp serve`) con `read/write/edit/glob/grep/shell` y las herramientas de contexto, con las nativas apagadas, en 5 de 6 proveedores (Antigravity no lo admite) | Mismos permisos y mismo registro para todos los modelos; un modelo nuevo no necesita aprender otra interfaz | ADR-0019 |
| Solo proyectos de código | `[project] kind`: `code`, `document`, `media`, `general`; profiles `@write`, `@media`, `@research`; gate `files` en documentos y medios | Leo quiere usarlo para video y documentos | ADR-0020, ADR-0021 |
| Chat como una vista más | El chat general es **un agente** con worktree y rama fija `symphony/chat`, la «pre-main»: los agentes nacen de ella y se integran en ella; solo el chat va a `main`, con acción explícita | Failover del chat entre proveedores y un lugar seguro para integrar | ADR-0006 |
| Agentes de sesión larga | Sesiones cortas con **plan por fases** (`.symphony/plan/<tarea>.md`, temporal) y `PHASE_DONE` como fin de run | Contexto acotado y handoff natural entre fases | ADR-0011, ADR-0013 |

## Proveedores

| Spec decía | Quedó así | Por qué | Referencia |
|---|---|---|---|
| 5 proveedores | 6: se agregó **OpenCode** (modelos gratuitos y de terceros, ids `opencode/<modelo>`) | Modelos gratis para pruebas live y dogfooding | ADR-0012 |
| stderr solo como latido | El executor entrega stderr al adapter (`parse_stderr_line`) | Kimi y Copilot reportan sesión y errores por stderr | ADR-0009 |
| Retener comandos pesados en el scheduler | Retención **por hook** con tiempos máximos por proveedor (capa 1) | Ningún CLI expone una API de pausa; el hook es el único punto común | ADR-0003 |

## Router, aprendizaje y ML

| Spec decía | Quedó así | Por qué | Referencia |
|---|---|---|---|
| Modelo de decisión ML, compresión ML y embeddings como experimentos | **Rechazados en v1**; el router es determinista (8–23 µs) y aprende con un ajuste acotado por muestras | Sin evidencia de que las reglas se queden cortas; ML no sería determinista ni reversible | ADR-0016, ADR-0017 |

## Base de datos

| Spec decía | Quedó así | Por qué | Referencia |
|---|---|---|---|
| Esquema de DB §3 en una migración | 9 migraciones (001 core … 009 check `files`); `PHASE_DONE` en `agent_runs` (004), check `plugin` (006), sugerencias (007), tipos de trabajo (008) | Cada fase agregó lo suyo con su test de migración con datos | ADR-0013, ADR-0015, ADR-0020, ADR-0021 |
| Retención de `events` y `resource_samples` «a definir» | Al arrancar el daemon: eventos de agentes terminados con más de 30 días y muestras de más de 14 (configurable en `[retention]`); checkpoints y runs nunca | P13.S4 | `QUICKSTART.md` § configuración |
| `checkpoints.is_valid` | Se usa: un checkpoint con el diff o el resumen ilegible se marca inválido, abre un recovery `CHECKPOINT_INVALID` y el handoff cae al anterior o al worktree | Journey E (FLOW §16), P13.S3 | `docs/phases/P13-endurecimiento.md` |

## Plan y proceso

| Spec decía | Quedó así | Por qué | Referencia |
|---|---|---|---|
| Fases P00–P16 en orden numérico | Orden real: … P07 → P07.5 → P11 → P10 → P09 → P08 → P11.5 → P12 → P14 → P15 → P15.5 → P13 → P16 | Proveedores antes que multiagente, endurecer al final sobre el producto completo | ADR-0008, ADR-0010, ADR-0014 |
| Gate de uso de 2 semanas tras v0.1 | Gate por criterio de contenido (uso con los dos proveedores y notas de dolor) | El tiempo solo no decía nada | ADR-0007 |

## Pendiente (no es desviación, falta hacerlo)
- `/compact` entre tareas (ADR-0013 D2): sin implementar hasta medirlo en vivo.
- Binarios grandes con puntero en git (ADR-0020): recortado.
- Publicación de releases y del tap de Homebrew: lista en dry run, la decide Leo.
