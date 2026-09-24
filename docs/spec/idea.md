# Symphony CLI

> Runtime local para correr varios agentes de programación de distintos proveedores desde una sola terminal, sin que se pisen, sin reexplicar nada al cambiar de modelo y sin que mi computadora se vuelva inutilizable.

**Estado:** Borrador v0. Pendiente de validar en Phase 0.
**Licencia:** MIT · **Telemetría:** ninguna · **Nube propia:** ninguna
**Autenticación:** mis suscripciones, a través de los CLIs oficiales

---

## 1. La idea en una frase

Hoy uso Claude Code, Codex, Kimi, Antigravity y Copilot en terminales separadas. Cuando se me acaba la cuota de uno, tengo que abrir otro y volver a explicarle todo. Symphony se pone en medio: el **agente** (la tarea, su rama, sus cambios, su estado) vive en Symphony, y el **modelo** solo es quien lo ejecuta en ese momento, así que se puede cambiar.

## 2. Principios (no negociables)

```text
AGENT ≠ MODEL                 El agente persiste; el modelo es reemplazable.
STATE ≠ MODEL MEMORY          La verdad vive en Symphony, no en el chat del modelo.
CHECKPOINT BEFORE FAILURE     El handoff se construye durante el trabajo, nunca después de fallar.
WORKSPACES MUST BE ISOLATED   Un worktree por agente.
HEAVY WORK IS SCHEDULED       La concurrencia se permite en red y se limita en builds/tests.
SMART FEATURES ARE OPTIONAL   Nada "inteligente" puede hacer más lento el camino básico.
RULES BEFORE ML               Primero código determinístico; ML solo si un benchmark lo justifica.
CLI FIRST · LOCAL FIRST · NO TELEMETRY · PERFORMANCE BEFORE FEATURES
```

## 3. Prioridades (en orden)

1. Fiabilidad
2. Velocidad
3. Bajo consumo de RAM/CPU
4. Varios agentes a la vez
5. Elegir modelo manualmente
6. Continuidad entre proveedores
7. Herramientas y contexto compartidos
8. Ahorro de tokens
9. Routing inteligente
10. GUI

Si algo de abajo perjudica algo de arriba, se vuelve opcional o se elimina.

**Métrica principal:** 3 agentes, 3 worktrees, 2–3 proveedores distintos, y yo sigo usando la computadora normal. Si no se cumple esto, Symphony falla, tenga las funciones que tenga.

## 4. Qué es y qué no es

**Es:** una capa de ejecución que coordina los CLIs oficiales que ya uso.

**No es:**
- un reemplazo de Claude Code o Codex;
- un proxy ni un SaaS;
- algo que extraiga tokens OAuth o evada restricciones;
- un runtime de modelos locales grandes;
- un swarm autónomo sin control;
- una app Electron.

---

## 5. Arquitectura

```text
                        symphony (CLI/TUI)
                               │ IPC (named pipe / unix socket)
                               ▼
                          symphonyd
      ┌──────────────┬─────────┼──────────┬───────────────┐
  Project State   Event Bus  Scheduler  Checkpoints   Context Engine
  SQLite + Git     (hooks)   (2 capas)  (incremental)  (handoff + MCP)
      └──────────────┴─────────┼──────────┴───────────────┘
                         Agent Runtime
                               │
               Availability Filter → Router (reglas)
                               │
          ┌─────────┬──────────┼──────────┬────────────┐
       Claude     Codex    Antigravity   Kimi       Copilot
        Code       CLI        CLI        Code         CLI
```

### 5.1 Conceptos

| Concepto | Qué es |
|---|---|
| Project | Un repositorio |
| Task | Una unidad de trabajo ("implementar refresh tokens") |
| Agent | Entidad persistente responsable de una task: estado + worktree + rama + checkpoint |
| Executor | El CLI oficial que la ejecuta en este momento |
| Model | El modelo exacto que usa ese CLI |
| Profile | Un modelo virtual que expresa una intención (`@code`, `@fast`...) |

Un agente es sobre todo **estado**, no un proceso. Puedo tener 6 agentes y solo 2–3 CLIs vivos.

### 5.2 Provider Adapters

Todos los adapters implementan la misma interfaz:

