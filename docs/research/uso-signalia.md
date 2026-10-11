# Uso real de Symphony en Signalia (3–5 oct 2026)

Registro de evidencia para futuras features. Es el primer uso de Symphony CLI sobre un proyecto ajeno a este repo, con orquestación autónoma: Leo le pedía a Claude Code que usara Symphony para repartir tareas entre agentes.

**Fuentes** (todas de solo lectura):
- Repo `Desktop/signalia`: `docs/07_planning/orchestration_waves.md`, `docs/03_architecture/changes/CHG-0002.md`, historial de git, ramas y worktrees `symphony/*`.
- Base de Symphony `~/.symphony/symphony.db`, proyecto `signalia` (30 agentes, 40 runs).

Fecha del registro: 2026-10-10. Lo que no se pudo verificar está marcado en § «Límites de este registro».

---

## 1. Comentario de Leo (2026-10-10)

> «se utilizo por primera vez symphony-cli en un proyecto ageno a este y con autonomia propia, simplemente le decia a claude code que usara symphony para hacer tareas con agentes, asi quiero que funcione el chat principal, para tareas mas sencillas y rapidas o que tal vez requieran mas atencion mia, pero que a la vez pueda decirle, haz x tarea con x agentes con tales modelos, me doy a entender?»

**Lectura (mía, para confirmar):** el chat principal tiene dos modos de trabajo y Leo pasa de uno a otro con una frase, sin cambiar de herramienta:

| Modo | Cuándo | Qué hace el chat |
|---|---|---|
| **Directo** | Tareas simples, rápidas o que piden la atención de Leo | Trabaja él mismo en la rama `symphony/chat`, como hoy |
| **Delegado** | «Haz X con N agentes y estos modelos» | Crea los agentes con el modelo pedido, reparte la tarea, vigila, integra y reporta |

Lo que Leo describe ya pasó en Signalia con Claude Code como intermediario y comandos `symphony spawn`. La feature es que **el chat principal de Symphony lo haga de forma nativa**, sin que Leo tenga que escribir un documento de orquestación a mano (§ 5).

---

## 2. Cómo se usó

El patrón, tal como quedó escrito en `orchestration_waves.md`:

1. **Plan de oleadas** en un documento: qué agente hace qué tarea, con qué modelo y sobre qué rutas. «Un dueño por ruta por oleada».
2. **Lanzamiento** por agente: `symphony spawn "<tarea>" --model <id> --failover none -d "<rutas propias + reglas>" --cwd .`. Todos los 30 agentes usaron `--failover none` y contexto `BALANCED`: Leo no quería cambios de modelo silenciosos.
3. **Máximo 5 agentes simultáneos** («laptop»). La oleada siguiente empieza cuando la anterior está mergeada.
4. **El orquestador integra:** corre el gate (`pnpm lint && typecheck && test && test:privacy && fitness`) en cada worktree, hace el commit y el merge. Kimi y Antigravity tienen revisión de diff obligatoria.
5. **Relevos declarados** en el plan por fila («claude/opus (openai)»), ejecutados a mano con `symphony switch N <modelo>`.

Reglas de modelos fijadas por Leo en CHG-0002: Opus 5.5, Sonnet 5.5, OpenAI fuerte vía Codex, y Kimi y Antigravity si están disponibles. Sin Haiku ni Copilot.

---

## 3. Números

Del git de Signalia y de la base de Symphony.

| Dato | Valor |
|---|---|
| Agentes creados | 30 (≈ 3 oct a 5 oct) |
| Estado final | 24 `COMPLETED`, 5 `FAILED`, 1 `RUNNING` |
| Merges de ramas `symphony/*` a `main` | 24 |
| Runs por modelo | gpt-5.6-sol 14 · claude/sonnet 14 · claude/opus 8 · moonshot/default 2 · gemini-3.1-pro-high 2 |
| Agentes con un solo run que terminaron bien | 16, con mediana ≈ 12 min (rango 6,7–54,8) |
| Fallas de proveedor registradas | 8 `TEMP_RATE_LIMIT` (Anthropic) y 1 `DAILY_QUOTA` (Moonshot) |
| `recovery_items` abiertos | 7 (5 `NO_HEARTBEAT`, 1 `EXECUTOR_EXITED`, 1 `RATE_LIMITED`) |
| Handoffs generados | 38 |