```text
detect · auth_status · list_models · spawn · resume · stop
parse_event · parse_error · health · install_hooks
```

- **CLI-bridge es la base.** Cada CLI maneja sus propias credenciales y Symphony no guarda tokens.
- **Direct mode** es una optimización futura. Solo se activa si el proveedor ofrece una ruta oficial.
- Cada adapter traduce los nombres canónicos (`claude/sonnet`) a lo que espera su CLI.

### 5.3 Event Bus (hooks)

Los CLIs tienen hooks (`PreToolUse`, `PostToolUse`, `Stop`, `SessionStart`, etc.) o streams JSON (`codex exec --json`). Cada adapter los normaliza a eventos propios:

```text
AgentStarted · TurnStarted/Finished · ToolRequested/Finished · CommandRequested/Finished
FileModified · ProviderError · CheckpointCreated · AgentStopped
```

Con estos eventos se alimentan el scheduler, los checkpoints, la TUI y el audit log.

**Aislamiento de hooks:** los hooks se inyectan por worktree (sin commitearlos) o revisan la variable `SYMPHONY_AGENT_ID`. Si no existe, no hacen nada. Así no afectan cuando uso los CLIs fuera de Symphony.

**Si un CLI no expone hooks suficientes:** se usa stdout/JSON, PTY y monitoreo del árbol de procesos, en ese orden.

### 5.4 Scheduler (dos capas)

**Capa de aplicación (hooks).** Entiende la *intención* de cada operación:

| Clase | Operaciones | Costo |
|---|---|---|
| 0 | Espera de LLM / red | Gratis |
| 1 | Leer archivos, grep, git diff | Barato |
| 2 | Lint, format, typecheck pequeño | Medio |
| 3 | Tests unitarios, typecheck grande | Pesado |
| 4 | Build, tests de integración, navegador, installs | Muy pesado |

**Capa OS.** Hace *cumplir* los límites sobre el árbol de procesos completo: Job Objects en Windows, cgroups en Linux. Así también controla los procesos hijos que los hooks no ven.

**Advertencia importante:** que un CLI tenga hooks no garantiza que pueda **encolar**. Un hook normalmente permite o deniega. Si deniega, el modelo lo toma como error y puede reintentar o improvisar otra cosa. Para encolar de verdad, el hook tiene que *quedarse esperando* hasta que haya slot. Por cada CLI hay que verificar:
- el timeout máximo del hook;
- qué hace el CLI si el hook tarda.

Si un CLI no aguanta la espera, en ese CLI el scheduler solo usa la capa OS.

**Perfiles de rendimiento:** `eco` · `balanced` (3 agentes, 1 proceso pesado, CPU/RAM al 70%) · `performance` · `custom`.

La TUI siempre dice *por qué* algo espera, por ejemplo: `Agent 4 WAITING_RESOURCE → esperando el test suite de Agent 2`.

### 5.5 Checkpoints incrementales

El checkpoint se actualiza después de cada evento significativo. Nunca se genera al fallar, porque cuando un modelo se queda sin cuota ya no puede resumir nada.

Qué contiene:
- task, objetivo, rama, worktree;
- archivos tocados, `git diff` actual;
- comandos ejecutados, resultados de tests y errores;
- **la cola del transcript:** el último mensaje del asistente y su plan o lista de TODOs, leídos del archivo de sesión del CLI (Claude Code pasa `transcript_path` a los hooks; Codex guarda sus sesiones en JSONL local). Así el checkpoint sabe *qué seguía*, no solo *qué se hizo*, y no gasta tokens.
- punteros a outputs grandes (`ctx://run/883`).

Se guarda en SQLite (WAL). Los blobs grandes van a un object store direccionado por contenido (BLAKE3 + zstd).

### 5.6 Context Engine (definido para CLI-bridge)

Con CLI-bridge, Symphony **no controla el contexto de cada turno**: cada CLI maneja su propia ventana. Lo que Symphony sí controla son dos cosas.

**a) Handoff assembler.** Arma el prompt inicial en el spawn y en cada failover a partir del checkpoint:
- objetivo;
- plan vigente;
- decisiones actuales (sin las que ya se reemplazaron);
- diff;
- fallos recientes;
- archivos relevantes, por nivel:

| Nivel | Qué se incluye |
|---|---|
| L0 | Nada |
| L1 | Nombre del archivo |
| L2 | Símbolos |
| L3 | Firmas y tipos |
| L4 | Funciones relevantes |
| L5 | Archivo completo |

Aquí está el ahorro real: en lugar de 80k tokens de historial, se reconstruye un working set de 15–30k.

**b) MCP server de contexto.** Lo exponen todos los CLIs:
- `context.retrieve(ctx://...)`
- `context.search(ctx://..., "refresh 401")`
- `context.lines(ctx://file/auth.ts, 120-190)`

**Compresión determinística** (sin ML en v1):
- colapso de logs y tests ("896 passed, 1 failed + el fallo");
- JSON estructural (schema, distribución y anomalías);
- deduplicación;
- AST con tree-sitter;
- búsqueda con SQLite FTS5/BM25.

La compresión siempre es **reversible**: el original queda en el store.

**Consolidación de memoria:** separada de la compresión. Registra qué hechos del proyecto siguen vigentes y cuáles quedaron reemplazados (por ejemplo, `localStorage → superseded`, `httpOnly cookie → current`).

**Modos:** `raw` (salida de emergencia, siempre disponible) · `safe` · `balanced` · `aggressive`.

*La compresión turno a turno y los presupuestos de tokens por categoría solo aplican si algún día existe Direct mode.*

### 5.7 Routing

```text
/model claude/sonnet   → modelo exacto: se obedece; nunca se cambia en silencio
/model @code           → profile: Symphony elige entre los modelos disponibles ahora
```

1. **Availability filter (determinístico).** Descarta proveedores offline, con auth inválida, cuota agotada, contexto insuficiente, capacidad faltante o en cooldown.
2. **Scoring por reglas.** `capacidad + ajuste de contexto + salud + margen de cuota − escasez − fallos recientes − carga`. Tarda microsegundos.
3. **Explicación.** `/explain-route` muestra la decisión generada desde los scores, sin otro LLM.

**Políticas de cambio de modelo:**

| Situación | Qué hace Symphony |
|---|---|
| Agotamiento o fallo | Failover automático (si está permitido: `none` / `same-provider` / `any`) |
| Otro modelo parece mejor | Solo sugiere: `[switch] [keep] [no sugerir]` |
| Modelo elegido por mí | Nunca se sobrescribe, salvo que no esté disponible |

**Conservación de cuota:** `reserve = 0.20` por proveedor. Los profiles automáticos no gastan el último 20%; yo sí puedo usarlo manualmente.

**Model registry:**
- metadata fija: contexto, tools, visión, clase de velocidad;
- desempeño aprendido (futuro): condicionado por características de la tarea, con un mínimo de muestras antes de influir en el router.

### 5.8 Salud de proveedores

Se registra por **proveedor + cuenta + modelo**:

```text
HEALTHY · DEGRADED · THROTTLED · RATE_LIMITED · QUOTA_LOW · EXHAUSTED · AUTH_ERROR · OFFLINE · UNKNOWN · PROBING
```

- **Un 429 no significa agotado.** Puede ser RPM, TPM, un límite temporal, diario, semanal o por modelo. Cada adapter tiene su propio `parse_error()`.
- **Cuota:** se muestra como `KNOWN`, `ESTIMATED` o `UNKNOWN`. Nunca con falsa precisión, porque las suscripciones casi no exponen el porcentaje restante.

### 5.9 Worktrees y Git

Un worktree por agente en `~/.symphony/worktrees/<proyecto>/agent-00N`, con rama `symphony/<sesion>/agent-00N`.

**Dependencias.** Cada worktree nace sin `node_modules`, y 3 agentes significarían 3 instalaciones. Estrategia por orden de preferencia:
1. pnpm con store compartido;
2. symlink o hardlink del `node_modules` base si el lockfile no cambió;
3. instalar solo cuando el agente toque el lockfile.

La instalación cuenta como operación de clase 4 para el scheduler.

**Merge:**

```text
rama del agente → validación → preflight de conflictos → cola de integración → merge
```