**Resultado por proveedor** (en runs; los `HANDED_OFF/USER_SWITCH` son cambios manuales de Leo/orquestador):

| Modelo | Completó | Falló | Notas |
|---|---|---|---|
| claude/opus | 5 | 1 | Backend, seguridad y landing. Los 4 agentes de un solo run duraron 8–20 min |
| claude/sonnet | 9 | 3 | Web, desktop y móvil. Tres fallas en la oleada de la cuota |
| openai/gpt-5.6-sol | 11 | 1 | Núcleo, medios, E2E, perf, integración. El más largo: 55 min |
| moonshot/default (Kimi) | 0 | 2 | Un `NO_HEARTBEAT` a los 15 min y una cuota diaria al minuto. Sonnet terminó ambos por `switch` |
| google/gemini-3.1-pro-high | 1* | 0 | *«Completó» en 39 s sin escribir nada. Se pasó a Sonnet, que murió con la cuota: #10 quedó sin rescate (§ 5.5) |

**Tokens:** `usage_records` suma ≈ 156,6 M de `tokens_in` y casi 0 de `tokens_out` para el proyecto. No es una medida de cuota ni de coste (§ 6).

**Los 5 agentes en paralelo agotaron la cuota rápido.** La oleada 2 (#6–#10) se lanzó 2026-10-03 18:37; Anthropic reportó rate limit a las 18:44 en cuatro de ellos y los cinco murieron entre las 18:59 y las 19:00. La oleada 4 (#16–#20) se pausó a ~22:40 por límite de uso y se reanudó a la mañana siguiente (por eso sus duraciones en la base marcan 440–450 min: es tiempo de reloj con la pausa incluida, no trabajo).

---

## 4. Qué funcionó

1. **La separación agente/modelo aguantó el uso real.** Dos agentes (#4 y #14) terminaron con un modelo distinto al pedido (Kimi → Sonnet) sobre el trabajo parcial, sin repetir la explicación de la tarea. En #4 Sonnet cerró en 4 min tras el relevo.
2. **Un worktree por agente evitó choques** con hasta 5 agentes escribiendo a la vez: ninguno tocó el árbol de trabajo principal.
3. **El rescate parcial funcionó.** En la oleada 2 el orquestador recuperó, corrió el gate y mergeó lo viable de agentes caídos (B1–B4) el mismo día.
4. **`--failover none` + relevo manual** dio control: ningún agente cambió de modelo sin que Leo lo supiera.
5. **Aislamiento por rutas** (`-d` con rutas propias): 24 merges de ramas `symphony/*` a `main`; los docs no registran conflictos de contenido.

---

## 5. Fricciones y huecos (con evidencia)

| # | Observación | Evidencia |
|---|---|---|
| 5.1 | **La cuota se agotó como causa y Symphony lo registró como otra cosa.** Los 5 agentes de la oleada 2 quedaron `FAILED` con `NO_HEARTBEAT` (4) y `CRASH` (1) a las 18:59–19:00; la causa real fue el límite de uso, que Symphony ya había visto como `TEMP_RATE_LIMIT` a las 18:44 en cuatro de ellos | `agent_runs.end_reason` frente a `provider_failures` |
| 5.2 | **Procesos zombie.** El orquestador tuvo que matarlos a mano | `orchestration_waves.md` § Oleada 2 |
| 5.3 | **Un agente `FAILED` no se reanuda con `resume`;** se usa `symphony switch N <modelo>`, incluso al mismo modelo (en la oleada 4 se usó `switch` al mismo modelo como forma de reanudar: #16–#20) | `orchestration_waves.md`; cadenas `HANDED_OFF/USER_SWITCH > EXITED` |
| 5.4 | **Los agentes no tenían permiso para `pnpm`, `python` ni `git commit`.** El orquestador corría el gate, hacía el commit y el merge de cada uno | `orchestration_waves.md` § Lecciones |
| 5.5 | **Antigravity no escribe en headless ni ejecuta comandos;** #10 quedó con carpeta vacía. Kimi no tiene sandbox y falló dos veces | § Oleada 2; CHG-0002 |
| 5.6 | **Entorno por worktree:** hay que correr `pnpm install --no-frozen-lockfile` en cada uno porque el lockfile está en `.gitignore` | § Lecciones |
| 5.7 | **Salud de proveedores con dato viejo:** «Symphony reporta Claude al 0 % (posiblemente dato viejo)» y se lanzó igual con relevos | § Plan de cierre en 3 horas |
| 5.8 | **Regla de modelos solo en documentos.** «Sin Haiku ni Copilot» no la hace cumplir nadie; hubo una sesión con Copilot (fuera de CHG-0002) que empujó a un repo público y abrió un PR, auditado y revertido | § Incidente Copilot |
| 5.9 | **Trabajo terminado sin integrar.** #30 (`COMPLETED`) tiene 12 archivos modificados sin commit en su worktree; #29 tiene una carpeta nueva sin commit y sigue `RUNNING` en la base tras un rate limit del 5 de oct. `main` siguió con trabajo manual de i18n | `git status` de los worktrees; DB |
| 5.10 | **Recovery Center sin cerrar:** 7 items abiertos de fallas ya rescatadas a mano | tabla `recovery_items` |
| 5.11 | **La coordinación vive en un Markdown.** Estado, relevos, rutas propias y pendientes se llevan en prosa en `orchestration_waves.md` (≈ 150 líneas) y se actualizan a mano | el propio archivo |

### 5.12 Hallazgo para ADR-0011

Cinco agentes largos en paralelo consumieron la cuota de Claude, Codex y Antigravity en ~22 minutos (oleada 2). Es el escenario que ADR-0011 quiere evitar: sesiones cortas por fase hacen que el plan sobreviva al corte y que la concurrencia se ajuste a lo que queda. La evidencia de Signalia refuerza la decisión, pero **no mide** el ahorro de tokens por fase; eso sigue pendiente de medir con live.

---

## 6. Features candidatas

No son decisiones: son lo que la evidencia sugiere, ordenado por cuánto respalda cada una. Las que tocan el core necesitan ADR (CLAUDE.md).

| # | Feature | Para qué | Respaldo | Se conecta con |
|---|---|---|---|---|
| F1 | **Modo delegado del chat principal:** «haz X con estos agentes y modelos». El chat crea, vigila, integra y reporta; en modo directo sigue trabajando él | Lo que Leo pidió explícitamente | § 1, § 2 | P07.5 (chat), P08 |
| F2 | **Symphony como herramienta de otros agentes** (p. ej. Claude Code): exponer `agent.create/list/switch/inspect` por MCP además de CLI, con salida estructurada | Que una sesión externa orqueste sin parsear texto. Hoy se hizo por shell | § 2 | P09.S8 (broker MCP, planeado solo con `context.*`) |
| F3 | **Plan de lote declarativo** (tareas, modelo/profile, rutas propias, dependencias, relevos, máximo simultáneo) en lugar de un Markdown a mano | Que el plan sea ejecutable y el estado se lea de Symphony | 5.11, § 2 | P08 (DAG, scheduler) |
| F4 | **Concurrencia según cuota restante:** no lanzar 5 si la cuota no alcanza para 5; escalonar | Evitar la muerte masiva de la oleada 2 | 5.12, 5.7 | P10 (salud), P08 |
| F5 | **Clasificación de fallas por causa real:** si hubo rate limit/cuota poco antes del corte, el run termina como `QUOTA_*`/`RATE_LIMITED`, no `NO_HEARTBEAT` | Decidir bien reanudar/cambiar; que Recovery Center sea fiable | 5.1, 5.10 | P10, Recovery Center |
| F6 | **Reanudar un agente `FAILED` con el mismo modelo** (`resume`), sin pasar por `switch` | Quitar el rodeo de § 5.3 | 5.3 | P06/P07.5 |
| F7 | **Gate y commit por agente configurables por proyecto** (`lint`, `typecheck`, `test`…), con commit en la rama del agente y merge solo si pasa | Hoy lo hace el orquestador a mano para cada agente | 5.4 | P08 (validación, merge) |
| F8 | **Comando de preparación del worktree** (p. ej. `pnpm install`) al crear el agente | Que el agente arranque con dependencias, sin que el orquestador las instale en cada worktree | 5.6 | P06 runtime |
| F9 | **Capacidades por proveedor que el router entienda:** «puede ejecutar comandos», «tiene sandbox», «solo lectura de archivos» | Asignar a Antigravity solo tareas sin comandos, vigilar a Kimi | 5.5 | P10 (router), P11 |
| F10 | **Lista de modelos permitidos por proyecto, aplicada por Symphony** (no solo escrita) | CHG-0002 la tenía solo en un documento | 5.8 | P10 profiles |
| F11 | **Vista de «pendiente de integrar»:** agentes terminados con cambios sin commit o sin merge | 5.9 | 5.9 | TUI Home/Recovery |
| F12 | **Contabilidad de tokens utilizable:** `tokens_out` casi nunca se llena y `tokens_in` (~157 M) no se compara con la cuota | Estimar coste por tarea para planificar oleadas (F4) | § 3 | P10 (uso) |
| F13 | **Chat principal consciente de lo que delegó:** al volver Leo, el resumen de «qué hizo cada agente, qué se integró, qué falta» sale de Symphony, no de un doc | Cierra el ciclo de F1 | 5.11 | P07.5, P09 hechos |

### Hechas (2026-10-11)
- **F5:** un run que muere sin latido, o que sale con error, poco (30 min) después de un rate limit, una cuota o un login rechazado registrado se cierra con esa causa (`RATE_LIMITED`, `QUOTA_EXHAUSTED`, `AUTH_ERROR`) y abre un item `RATE_LIMITED` o `AUTH_ERROR` en el Recovery Center en vez de `NO_HEARTBEAT` o `EXECUTOR_EXITED`.
- **F6:** `symphony resume N` (y `agent.resume`) reabre un agente `FAILED` desde su último checkpoint con el mismo proveedor y modelo (`reclaim` si murió en silencio, `restart` si salió con error). Ya no hace falta `switch`.

### Orden sugerido (tentativo)
1. **F5 y F6** son correcciones chicas, sin ADR (clasificación y reanudación). Mejoran lo que ya existe.
2. **F11 y F8** son vistas y un hook; bajo riesgo.
3. **F1, F2, F3, F4, F7** son el producto nuevo y conviene diseñarlos juntos dentro de P08 (el DAG y la validación ya están planeados); necesitan ADR.
4. **F9, F10, F12** entran con P10 reabierto o P08, según el diseño del router.

---

## 7. Límites de este registro

- No se leyó la conversación de Claude Code con Leo; el patrón sale de los documentos de Signalia y de la base de Symphony.
- `orchestration_waves.md` dice que en un momento «el orquestador (Kimi)» corrió los gates; Leo dice que le pedía a Claude Code. No se sabe qué sesión orquestó cada oleada.
- Las duraciones de #16–#20 incluyen una pausa; no son tiempo de trabajo.
- `tokens_in` no se validó contra lo que cada proveedor reporta; tratarlo como orden de magnitud.
- No se cruzaron las causas de `FAILED` con los logs de cada CLI (solo con `provider_failures` y los documentos).
- `symphony.db` estaba en uso por el daemon (WAL); se leyó en modo solo lectura y puede haber cambiado después.
- El proyecto `signalia-lsm-app` también está registrado en la base y no se analizó.
- No se midió la calidad del código de los agentes, solo su estado y su integración.