Si hay un conflicto, la task se marca `BLOCKED` y la reviso yo (o, opcionalmente, un agente de review). Symphony nunca resuelve conflictos en silencio ni mergea a main sin una política explícita.

### 5.10 Tasks y resiliencia (tomado de Hermes)

- **DAG de dependencias:** una task no arranca hasta que sus dependencias estén `DONE`.
- **Heartbeat:** último evento, proceso vivo, esperando proveedor.
- **Reclaim:** si no hay heartbeat y el proceso murió, se marca el run como fallido, se conservan worktree y checkpoint, y se reinicia o reasigna.
- **Event log (`agent_runs`, `events`):** qué pasó, con qué modelo, cuántos reintentos y por qué falló.
- **Dispatcher determinístico.** No es un LLM supervisando.
- **Milestones opcionales** con aprobación manual. Las fases rígidas quedan fuera.

### 5.11 Validación por niveles

| Nivel | Qué corre | Cuándo |
|---|---|---|
| Tier 1 | Sintaxis, formato, lint dirigido | Siempre (barato) |
| Tier 2 | Typecheck, tests dirigidos | Siempre |
| Tier 3 | Suite completa, build, integración, seguridad | Solo en gates de integración |

Todo pasa por el scheduler.

### 5.12 Recursos compartidos

- **Skills canónicas** en `~/.symphony/skills/` y en el proyecto. Cada adapter las traduce al formato de su CLI.
- **Registro central de MCP** en `~/.symphony/mcp.toml`. Una sola instancia cuando el protocolo lo permite; si el CLI exige su propia instancia stdio, se usa modo compatibilidad.
- **Contexto canónico del proyecto:** stack, decisiones y restricciones que reciben todos los agentes.
- **Los agentes no conversan entre sí.** Se comunican a través del estado (commits, outputs, decisiones).

---

## 6. Stack

| Pieza | Elección |
|---|---|
| Core/daemon | Rust + Tokio |
| CLI/TUI | clap + ratatui + crossterm |
| Estado | SQLite (rusqlite, WAL) |
| Blobs | Object store propio, BLAKE3 + zstd |
| Código | tree-sitter |
| Búsqueda | SQLite FTS5 / BM25 (sin vector DB) |
| Config | TOML (serde) |
| Logs | tracing, con redacción de secretos |
| IPC | Named pipes (Windows) / Unix sockets |
| GUI (mucho después) | Tauri, consumiendo la misma API local |

**Nota honesta:** Rust **no** es lo que me va a dejar correr 4 agentes. La RAM se la llevan los CLIs (procesos Node de cientos de MB), los MCP y los builds. Rust sirve para no sumar otro runtime pesado, tener un binario único y controlar procesos. Lo que realmente permite la concurrencia es el scheduler. Si el spike va más rápido en otro lenguaje, está bien: el lenguaje final se decide después.

**Objetivos de consumo (no garantías):**
- daemon idle < 100 MB;
- routing en milisegundos;
- métrica real: consumo total **con** Symphony contra los **mismos** agentes corriendo sueltos.

---

## 7. Supuestos abiertos (lo que el spike tiene que validar)

| # | Supuesto | Si falla... | Test |
|---|---|---|---|
| 1 | 3 CLIs caben en mi máquina y se pueden medir | Bajo la meta a 2 agentes | A |
| 2 | Los hooks se pueden normalizar a un event bus común | Degrado a stdout/PTY | B |
| 3 | Un `PreToolUse` puede retener un comando varios minutos | El scheduler de ese CLI queda solo en capa OS | C |
| 4 | Un checkpoint sin resumen permite que otro CLI continúe | AGENT ≠ MODEL no se sostiene; Symphony queda como gestor paralelo de CLIs (igual útil) | D |
| 5 | Usar los CLIs en modo headless con mi suscripción está permitido | Ese proveedor queda solo en modo interactivo o fuera | Leer ToS |
| 6 | Hay estrategia viable para `node_modules` por worktree | Limito los agentes paralelos en proyectos JS | A |

---

## 8. Roadmap

### Phase 0: Spike (1–2 semanas, **solo Claude Code + Codex**)
- **A.** Lanzar 2–3 CLIs en worktrees y medir RAM, CPU, procesos y costo de dependencias.
- **B.** Event bus: juntar los hooks de ambos CLIs en un solo stream.
- **C.** Un `PreToolUse` retiene `npm test` 2 minutos. ¿El CLI aguanta?
- **D.** `kill` a Claude a media tarea, sin cleanup, y Codex continúa solo con el checkpoint.
- **ToS:** leer las condiciones de uso programático de cada proveedor.

**Criterio de éxito:** si C y D funcionan, la arquitectura es viable y lo demás es integración.

### Phase 1: Core
Daemon, CLI/TUI, SQLite, event bus, worktrees, 2 adapters, selección manual de modelo y checkpoints incrementales.

### Phase 2: Multi-agent runtime
Scheduler de dos capas, heartbeat/reclaim, DAG, validación por niveles y estrategia de dependencias.

### Phase 3: Context Engine
Handoff assembler, MCP de contexto, compresión determinística y consolidación de memoria.

### Phase 4: Failover y profiles
Salud de proveedores, parsers de error, failover automático, profiles con scoring por reglas y conservación de cuota.

### Phase 5: Más proveedores
Kimi, Antigravity y Copilot. Si el contrato de `ProviderAdapter` quedó bien, esto es escribir adapters, no rediseñar.

### Phase 6: Opcionales (solo con benchmark que lo justifique)
Decision engine local, desempeño aprendido, compresores ML y embeddings.

### Phase 7: GUI en Tauri
Solo cuando el CLI ya sea excelente.

### Después: `symphony plan`
El modo proyecto de la propuesta original: de una descripción a tasks + DAG + profiles sugeridos. Va como capa **encima** del runtime, no dentro.

---

## 9. Decisiones tomadas y descartadas

| Decisión | Por qué |
|---|---|
| ✅ AgentHub y Symphony se unen en un solo proyecto | Eran dos capas del mismo sistema: runtime (AgentHub) + workflow (Symphony) |
| ✅ CLI-bridge como base | Menor riesgo de ToS y no toco credenciales |
| ✅ Mecanismos de Hermes (heartbeat, reclaim, DAG, event log, dispatcher determinístico) | Ingeniería probada; el Kanban visual no hace falta |
| ✅ Ideas de Headroom (compresión por tipo, reversible), implementación propia | Sin dependencia, sin telemetría, adaptado a código y Git |
| ❌ Orquestador LLM (Haiku) supervisando | Caro e impredecible; lo reemplaza un dispatcher determinístico |
| ❌ Modelo de decisión en el camino crítico | Con 2–4 candidatos, un scoring por reglas decide igual y en microsegundos |
| ❌ Scores fijos tipo "coding: 0.95" como verdad | Arbitrarios; solo sirven de bootstrap |
| ❌ OpenCode como proveedor | Fuera del pool inicial |
| ❌ Extraer o reutilizar OAuth fuera de su CLI | Riesgo de baneo de cuenta |
| ❌ Vector DB, embeddings, ML de compresión en v1 | RAM y complejidad sin evidencia de beneficio |

---

## 10. Pendientes por verificar

- Nombres reales de los modelos disponibles en cada CLI. "Sol", "Luna", "Terra" y los profiles son placeholders.
- Si existen y qué ofrecen **Jev, Laya, NanoJev y SemIf/OpenJev** (motores de decisión mencionados en borradores anteriores). No construir nada alrededor de ellos sin verificarlos.
- Qué hooks de Kimi, Antigravity y Copilot pueden **bloquear o esperar**, no solo observar.
- Si Kimi, Antigravity y Copilot exponen su transcript como Claude Code y Codex.
- Herramientas existentes que ya orquestan CLIs con worktrees (por ejemplo, Claude Squad). Probarlas antes de construir para no rehacer lo que ya existe.

---

## 11. Definición final

> Symphony CLI es un runtime local, ligero y CLI-first que coordina varios agentes de programación sobre distintos proveedores usando mis suscripciones y los CLIs oficiales. Mantiene el estado fuera de los modelos, aísla a cada agente en su propio worktree, controla los recursos de la máquina y permite cambiar de modelo o proveedor sin reiniciar el trabajo.

**Siguiente paso:** dejar de escribir specs y hacer el Test D.
