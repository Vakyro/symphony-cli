# Symphony CLI: Plan de implementación para agentes de código

> Este plan lo escribí para que cualquier agente de código (Claude Code, Codex, Antigravity, Kimi Code, Copilot, OpenCode) pueda construir Symphony desde cero hasta la versión final probada. Está pensado para que varios agentes trabajen **uno después de otro**: cualquiera puede parar a media fase y otro (u otro día) continuar exactamente donde se quedó.
>
> Si eres un agente y es tu primera vez en este repo, lee completas las secciones **0 a 7** antes de tocar código. Después ve directo a la fase y el paso que indique `docs/progress/STATUS.md`.

### Registro de cambios del plan

| Fecha | Cambio | Por qué |
|---|---|---|
| 2026-09-24 | Nuevo **P00.S0 · Prevalidación**: probar herramientas existentes (Claude Squad y similares) y hacer el Test D a mano, antes de escribir código | IDEA §10 pedía probar lo existente y el PLAN no tenía paso para eso. Test D es el supuesto que decide el producto (IDEA §7 fila 4) y se puede probar en horas sin Rust |
| 2026-09-24 | P01.S2 investiga el **modo de interacción** (headless o PTY) y P01.S8 produce **ADR-0005** | Ningún documento decidía cómo ve e interviene Leo en la sesión de cada CLI. Headless pierde la TUI del CLI; PTY dentro de ratatui obliga a emular una terminal. Cambia P04, P05 y P07 |
| 2026-09-24 | **P08–P16 son provisionales.** Nuevo gate **P07.S10 · Replanificación** tras v0.1: uso real ≥ 2 semanas con 2 proveedores, y luego revisión de P08–P16 con ADR | El detalle de P08–P16 se escribió sin evidencia de uso; el spike y el uso diario lo van a cambiar. Evita construir 5 adapters y una GUI sobre supuestos |

---

## Índice

- [0. Cómo usar este plan](#0-cómo-usar-este-plan)
- [1. Documentación de referencia](#1-documentación-de-referencia)
- [2. Reglas que no se rompen](#2-reglas-que-no-se-rompen)
- [3. Archivos de coordinación entre agentes](#3-archivos-de-coordinación-entre-agentes)
- [4. Protocolo de sesión (inicio, trabajo, cierre)](#4-protocolo-de-sesión-inicio-trabajo-cierre)
- [5. Git: ramas, commits y tags](#5-git-ramas-commits-y-tags)
- [6. Pruebas: qué, cuándo y con qué](#6-pruebas-qué-cuándo-y-con-qué)
- [7. Skills, MCPs y herramientas del agente](#7-skills-mcps-y-herramientas-del-agente)
- [8. Mapa de fases](#8-mapa-de-fases)
- Fases: [P00](#p00--arranque-del-repositorio) · [P01](#p01--spike-de-viabilidad) · [P02](#p02--cimientos-del-core) · [P03](#p03--persistencia) · [P04](#p04--git-worktrees-y-procesos) · [P05](#p05--adapters-y-event-bus) · [P06](#p06--agent-runtime-checkpoints-y-handoff) · [P07](#p07--tui-y-release-v01) · [P08](#p08--runtime-multiagente) · [P09](#p09--context-engine) · [P10](#p10--salud-failover-y-routing) · [P11](#p11--proveedores-restantes) · [P12](#p12--plugins) · [P13](#p13--endurecimiento-y-pre-release) · [P14](#p14--sugerencias-de-modelo-y-experimentos) · [P15](#p15--gui-de-escritorio) · [P16](#p16--validación-final-y-v100)
- [Apéndice A. Plantilla de STATUS.md](#apéndice-a-plantilla-de-statusmd)
- [Apéndice B. Plantilla de bitácora de fase](#apéndice-b-plantilla-de-bitácora-de-fase)
- [Apéndice C. Plantilla de ADR](#apéndice-c-plantilla-de-adr)
- [Apéndice D. Contenido inicial de AGENTS.md](#apéndice-d-contenido-inicial-de-agentsmd)

---

## 0. Cómo usar este plan

**Estructura.** El trabajo está dividido en **17 fases (P00–P16)**. Cada fase tiene **pasos numerados (P05.S3 = fase 5, paso 3)**. Cada paso indica:

| Campo | Qué contiene |
|---|---|
| **Qué** | El resultado concreto del paso |
| **Por qué** | Qué problema o requisito del spec resuelve |
| **Cómo** | Instrucciones, archivos, crates y comandos |
| **Docs** | Qué secciones de la documentación leer antes |
| **Verifica** | El comando o la prueba que demuestra que el paso está terminado |

Al inicio de cada fase también hay: objetivo, prerrequisitos, tecnologías, skills y criterios de salida.

**Orden.** Las fases van en orden estricto: no empieces una fase si la anterior no cerró con su tag `pNN-done`. Dentro de una fase, los pasos también van en orden, salvo que el paso diga "puede hacerse en paralelo con…".

**Ejecución.** Un paso es la unidad mínima de trabajo. Termina el paso, verifica, commitea y actualiza `STATUS.md`. Si tu sesión se va a cortar, **no dejes un paso a medias sin documentarlo** (ver §4.4).

**Fuente de verdad del avance.** `docs/progress/STATUS.md` dice dónde estamos. La bitácora de cada fase (`docs/phases/PNN-*.md`) dice qué se hizo, qué funciona, qué está roto y quién lo hizo. El historial de git lo confirma.

**Cuándo detenerse y preguntarle a Leo (el humano):**
- antes de correr un CLI real con su suscripción (gasta cuota);
- antes de crear el repo remoto, hacer push por primera vez o publicar un release;
- cuando un criterio de "gate" falle (P00.S0, P01, P07.S10, P08, P14);
- cuando la documentación se contradiga y la regla de precedencia (§1.2) no lo resuelva;
- cuando haya que aceptar términos, instalar software a nivel sistema o cambiar configuración global de sus CLIs.

---

## 1. Documentación de referencia

### 1.1 Mapa de documentos

Todos viven en `docs/spec/` desde P00.S3. Cuando un paso diga "Docs: DB §3.C", significa `docs/spec/symphony_database.md`, sección 3, dominio C.

| Alias | Archivo | Para qué sirve | Cuándo leerlo |
|---|---|---|---|
| **IDEA** | `idea.md` | Visión, principios, arquitectura, supuestos abiertos, roadmap de producto | Siempre al empezar el proyecto; §2 y §5 antes de cada fase |
| **FLOW** | `Symphony_CLI_User_Flow_and_Views.html` | Flujo de usuario, 35 vistas, estados, ramas, journeys A–E, reglas UX | Antes de cualquier comando CLI, vista TUI o mensaje al usuario |
| **STACK** | `Symphony_CLI_Ideal_Technology_Stack.md` | Qué tecnología usar, dónde, por qué y qué está prohibido | Antes de agregar cualquier dependencia o crate |
| **DB** | `symphony_database.md` | 42 tablas, tipos, enums, índices, relaciones y fases | Antes de cualquier migración o consulta |
| **ER** | `symphony_er_diagram.html` | Diagrama entidad-relación por dominio | Para ver relaciones rápido; **no es fuente de verdad** (lo es DB) |
| **STACKDIAG** | `symphony_stack_diagram.html` | Cómo se conectan los componentes y tecnologías | Al diseñar la conexión entre crates o procesos |
| **SKILLS** | `Catalogo-Skills-ClaudeCode.pdf` | Skills disponibles en la máquina de Leo | Al elegir skills (ver §7) |
| **PLAN** | `PLAN.md` (este archivo, en la raíz) | Orden de trabajo y protocolo | Siempre |

Para abrir los `.html` sin navegador, léelos como texto (el contenido está en el HTML) o pídele a Leo una captura.

### 1.2 Precedencia cuando los documentos no coinciden

1. **ADRs aceptados** en `docs/adr/` (decisiones tomadas con evidencia, por ejemplo tras el spike).
2. **PLAN.md** para el *orden* y el *protocolo* de trabajo.
3. **DB** para esquema, tipos, enums y nombres de tablas y columnas.
4. **STACK** para tecnología, crates y prohibiciones.
5. **FLOW** para comportamiento visible al usuario (vistas, estados y textos).
6. **IDEA** para principios y visión general.

Si encuentras una contradicción, no la resuelvas en silencio: anótala en la bitácora de la fase en "Desviaciones del spec". Si es relevante, crea un ADR.

### 1.3 Numeración de fases: plan vs producto

IDEA y STACK usan "Fase 0–7" del **producto**. Este plan usa **P00–P16** de **implementación**. Equivalencia:

| Plan | Producto (IDEA §8 / STACK §40 / DB §6) |
|---|---|
| P00 | (preparación) |
| P01 | Fase 0 · Spike |
| P02–P07 | Fase 1 · Core |
| P08 | Fase 2 · Multi-agent runtime |
| P09 | Fase 3 · Context Engine |
| P10 | Fase 4 · Failover y profiles |
| P11–P12 | Fase 5 · Más proveedores y plugins |
| P13 | Endurecimiento (transversal, antes de 1.0) |
| P14 | Fase 6 · Opcionales |
| P15 | Fase 7 · GUI |
| P16 | Validación final y 1.0 |

---

## 2. Reglas que no se rompen

Vienen de IDEA §2 y STACK §1, §36 y §57. Si un paso parece pedir romper una, detente y pregunta.

1. **AGENT ≠ MODEL.** El agente persiste; el modelo es reemplazable. Nunca guardes "el modelo actual" en `agents`: vive en el `agent_run` abierto (DB §3.C).
2. **STATE ≠ MODEL MEMORY.** La verdad vive en Symphony (SQLite + Git + object store).
3. **CHECKPOINT BEFORE FAILURE.** Los checkpoints son incrementales. Nunca se generan "al fallar".
4. **Un worktree por agente.** Nunca dos agentes en la misma carpeta.
5. **El trabajo pesado pasa por el scheduler.**
6. **Rules before ML.** Router, dispatcher, scheduler y parsers de error son deterministas. Nada de LLM en el camino crítico.
7. **Smart features are optional.** Todo lo "inteligente" va detrás de una feature flag o config y tiene fallback.
8. **Sin telemetría ni tráfico de red oculto.** El core no hace llamadas HTTP (STACK §50).
9. **Nunca tocar credenciales de los proveedores.** No leer, copiar ni extraer tokens OAuth. Cada CLI oficial es dueño de su login.
10. **Un solo escritor en SQLite.** Los hooks y los procesos externos nunca abren la DB. Todo pasa por el daemon vía IPC (DB §1, STACK §9.2).
11. **Prohibido en el core:** Node/Python como runtime, Electron, PostgreSQL, Redis, NATS/Kafka, Docker obligatorio, ORM, gRPC, servidor HTTP en v1, vector DB, embeddings en v1, orquestador LLM, plugins nativos in-process, libgit2 como backend principal (STACK §36).
12. **Código completo.** Nada de `todo!()`, `unimplemented!()`, `unwrap()` en rutas de runtime, placeholders ni stubs "para después" en código que se commitea como terminado. Si algo queda pendiente, se documenta como pendiente en la bitácora y en un issue.
13. **No agregar dependencias sin justificarlas.** Si una crate no está en STACK §58, justifícala en la bitácora. Si es de peso (runtime, red, ML), escribe un ADR.
14. **El performance es un requisito.** La métrica principal (IDEA §3): 3 agentes, 3 worktrees, 2–3 proveedores, y la computadora sigue usable.

---

## 3. Archivos de coordinación entre agentes

Se crean en P00 y los mantiene **cada** agente en **cada** sesión.

```text
symphony/
├── PLAN.md                         # este archivo (no se edita salvo para corregirlo con ADR)
├── AGENTS.md                       # reglas cortas para cualquier agente (Apéndice D)
├── CLAUDE.md                       # 1 línea: "Lee AGENTS.md y PLAN.md"
├── GEMINI.md                       # igual (Antigravity)
├── CONSTRAINTS.md                  # presupuestos y prohibiciones verificables
├── .github/copilot-instructions.md # igual (Copilot)
└── docs/
    ├── spec/                       # documentación original (solo lectura)
    ├── progress/
    │   ├── STATUS.md               # ← dónde estamos AHORA (Apéndice A)
    │   ├── SESSIONS.md             # registro append-only de cada sesión
    │   └── LEARNINGS.md            # trampas descubiertas, comandos útiles
    ├── phases/
    │   ├── _TEMPLATE.md            # Apéndice B
    │   ├── P00-bootstrap.md
    │   ├── P01-spike.md
    │   └── ...                     # una bitácora por fase
    ├── adr/
    │   ├── 0000-template.md        # Apéndice C
    │   └── NNNN-titulo.md
    └── research/                   # investigación de CLIs, hooks, ToS
```

| Archivo | Quién lo escribe | Cuándo |
|---|---|---|
| `STATUS.md` | El agente activo | Al terminar **cada paso** y al cerrar sesión |
| `SESSIONS.md` | El agente activo | Al inicio (1 línea) y al cierre (resumen) de cada sesión |
| `docs/phases/PNN-*.md` | El agente activo | Al terminar cada paso (sección del paso) y al cerrar la fase |
| `LEARNINGS.md` | Cualquier agente | Cuando descubra algo que le ahorraría tiempo al siguiente |
| `docs/adr/*.md` | Cualquier agente | Cuando se tome una decisión que cambie el spec, la tecnología o el orden |

**Identidad del agente.** Usa siempre el formato `herramienta/modelo`, por ejemplo: `claude-code/opus`, `codex/gpt-5.x`, `antigravity/gemini-x`, `kimi-code/kimi-x`, `copilot/<modelo>`, `opencode/<modelo>`. Si no sabes el modelo exacto, pon solo la herramienta.

---

## 4. Protocolo de sesión (inicio, trabajo, cierre)

### 4.1 Al iniciar sesión (siempre, en este orden)

```bash
git status
git log --oneline -15
git branch --show-current
```

1. Lee `docs/progress/STATUS.md` completo.
2. Lee la bitácora de la fase actual (`docs/phases/PNN-*.md`), sobre todo "Qué está roto" y "Notas para el siguiente agente".
3. Lee `docs/progress/LEARNINGS.md`.
4. Lee en este plan la fase actual completa y el paso actual.
5. Lee las secciones de docs que indica el paso.
6. Corre la verificación base del repo (desde P00):
   ```bash
   cargo xtask check    # fmt + clippy + tests rápidos (desde P00.S6)
   ```
   Si falla y STATUS no lo reportaba como roto, **eso es lo primero que arreglas**, antes de avanzar.
7. Agrega una línea a `SESSIONS.md`:
   `- 2026-10-02 14:10 · codex/gpt-5.x · inicio · P05.S4 · rama phase/p05-adapters`
8. En `STATUS.md`, marca `En curso por: <tu identidad> desde <fecha>`.

**Si la memoria o el contexto de tu herramienta tiene algo distinto a STATUS.md, gana STATUS.md + git.**

### 4.2 Durante el trabajo

- Trabaja **un paso a la vez**.
- Usa commits pequeños dentro del paso si es largo (ver §5). El commit final del paso lleva el tag de paso en el mensaje.
- Si descubres algo no obvio (un flag del CLI, un bug de Windows, un comando que tarda), anótalo en `LEARNINGS.md` en ese momento, no al final.
- Si una salida de comando es enorme (tests, builds), resúmela antes de meterla a tu contexto (skill `context-mode` si la tienes, o `| tail -50` / filtrado).

### 4.3 Al terminar un paso

1. Corre el **Verifica** del paso y `cargo xtask check`.
2. Actualiza la bitácora de fase, sección del paso: qué se hizo, archivos clave, cómo se verificó, qué quedó pendiente.
3. Actualiza `STATUS.md`: paso marcado `[x]`, siguiente paso, próxima acción concreta.
4. Commit (ver §5.3).

### 4.4 Si la sesión se va a cortar a media tarea

Nunca te vayas sin dejar esto:

1. Commit del trabajo en curso con prefijo `wip:` **en la rama de fase** (nunca en `main`). Si el código no compila, igual commitea, pero dilo en el mensaje: `wip(p05.s4): adapter claude, parse_event incompleto — NO COMPILA`.
2. En `STATUS.md`, sección "Handoff para el siguiente agente":
   - qué estabas haciendo exactamente;
   - qué archivo o función estaba a medias;
   - qué ibas a hacer después (acción concreta, no "seguir");
   - qué comandos corriste y con qué resultado;
   - hipótesis descartadas (para que nadie las repita).
3. Línea de cierre en `SESSIONS.md`.
4. `git push` de la rama de fase, si hay remoto configurado.

> Es el mismo principio de Symphony: el checkpoint se escribe **antes** de que el agente muera, no después.

### 4.5 Si te atoras

| Intentos | Acción |
|---|---|
| 1–2 | Relee el spec, revisa `LEARNINGS.md`, reproduce el problema con un test mínimo |
| 3 | Usa la skill `investigate` (causa raíz). En Claude Code puedes pedir una segunda opinión con `codex:rescue` |
| 4 | Marca el paso `BLOQUEADO` en STATUS con evidencia (error, comando, lo que probaste). Si otro paso de la fase es independiente, sigue con ese; si no, detente y pregúntale a Leo |

Nunca "arregles" un test borrándolo o debilitándolo para que pase. Si un test está mal, explícalo en la bitácora y en el commit.

### 4.6 Al cerrar una fase

1. Todos los pasos `[x]` o documentados como diferidos con ADR.
2. Los **Criterios de salida** de la fase se cumplen, con la evidencia pegada en la bitácora.
3. `cargo xtask check` + pruebas de la fase en verde. CI en verde en los 3 OS si hay remoto.
4. Revisión de código del diff de la fase (skill `code-review` o `code-review-and-quality`); corrige lo que encuentre.
5. La bitácora de fase completa, incluida la sección "Estado final" (Apéndice B).
6. Merge de `phase/pNN-*` a `main` (merge commit, no squash, para conservar la autoría por paso), tag `pNN-done` y push.
7. STATUS apuntando al paso S1 de la siguiente fase.
8. Si tienes la skill `graphify-smart`, actualiza el grafo después del merge a `main`.

---

## 5. Git: ramas, commits y tags

Skill: `git-workflow-and-versioning`.

### 5.1 Ramas

- `main`: siempre compila y pasa tests. Solo recibe merges de fase cerrada.
- `phase/pNN-nombre-corto`, por ejemplo `phase/p05-adapters`: una por fase, se crea en el S1 de cada fase desde `main`.
- Si Leo pide trabajo paralelo de dos agentes, cada uno usa `phase/pNN-.../sX-nombre` y se integra a la rama de fase. **Nunca dos agentes en la misma rama al mismo tiempo.**

### 5.2 Formato de commit

Conventional Commits, con el paso del plan y trailers:

```text
feat(scheduler): cola de operaciones pesadas con semáforo [P08.S3]

- HeavySemaphore con slots configurables desde config.toml
- tool_calls pasa a QUEUED con blocked_by_tool_call_id
- QueueExplainer genera la razón humana (FLOW §10.1)

Plan-Step: P08.S3
Agent: claude-code/opus
Tests: cargo nextest run -p symphony-scheduler (34 passed)
```

Tipos: `feat`, `fix`, `test`, `refactor`, `docs`, `chore`, `perf`, `ci`, `build`, `wip` (solo en ramas de fase).

### 5.3 Cuándo commitear

| Momento | Commit |
|---|---|
| Sub-avance estable dentro de un paso largo | `feat(...)`/`test(...)` normal, sin cerrar el paso |
| Fin de paso | Commit con `[PNN.SX]` en el título + actualización de STATUS y bitácora (pueden ir en el mismo commit o en uno `docs(progress): cierre P05.S4`) |
| Sesión cortada | `wip(...)` + STATUS con handoff |
| Fin de fase | Merge a `main` + tag `pNN-done` |
| Releases | Tags `v0.1.0` (P07), `v0.5.0` (P10), `v0.9.0` (P13), `v1.0.0` (P16) |

### 5.4 Qué nunca se commitea

- Credenciales, tokens, cookies, `.env` reales.
- Transcripts reales de CLIs sin sanitizar (pasan por el redactor antes de volverse fixtures).
- Archivos de hooks inyectados en worktrees de prueba (`.claude/settings.local.json`, etc.).
- `target/`, `node_modules/`, bases `.db` locales.

---

## 6. Pruebas: qué, cuándo y con qué

Detalle completo en STACK §24–§26 y §41–§42.

| Tipo | Herramienta | Cuándo |
|---|---|---|
| Unitarias | `cargo nextest` | Cada paso con lógica |
| Estados e invariantes | `proptest` | Máquinas de estado, DAG, scheduler, routing, framing IPC |
| CLI | `trycmd` | Cada comando nuevo del binario `symphony` |
| Snapshots TUI | `insta` + `ratatui::backend::TestBackend` | Cada vista |
| Integración con procesos | Subprocesos reales + `fake-agent` | Desde P04 |
| E2E | Harness propio (`crates/testkit`) | Desde P06 |
| Fuzzing | `cargo-fuzz` | P13 (se preparan targets antes) |
| Microbenchmarks | Criterion | Rutas calientes (store, compresores, routing) |
| Benchmark principal | Harness E2E (STACK §25.2) | P08, P10, P13 y P16 |
| Cobertura | `cargo-llvm-cov` | Al cerrar fase desde P08 |

**Regla de oro de proveedores (STACK §42):** CI nunca gasta suscripciones reales.
- **L1 fixtures:** payloads de hooks y outputs sanitizados en `fixtures/providers/<cli>/`.
- **L2 `fake-agent`:** binario que simula un CLI (stream, tool call con hook, 429, cuota agotada, crash, cuelgue, error de auth). Se construye en P04.S5 y es la base de casi todos los E2E.
- **L3 live:** solo con `SYMPHONY_LIVE=1`, corrido manualmente, **con permiso de Leo**.

**Qué pruebas documentar en la bitácora:** comando exacto, número de tests, cuáles fallan y por qué. Un "funciona" sin comando de verificación no cuenta.

---

## 7. Skills, MCPs y herramientas del agente

### 7.1 Cómo usar las skills

Las skills de SKILLS (el catálogo de Leo) son de Claude Code. Varias también están disponibles para Codex, Cursor, Gemini y Copilot vía `~/.agents/skills`.
- Si tu herramienta tiene la skill, cárgala en el paso indicado.
- Si no la tiene, **aplica la práctica que describe** (la tabla dice qué hace cada una). El plan nunca depende de que una skill exista.
- Skills solo de Claude Code: `codex:*`, `update-config`, `fewer-permission-prompts`, `loop`, `schedule`.

### 7.2 Skills por actividad

| Actividad | Skill | Cuándo |
|---|---|---|
| Trabajar desde spec | `spec-driven-development` | Todas las fases: el spec está en `docs/spec/` |
| Implementar por pasos | `incremental-implementation` | Pasos grandes (adapters, scheduler, context) |
| TDD | `test-driven-development` | Máquinas de estado, DAG, scheduler, parsers, routing |
| Contratos | `api-and-interface-design` | Protocolo IPC (P02), `ProviderAdapter` (P05), `ProcessSupervisor` (P04), MCP de contexto (P09) |
| Presupuestos y reglas | `constraint-driven-development` | P00 (crear `CONSTRAINTS.md`) y cierre de cada fase |
| Evitar sobre-ingeniería | `ponytail` / `ponytail-review` / `ponytail-audit` | `ponytail-review` al cerrar cada fase; `ponytail-audit` en P13 |
| Limpieza | `simplify`, `deslop` | Después de un paso grande, antes del commit final |
| Revisión | `code-review`, `code-review-and-quality` | Cierre de cada fase (§4.6) |
| Seguridad | `security-review`, `security-and-hardening`, `cso` | P02 (IPC), P05 (hooks), P09 (ctx://), P12 (plugins), P13 |
| Debugging | `investigate` | Cualquier bug que no se entienda en 2 intentos |
| Segunda opinión | `codex:rescue` (Claude Code) | Bloqueos (§4.5) |
| Decisiones difíciles | `the-council` | Gates de P00.S0, P01, P07.S10, P08 y P14; cualquier ADR importante |
| ADRs y docs | `documentation-and-adrs` | Cada ADR; P13 (mdBook) |
| Git | `git-workflow-and-versioning` | Siempre |
| CI | `ci-cd-and-automation` | P00.S8, P13.S6 |
| Migraciones | `deprecation-and-migration` | Cada migración nueva de SQLite (P03, P08, P09, P10, P14) |
| Rendimiento | `performance-optimization` | P08.S9, P13.S3 |
| Observabilidad | `observability-and-instrumentation` | P02.S4 (tracing), P13 |
| Evaluación | `eval-harness` | P06.S8 y P09.S10 (calidad del handoff) |
| Hooks de Claude Code | `update-config` | P01.S4 y P05.S4 (entender el formato de hooks en settings) |
| Menos prompts de permiso | `fewer-permission-prompts` | Opcional, al inicio, si Claude Code pide permisos repetidos |
| Ahorro de contexto | `context-mode`, `ctx-search`, `ctx-stats` | Salidas enormes (builds, tests, logs) |
| Mapa del repo | `graphify-smart` | Después de cada merge a `main` desde P03 |
| Aprendizajes | `learn` | Complementa `LEARNINGS.md` |
| Salud del código | `health` | Cierre de P07, P10, P13 y P16 |
| Frontend GUI | `ultimate-frontend`, `design-review`, `api-and-interface-design` | P15 |
| Pruebas en navegador | `browser-testing-with-devtools` | P15 (WebView/GUI) |
| Diagramas | `artifact-diagramming`, `dataviz` | Docs de P13, gráficas de benchmarks |
| PDF de documentación | `make-pdf` | Opcional, P16 (entregables para Leo) |

**No uses** para este proyecto las skills de video (HyperFrames), las de diseño de landing pages ni `benchmark`/`gstack` (son de navegador web, no de este binario). Tampoco las científicas.

### 7.3 MCPs

Ningún MCP es obligatorio para construir Symphony. Útiles si están configurados:

| MCP / herramienta | Para qué | Fase |
|---|---|---|
| GitHub (MCP o `gh` CLI) | Crear el repo, revisar runs de CI, PRs, releases | P00.S8, cierres de fase, P13, P16 |
| Chrome DevTools MCP (vía `browser-testing-with-devtools`) | Probar la GUI | P15 |
| Buscador web / docs del agente | Investigar hooks, flags y versiones de los CLIs y crates | P01, P05, P11 |

El MCP que **Symphony construye** (`symphony mcp serve`, P09.S8) es producto, no herramienta de desarrollo.

### 7.4 Herramientas del sistema

| Herramienta | Obligatoria | Desde |
|---|---|---|
| Git | Sí | P00 |
| Rust estable (rustup) + toolchain de la plataforma (MSVC en Windows) | Sí | P00 |
| `cargo-nextest`, `cargo-deny` | Sí | P00 |
| `cargo-llvm-cov`, `cargo-fuzz`, `cargo-dist`, `git-cliff`, `typos-cli`, `lychee` | Sí, en su fase | P08, P13 |
| Claude Code CLI y Codex CLI autenticados | Sí para el spike y los live tests | P01 |
| Kimi Code, Antigravity CLI, Copilot CLI | Sí para su fase | P11 |
| Node + pnpm | Solo para repos de prueba JS y para la GUI | P01 (repo de prueba), P15 |
| mdBook | Sí | P13 |

---

## 8. Mapa de fases

| Fase | Nombre | Resultado verificable | Tag |
|---|---|---|---|
| P00 | Arranque del repositorio | Repo, workspace, docs, CI y archivos de coordinación | `p00-done` |
| P01 | Spike de viabilidad | Respuestas a los supuestos IDEA §7 con evidencia + ADRs de gate | `p01-done` |
| P02 | Cimientos del core | `symphony` ↔ `symphonyd` hablando por IPC, config y logs | `p02-done` |
| P03 | Persistencia | SQLite con las 19 tablas de Fase 1, store writer y object store | `p03-done` |
| P04 | Git, worktrees y procesos | Worktrees por agente, supervisor de procesos y `fake-agent` | `p04-done` |
| P05 | Adapters y event bus | Claude Code y Codex como executors con hooks → eventos | `p05-done` |
| P06 | Agent runtime, checkpoints y handoff | Forced-kill test: un agente sobrevive a su modelo | `p06-done` |
| P07 | TUI y release v0.1 | Journey A completo en TUI + gate de replanificación (P07.S10) | `p07-done`, `v0.1.0` |
| P08 ⚠️ | Runtime multiagente | Scheduler, DAG, validación y merge; benchmark de 3 agentes | `p08-done` |
| P09 ⚠️ | Context Engine | Compresión reversible, handoff por niveles y MCP de contexto | `p09-done` |
| P10 ⚠️ | Salud, failover y routing | Failover automático, profiles y `/explain-route` | `p10-done`, `v0.5.0` |
| P11 ⚠️ | Proveedores restantes | Kimi, Antigravity y Copilot | `p11-done` |
| P12 ⚠️ | Plugins | Host de plugins aislados y un plugin de ejemplo | `p12-done` |
| P13 ⚠️ | Endurecimiento y pre-release | Seguridad, fuzzing, recovery, benchmarks, docs, instaladores | `p13-done`, `v0.9.0` |
| P14 ⚠️ | Sugerencias y experimentos | Sugerencias de modelo por reglas; experimentos ML con gate | `p14-done` |
| P15 ⚠️ | GUI de escritorio | Tauri + SolidJS sobre la misma API | `p15-done` |
| P16 ⚠️ | Validación final | Todas las vistas y journeys probados, benchmark principal, v1.0 | `p16-done`, `v1.0.0` |

⚠️ **Provisional.** P08–P16 describen la dirección, no un contrato. Se revisan en P07.S10 con evidencia de uso real, y cualquier cambio queda en un ADR. Hasta entonces, un agente no invierte trabajo en esas fases (ni crea sus crates, tablas o vistas) y el MVP se queda en 2 proveedores: Claude Code y Codex.

---

# FASES

---

## P00 · Arranque del repositorio

**Objetivo:** dejar el repositorio listo para que cualquier agente trabaje con el protocolo de este plan.
**Por qué:** sin archivos de coordinación, un agente no puede retomar el trabajo de otro.
**Prerrequisitos:** ninguno.
**Docs:** PLAN completo; IDEA §2, §6; STACK §3, §4, §27–§29, §58–§59, §63.
**Tecnologías:** Git, Rust estable (Edition 2024), Cargo workspace, `cargo xtask`, GitHub Actions, `cargo-nextest`, `cargo-deny`.
**Skills:** `spec-driven-development`, `constraint-driven-development`, `git-workflow-and-versioning`, `ci-cd-and-automation`.

### P00.S0 · Prevalidación (antes de escribir código)
- **Qué:** dos comprobaciones baratas que pueden cambiar qué se construye.
- **Por qué:** IDEA §10 pide probar lo que ya existe antes de construir, e IDEA §11 termina con "dejar de escribir specs y hacer el Test D". Si el handoff no funciona a mano, tampoco va a funcionar con un daemon en Rust.
- **Cómo:**
  1. **Herramientas existentes.** Probar Claude Squad y al menos otra herramienta que orqueste CLIs de código con worktrees. Anotar qué cubren (worktrees, varios agentes, varios proveedores, handoff entre proveedores, límites de recursos) y qué no. Pregunta a responder: ¿qué parte de Symphony ya existe, y cuál es el diferencial real?
  2. **Test D manual.** En un repo de prueba pequeño, Claude Code trabaja en una tarea mediana. A media tarea, matarlo sin cleanup. Armar a mano un prompt solo con: objetivo, `git diff`, último comando y su resultado, y el último plan o TODOs sacados del transcript (`transcript_path`). Dárselo a Codex en el mismo worktree. Anotar si continúa sin reexplicación, si termina y si pasan los tests. Repetir en sentido inverso (Codex → Claude).
- **Requiere:** permiso de Leo (usa suscripciones reales). Lo puede hacer Leo a mano o un agente con su permiso.
- **Salida:** `prevalidacion.md` en la raíz (P00.S3 la mueve a `docs/research/`), con una tabla por herramienta y una tabla de resultados del Test D.
- **Gate:** si una herramienta existente cubre la mayor parte de IDEA §3, o si el Test D manual falla en ambos sentidos, **detente y pregúntale a Leo** antes de P00.S1. Opciones: contribuir a esa herramienta, construir solo el diferencial, o seguir como gestor paralelo de CLIs (IDEA §7 fila 4).
- **Verifica:** `prevalidacion.md` existe y Leo aprobó seguir.

### P00.S1 · Verificar el entorno
- **Qué:** confirmar que la máquina puede compilar Rust en su plataforma.
- **Cómo:** `git --version`, `rustup show`, `cargo --version`. En Windows, confirmar las MSVC Build Tools. Instalar `cargo-nextest` y `cargo-deny` (`cargo install --locked ...`).
- **Verifica:** anota las versiones en la bitácora P00. Un `cargo new --bin /tmp/hello && cargo run` funciona.

### P00.S2 · Crear el repositorio
- **Qué:** repo `symphony/` con `main`.
- **Cómo:** `git init -b main`. `.gitignore` (Rust + `.env*` + `*.db` + `*.db-wal` + `*.db-shm` + `node_modules/` + `target/` + `.symphony/`). `LICENSE` MIT. `README.md` corto: qué es, estado "en construcción", link a `docs/spec/idea.md`.
- **Verifica:** `git status` limpio tras el primer commit `chore: init repo [P00.S2]`.

### P00.S3 · Copiar la documentación
- **Qué:** `docs/spec/` con los 7 documentos de §1.1 y un `docs/spec/README.md` con la tabla de §1.1 y la precedencia de §1.2.
- **Por qué:** el spec viaja con el código y cualquier agente lo encuentra.
- **Cómo:** Leo entrega los archivos. Si no están en la máquina, **pídeselos**. Copia `PLAN.md` a la raíz.
- **Además:** mueve `prevalidacion.md` y la carpeta `prevalidacion/` (P00.S0) a `docs/research/` (juntos, así los links relativos siguen funcionando). Pasa los hallazgos H1–H7 a `LEARNINGS.md`.
- **Verifica:** `ls docs/spec` muestra los 7 archivos más el README.

### P00.S4 · Archivos de coordinación
- **Qué:** `AGENTS.md` (Apéndice D), `CLAUDE.md`, `GEMINI.md`, `.github/copilot-instructions.md` (una línea que redirige a `AGENTS.md` y `PLAN.md`), `docs/progress/STATUS.md` (Apéndice A), `SESSIONS.md`, `LEARNINGS.md`, `docs/phases/_TEMPLATE.md` (Apéndice B), `docs/adr/0000-template.md` (Apéndice C), `docs/research/.gitkeep`.
- **Verifica:** STATUS apunta a P00.S5. La bitácora `docs/phases/P00-bootstrap.md` existe con los pasos S1–S4 llenos.

### P00.S5 · CONSTRAINTS.md
- **Qué:** contrato verificable de calidad y rendimiento.
- **Cómo (skill `constraint-driven-development`):** incluir:
  - reglas de §2;
  - prohibiciones de STACK §36;
  - presupuestos de IDEA §6 y STACK §39 (daemon idle < 100 MB, routing en ms, latencia de eventos en ms, "3 agentes y la máquina usable");
  - política de `unsafe` (STACK §27);
  - política de errores (`thiserror` en crates, `miette` en binarios, sin `unwrap()` en runtime);
  - política de dependencias (§2.13).
- **Verifica:** el archivo existe y `AGENTS.md` lo referencia.

### P00.S6 · Esqueleto del workspace
- **Qué:** Cargo workspace mínimo que compila.
- **Cómo:** estructura de STACK §4.1. **Crea solo los crates que se usan ya**: `crates/protocol`, `crates/core`, `crates/daemon` (bin `symphonyd`), `crates/cli` (bin `symphony`), `crates/testkit`, `xtask/`. Los demás se crean en su fase.
  - `rust-toolchain.toml` (stable) y MSRV declarado (STACK §3.1).
  - `rustfmt.toml`, `clippy.toml`, `deny.toml`.
  - `[workspace.dependencies]` vacío por ahora (se llena al usar cada crate).
  - `[profile.release]` según STACK §38.
  - `cargo xtask check` = `cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo nextest run --workspace`.
- **Verifica:** `cargo xtask check` pasa. `cargo run -p symphony-cli -- --version` imprime la versión.

### P00.S7 · Verificar que las crates del stack existen
- **Qué:** confirmar nombre y versión actual de cada crate de STACK §58.
- **Por qué:** STACK se escribió sin compilar. Alguna crate puede no existir, llamarse distinto o tener otra versión mayor (en particular **ProcessKit**, `rmcp` y `rusqlite_migration`).
- **Cómo:** `cargo search <crate>` o crates.io. Anota una tabla en `docs/research/crates.md` con: crate, existe, versión, licencia y notas. Si ProcessKit no existe o no cumple, anótalo: STACK §60 ya define el plan B (`tokio::process` + `windows-sys` + `nix`).
- **Verifica:** existe `docs/adr/0001-versiones-y-crates.md` con la lista fijada y los reemplazos.

### P00.S8 · CI mínima
- **Qué:** GitHub Actions con matriz ubuntu, windows y macos que corre `cargo xtask check` y `cargo deny check`.
- **Cómo:** skill `ci-cd-and-automation`. Usa `Swatinem/rust-cache`. **Pregúntale a Leo antes de crear el repo remoto o hacer push.** Si todavía no hay remoto, deja el workflow listo y márcalo como "no ejecutado" en la bitácora.
- **Verifica:** el run pasa en los 3 OS, o queda documentado que falta el remoto.

### P00.S9 · Cierre
- Protocolo §4.6. Tag `p00-done`.

**Criterios de salida P00:** el workspace compila en la máquina de Leo; los archivos de coordinación existen; `CONSTRAINTS.md` existe; ADR-0001 con las crates verificadas; CI lista.

---

## P01 · Spike de viabilidad

**Objetivo:** responder con evidencia los supuestos de IDEA §7 **antes** de construir el producto.
**Por qué:** si los hooks no pueden retener comandos o el handoff no funciona, cambian P05, P06 y P08.
**Prerrequisitos:** P00. Claude Code y Codex instalados y autenticados. Permiso de Leo para usar sus suscripciones en pruebas cortas.
**Docs:** IDEA §5.2–§5.6, §7, §8; STACK §7, §40 (Phase 0), §60; FLOW §13.
**Tecnologías (solo estas, STACK §40):** Rust, Tokio, clap, serde/serde_json, Git CLI, ProcessKit (o el plan B), sysinfo, interprocess, tempfile. Estado en `events.jsonl` + `checkpoint.json`, **sin SQLite**.
**Skills:** `the-council` (gate), `update-config` (hooks de Claude Code), `investigate`, `documentation-and-adrs`.

> El código del spike vive en `spikes/` (crates `spike-*`), **fuera** de `crates/`. Es desechable: se puede borrar después. Su valor son los resultados y los ADRs.

### P01.S1 · Rama y bitácora
- `git switch -c phase/p01-spike`. Crea `docs/phases/P01-spike.md` desde la plantilla.

### P01.S2 · Investigar los contratos de los CLIs
- **Qué:** documentar para Claude Code y Codex:
  - eventos de hooks disponibles y payload de cada uno;
  - si un hook puede **bloquear o esperar**, y con qué timeout máximo;
  - qué pasa si el hook tarda;
  - dónde se configuran los hooks (proyecto o usuario) y cómo inyectarlos por worktree sin commitearlos;
  - modo headless o no interactivo, y streaming JSON (`codex exec --json`);
  - ruta y formato del transcript o sesión (`transcript_path` en Claude Code, JSONL de Codex);
  - cómo listar modelos y cómo elegir uno por flag;
  - formato de los errores de rate limit y cuota;
  - **modo de interacción:** si corre en headless, ¿cómo le manda Leo un mensaje a media tarea (¿`resume` con un prompt nuevo?)? Si corre en una PTY, ¿qué tan usable queda su TUI embebida en ratatui (colores, redimensionado, teclas)? ¿Se puede pasar de un modo al otro en la misma sesión?
- **Cómo:** documentación oficial (buscador web). Contrasta con la versión instalada (`--help`, `--version`).
- **Salida:** `docs/research/cli-claude-code.md` y `docs/research/cli-codex.md`.
- **Además:** `docs/research/tos.md` con un resumen de lo que dicen los términos de cada proveedor sobre uso programático o automatizado del CLI con suscripción. **Leo confirma**; el agente no decide sobre ToS.

### P01.S3 · Test A: recursos
- **Qué:** medir RAM, CPU, número de procesos y tiempo de arranque de 1, 2 y 3 CLIs en worktrees distintos. Medir también el costo de dependencias por worktree.
- **Cómo:** crate `spikes/spike-resources`: crea 3 worktrees de un repo de prueba JS pequeño (Next o Vite con pnpm), lanza los CLIs con una tarea trivial y muestrea con `sysinfo` cada segundo. Mide `pnpm install` en frío contra el store compartido.
- **Salida:** `spikes/results/test-a.md` con la tabla de números y la máquina usada.

### P01.S4 · Test B: event bus de hooks
- **Qué:** hooks de ambos CLIs → binario `spike-hook` → IPC local (`interprocess`) → collector → `events.jsonl` normalizado (nombres de IDEA §5.3).
- **Cómo:** configura los hooks **solo en el worktree de prueba**. `spike-hook` no hace nada si no existe `SYMPHONY_AGENT_ID`.
- **Verifica:** una sesión de cada CLI produce eventos `ToolRequested`, `CommandFinished`, `FileModified` y `TurnFinished` en el JSONL.

### P01.S5 · Test C: retener comandos
- **Qué:** un `PreToolUse` que retiene `npm test` 120 s antes de permitirlo.
- **Verifica:** por cada CLI anota si espera sin timeout, hace timeout (a los cuántos segundos) o interpreta el retraso como error, y qué hace el modelo después. Anota el timeout máximo configurable.
- **Salida:** `spikes/results/test-c.md`. Define el valor de `providers.hooks_can_hold` (DB §3.D).

### P01.S6 · Test D: handoff forzado
- **Qué:** probar AGENT ≠ MODEL.
- **Cómo:**
  1. Claude Code trabaja en una tarea mediana en el repo de prueba (por ejemplo, "agregar validación de email con tests").
  2. `spike-checkpoint` actualiza `checkpoint.json` en cada evento: objetivo, archivos tocados, `git diff`, último comando, resultado de tests y cola del transcript (último plan o TODOs).
  3. A media tarea, **mata el proceso sin cleanup** (`kill -9` / `taskkill /F /T`).
  4. Lanza Codex en el mismo worktree con un prompt armado **solo** desde `checkpoint.json`.
  5. Mide si continúa sin reexplicación, si termina y si pasan los tests.
- Repite con 3 tareas distintas y también en sentido inverso (Codex → Claude).
- **Salida:** `spikes/results/test-d.md` con una tabla de resultados.

### P01.S7 · Gate de ProcessKit
- **Qué:** STACK §60: kill del árbol sin huérfanos, streaming estable, overhead bajo y límites predecibles en el OS de Leo (y en CI si hay remoto).
- **Salida:** ADR `0002-process-layer.md` que dice ProcessKit o plan B.

### P01.S8 · Decisión de gate
- **Qué:** decidir con evidencia (skill `the-council`).
- **Salidas:**
  - ADR `0003-hooks-hold.md`: por CLI, si el scheduler podrá retener comandos por hook o solo por la capa OS.
  - ADR `0004-handoff-viability.md`: resultado de Test D (compáralo con el Test D manual de P00.S0).
  - ADR `0005-interaction-mode.md`: cómo ve e interviene Leo en cada agente. Opciones: (a) headless + vista Conversation propia de Symphony; (b) PTY embebida en la TUI; (c) headless por defecto con "attach" que abre el CLI en su propia terminal sobre la misma sesión. El ADR fija qué asumen P04.S* (PTY sí/no en `ProcessSupervisor`), P05.S4–S5 (`spawn`/`resume`) y P07.S5 (vista de agente).
  - Actualiza la tabla de IDEA §7 en la bitácora con ✅ / ⚠️ / ❌.
- **Si Test D falla:** el producto sigue como "gestor paralelo de CLIs" (IDEA §7, fila 4). Escribe en el ADR qué pasos de P06 y P10 se simplifican. **Pregúntale a Leo antes de continuar.**

### P01.S9 · Cierre
- Protocolo §4.6. Tag `p01-done`.

**Criterios de salida P01:** los 6 supuestos de IDEA §7 tienen respuesta con evidencia; ADRs 0002–0005 aceptados; Leo validó el resumen de ToS.

---

## P02 · Cimientos del core

**Objetivo:** `symphony` (cliente) y `symphonyd` (daemon) comunicándose por IPC tipado, con config y logs.
**Docs:** STACK §3, §5, §6, §11, §21, §22, §45–§47, §49; IDEA §5, §6; FLOW §4.1.
**Tecnologías:** Tokio (features mínimas, STACK §3.2), tokio-util, clap, interprocess, serde, serde_json, toml_edit, thiserror, miette, tracing + tracing-subscriber + tracing-appender, ulid, proptest, trycmd.
**Skills:** `api-and-interface-design`, `test-driven-development`, `security-review`, `observability-and-instrumentation`.

### P02.S1 · Crate `protocol`
- **Qué:** contrato IPC versionado.
- **Cómo:**
  - frame = `u32` big-endian con la longitud + JSON;
  - límite de tamaño de frame;
  - mensajes `Request{protocol, id, method, params}`, `Response`, `Event` y `Subscribe`;
  - `protocol_version = 1`; si la versión es desconocida, error explícito (STACK §46);
  - codec async sobre `interprocess`.
- **Verifica:** proptest de roundtrip de framing (frames partidos, concatenados, truncados y sobredimensionados).

### P02.S2 · Crate `core`: tipos de dominio
- **Qué:** newtypes de ID (ULID); enums **exactamente** con los valores de DB (`AgentState` con 12 estados, `TaskStatus`, `RunStatus`, `ProviderState`, `FailoverPolicy`, `ContextMode`, `ExecutionMode`, etc.) con conversión a y desde `&str`; transiciones válidas de `AgentState` y `TaskStatus` como funciones puras.
- **Cómo:** TDD. Las transiciones salen de FLOW §7 y §9.2 y de IDEA §5.10.
- **Verifica:** tests de todas las transiciones válidas e inválidas; proptest "una transición inválida nunca cambia el estado".

### P02.S3 · Config
- **Qué:** carga y creación de `~/.symphony/config.toml` y `<proyecto>/.symphony/project.toml`.
- **Cómo:** esquema de STACK §11 e IDEA §5; defaults razonables (FLOW regla 5). Edición con `toml_edit` conservando comentarios. Resolución de rutas cross-OS (si agregas `dirs`/`directories`, justifícalo en la bitácora).
- **Verifica:** tests con `tempfile`: crear, leer y editar sin perder comentarios.

### P02.S4 · Logging y redacción
- **Qué:** `tracing` con spans (`project_id`, `agent_id`, `run_id`…), archivo rotativo en `~/.symphony/logs/` y redactor central (STACK §22).
- **Verifica:** tests del redactor con `Authorization: Bearer …`, API keys conocidas, cookies y variables de entorno de credenciales.

### P02.S5 · Daemon `symphonyd`
- **Qué:** proceso único con lock de instancia; servidor IPC; métodos `ping`, `status` y `shutdown`; apagado ordenado con `CancellationToken`; socket o pipe accesible **solo** por el usuario actual (STACK §47); timeouts por request.
- **Verifica:** test de integración: dos daemons → el segundo sale con error claro.

### P02.S6 · Cliente `symphony`
- **Qué:** clap con `symphony` (sin args abre TUI, placeholder "TUI llega en P07"), `status`, `daemon start|stop|status` y `--version`. Autoarranca el daemon si no está vivo (IDEA §10). Errores con `miette`.
- **Verifica:** tests `trycmd` de cada comando.

### P02.S7 · Integración
- **Qué:** test E2E: CLI arranca el daemon → `ping` → matar el daemon → el CLI detecta y lo reinicia.
- **Verifica:** pasa en los 3 OS (CI).

### P02.S8 · Cierre
- Revisión de seguridad del IPC (skill `security-review`). Protocolo §4.6. Tag `p02-done`.

**Criterios de salida P02:** `symphony status` funciona en frío (autoarranca el daemon); protocolo con proptest; nada de `unwrap()` en runtime.

---

## P03 · Persistencia

**Objetivo:** estado persistente con SQLite (un solo escritor) y object store.
**Docs:** DB completo (sobre todo §1, §3 A/B/C/D/F/G/I, §6, §7); STACK §9, §10.
**Tecnologías:** rusqlite (bundled), rusqlite_migration, ulid, blake3, zstd, tempfile, Criterion.
**Skills:** `deprecation-and-migration`, `test-driven-development`, `performance-optimization`.

### P03.S1 · Crate `store` y migración 001
- **Qué:** `migrations/001_core.sql` con **las 19 tablas de Fase 1** (DB §6): projects, sessions, tasks, agents, worktrees, agent_runs, executor_changes, messages, providers, models, provider_failures, events, tool_calls, checkpoints, checkpoint_refs, blobs, context_objects, handoffs, recovery_items.
- **Cómo:** copia fiel de DB §3: tipos, `NOT NULL`, `CHECK` de enums, FKs, `UNIQUE` e **índices parciales** (un agente activo por task; un run abierto por agente). Las FKs hacia tablas de fases futuras (`profiles`, `provider_accounts`, `routing_decisions`) **se dejan como columnas nullable sin FK** y se agregan en su migración, con una nota en la bitácora. PRAGMAs de DB §7 al abrir.
- **Verifica:** test que aplica la migración en DB vacía y verifica `PRAGMA foreign_key_check`. Tests de que los índices parciales rechazan duplicados.

### P03.S2 · Store writer
- **Qué:** hilo dedicado con la única conexión de escritura; recibe comandos por canal acotado; escribe en lotes (~100 ms o 50 eventos); conexiones de solo lectura aparte (STACK §9.2).
- **Verifica:** test de 4 productores concurrentes con 10k eventos: 0 errores `SQLITE_BUSY` y orden por productor conservado.

### P03.S3 · Repositorios
- **Qué:** funciones SQL escritas a mano (sin ORM, STACK §43) para las entidades de Fase 1: crear, leer y actualizar estado. Conversión con los enums de `core`.
- **Verifica:** tests unitarios por entidad con DB temporal.

### P03.S4 · Crate `object-store`
- **Qué:** `put(bytes) -> hash` (BLAKE3), guardado con zstd en `~/.symphony/objects/<h[0:2]>/<h>` con escritura atómica (tempfile + rename); fila en `blobs`; `ref_count`; `gc()`.
- **Verifica:** dedupe (dos puts iguales = 1 archivo); simulación de crash a media escritura sin archivos truncados; roundtrip.

### P03.S5 · Recuperación al arrancar
- **Qué:** al iniciar el daemon: sesiones `ACTIVE` sin daemon vivo → `INTERRUPTED` + `recovery_items(SESSION_INTERRUPTED)` (FLOW §16, Journey E).
- **Verifica:** test: crea sesión, mata el daemon, reinicia y aparece el recovery item.

### P03.S6 · Benchmarks
- **Qué:** Criterion: insertar 10k y 100k eventos; leer la Home query (DB §5).
- **Salida:** números en la bitácora (serán la línea base).

### P03.S7 · Cierre
- Protocolo §4.6. Tag `p03-done`.

**Criterios de salida P03:** migración 001 idéntica a DB; un solo escritor probado con concurrencia; object store atómico.

---

## P04 · Git, worktrees y procesos

**Objetivo:** aislar agentes en worktrees y controlar procesos externos de forma confiable.
**Docs:** STACK §7, §12, §48, §60; IDEA §5.9; DB §3.C (`worktrees`), §3.F (`processes`); ADR-0002.
**Tecnologías:** Git CLI (porcelain), ProcessKit o plan B (según ADR-0002), windows-sys (Job Objects), nix, sysinfo, parser/stripper de ANSI (crate pequeña o VTE).
**Skills:** `api-and-interface-design`, `incremental-implementation`, `investigate`.

### P04.S1 · Crate `git`
- **Qué:** wrapper del CLI de Git: `worktree add/list/remove`, ramas `symphony/<session>/agent-NNN`, `status --porcelain=v2 -z`, `diff --no-ext-diff` + numstat, commit actual y preflight de merge (`merge-tree` o prueba en worktree temporal).
- **Verifica:** tests con repos temporales, incluidas rutas con espacios y Windows.

### P04.S2 · Estrategia de dependencias
- **Qué:** detección por lockfile (STACK §12.2) y estrategias `PNPM_STORE`, `LINK`, `INSTALL` y `NONE`; guardar `deps_strategy` y `deps_lock_hash` (DB `worktrees`).
- **Por qué:** IDEA §5.9: evitar 3 instalaciones completas.
- **Verifica:** con un repo pnpm de prueba, el segundo worktree reutiliza el store (mide el tiempo y anótalo).

### P04.S3 · Crate `process`
- **Qué:** trait `ProcessSupervisor` (STACK §7.1) + implementación según ADR-0002: spawn con o sin PTY, streaming de stdout/stderr, `terminate_tree`, `suspend` si el OS lo permite, `stats` con sysinfo.
- **Regla:** ProcessKit (o `windows-sys`/`nix`) **no se filtran** fuera de este crate.
- **Verifica:** test de un árbol de procesos (script que lanza hijos y nietos) → `terminate_tree` → 0 huérfanos, en los 3 OS.

### P04.S4 · Saneamiento de ANSI
- **Qué:** filtro `raw → sanitizer → TUI/log` (STACK §48).
- **Verifica:** tests con secuencias de escape maliciosas (cambio de título, OSC 52, borrado de pantalla).

### P04.S5 · `fake-agent` (testkit)
- **Qué:** binario que imita un CLI de proveedor, configurado por un guion YAML/TOML: emite texto en stream; llama `symphony hook emit` como lo haría un hook real; edita archivos; corre comandos; y simula 429 temporal, cuota agotada, error de auth, crash, cuelgue y sesión con transcript JSONL.
- **Por qué:** STACK §42 L2. Sin esto no hay E2E sin gastar suscripciones.
- **Verifica:** cada escenario tiene un test que valida su salida.

### P04.S6 · Integración
- **Qué:** crear worktree → lanzar `fake-agent` ahí → leer su stream → `terminate_tree` → borrar worktree.
- **Verifica:** pasa en CI 3 OS.

### P04.S7 · Cierre
- Protocolo §4.6. Tag `p04-done`.

**Criterios de salida P04:** worktrees por agente; kill sin huérfanos en los 3 OS; `fake-agent` con todos los escenarios.

---

## P05 · Adapters y event bus

**Objetivo:** Claude Code y Codex como executors cuyos hooks alimentan un event bus común.
**Docs:** IDEA §5.2, §5.3; STACK §6.3, §18, §20; DB §3.D, §3.F (`events`, `tool_calls`); `docs/research/cli-*.md`; ADR-0003.
**Tecnologías:** Tokio channels (mpsc/broadcast/watch/oneshot), regex, serde_json.
**Skills:** `api-and-interface-design`, `update-config` (formato de hooks de Claude Code), `security-review`, `test-driven-development`.

### P05.S1 · Trait `ProviderAdapter`
- **Qué:** `crates/adapters/common` con el trait de STACK §18.1 (`detect`, `auth_status`, `list_models`, `spawn`, `resume`, `stop`, `parse_event`, `parse_error`, `health`, `install_hooks`) y el enum de eventos canónicos (IDEA §5.3).
- **Regla:** el core nunca hace `if provider == "claude"`.
- **Verifica:** un adapter `fake` (sobre `fake-agent`) implementa el trait y pasa una suite de contrato compartida.

### P05.S2 · Event bus
- **Qué:** bus en el daemon (STACK §6.3) con suscriptores: persistencia (`events` vía store writer, en lote), TUI (broadcast) y estado actual (watch).
- **Verifica:** test de backpressure: un suscriptor lento no bloquea al productor ni pierde eventos persistidos.

### P05.S3 · `symphony hook emit`
- **Qué:** subcomando que leen los hooks: lee el JSON de stdin, agrega `SYMPHONY_AGENT_ID`/`RUN_ID` desde el entorno y lo envía al daemon por IPC. **Si no hay `SYMPHONY_AGENT_ID`, sale 0 sin hacer nada** (IDEA §5.3). Soporta respuesta de "retener": espera la decisión del daemon hasta el timeout configurado (en P05 el daemon siempre permite; el scheduler llega en P08).
- **Verifica:** `trycmd` sin env (no-op) y con env (evento llega al bus); latencia medida.

### P05.S4 · Adapter Claude Code
- **Qué:** implementación completa según `docs/research/cli-claude-code.md`:
  - detección y versión;
  - estado de auth **sin leer credenciales** (solo lo que reporta el CLI);
  - lista de modelos;
  - spawn en el worktree con el modelo exacto;
  - hooks inyectados **por worktree** en un archivo local excluido de git (`.git/info/exclude`);
  - `parse_event`, `parse_error` (taxonomía de DB `provider_failures`);
  - lector incremental de la cola del transcript.
- **Verifica:** tests L1 con fixtures sanitizados de P01 en `fixtures/providers/claude-code/`; un live test L3 opt-in.

### P05.S5 · Adapter Codex
- **Qué:** lo mismo para Codex (`docs/research/cli-codex.md`), usando el stream JSON cuando exista.
- **Verifica:** L1 + L3 opt-in.

### P05.S6 · Registro de proveedores y modelos
- **Qué:** al arrancar (y con `symphony providers refresh`), detectar CLIs y llenar `providers` y `models`. Estados de FLOW §4.3 (`READY`, `LOGIN_REQUIRED`, `NOT_FOUND`, `ERROR`). Comando `symphony providers`.
- **Verifica:** `trycmd` con PATH simulado (solo fake, fake + claude, ninguno).

### P05.S7 · Cierre
- Revisión de seguridad de hooks (inyección de comandos, env, rutas). Protocolo §4.6. Tag `p05-done`.

**Criterios de salida P05:** una sesión real de cada CLI (con permiso de Leo) produce eventos canónicos en `events`; ningún archivo de hooks queda fuera del worktree ni commiteado.

---

## P06 · Agent runtime, checkpoints y handoff

**Objetivo:** el corazón del producto: un agente que sobrevive a la muerte de su modelo.
**Docs:** IDEA §2, §5.5, §5.6 (handoff assembler v1), §5.7 (políticas), §5.10; DB §3.C, §3.G (`checkpoints`, `handoffs`), §3.I; FLOW §6, §7, §13, §16, Journey C; ADR-0004.
**Tecnologías:** tokio-util (cancelación), store, object-store, git, process, adapters.
**Skills:** `test-driven-development`, `incremental-implementation`, `eval-harness`, `the-council` (si Test D fue ⚠️).

### P06.S1 · Ciclo de vida del agente
- **Qué:** flujo de creación de FLOW §6: task + agente + worktree + checkpoint inicial → spawn del run con **modelo exacto**. No se crea un agente "medio roto": si falla el worktree, se hace rollback. Estados y **razón humana** obligatoria en esperas y bloqueos (FLOW §7, regla UX 1).
- **Verifica:** tests del flujo feliz y de cada rama de error de FLOW §6.

### P06.S2 · Mensajes y tool calls
- **Qué:** espejo de conversación en `messages` (mensajes largos al object store); registro de `tool_calls` desde eventos.
- **Verifica:** una sesión con fake-agent deja mensajes y tool calls coherentes.

### P06.S3 · Checkpoints incrementales
- **Qué:** actualizar el checkpoint en cada evento significativo (archivo modificado, comando terminado, fin de turno), con el contenido de IDEA §5.5 y DB `checkpoints`: objetivo, `plan_tail`, `current_step`, `next_step`, `head_commit`, `diff_object_id`, `summary_json`. Conservar los últimos N más los usados en handoffs.
- **Verifica:** test: 50 eventos → checkpoints monótonos por `seq`; ninguno referencia objetos inexistentes (proptest).

### P06.S4 · Handoff v1
- **Qué:** prompt de arranque armado desde el checkpoint (objetivo, plan vigente, diff, fallos recientes, archivos tocados) con plantilla fija. Guardar `handoffs` con `tokens_sent` estimados y `outcome`.
- **Verifica:** snapshot (`insta`) del prompt para un checkpoint fijo.

### P06.S5 · Cambio de executor
- **Qué:**
  - `symphony switch <agente> --model <provider/model>` (manual);
  - failover básico: si el adapter clasifica `QUOTA_EXHAUSTED` o `AUTH` y la `failover_policy` lo permite, se elige el siguiente proveedor **habilitado** por orden de config (el routing completo llega en P10);
  - registro en `executor_changes` y separador visible en la conversación (FLOW §13.4);
  - con `failover = NONE` → `WAITING_PROVIDER` + recovery item.
- **Verifica:** E2E con fake-agent: cuota agotada → switch → mismo agente, task, worktree y rama.

### P06.S6 · Heartbeat y reclaim
- **Qué:** `last_heartbeat_at` (en memoria, persistido cada N s); si el proceso muere o no hay heartbeat → run `FAILED`, worktree y checkpoint intactos, `recovery_items`. Opciones de FLOW §16: restart, failover, pause, stop.
- **Verifica:** E2E con los escenarios crash y hang de fake-agent.

### P06.S7 · Comandos de agente
- **Qué:** `spawn`, `agents`, `send`, `pause`, `resume`, `stop`, `kill`, `switch`, `diff`, `logs`, `inspect` (IDEA §5 y la lista de comandos de la spec).
- **Verifica:** `trycmd` de cada uno.

### P06.S8 · Prueba de aceptación: forced kill
- **Qué:** IDEA §8, Test D, como prueba automatizada:
  1. fake-agent A trabaja;
  2. `kill` sin cleanup;
  3. fake-agent B continúa solo con el handoff;
  4. se verifica que termina la tarea (usa un guion que depende del handoff).

  Además, **una ejecución live** Claude Code → Codex con permiso de Leo, con resultados en la bitácora. Usa `eval-harness` para registrar el caso de forma repetible.
- **Verifica:** test E2E verde + resultado live documentado.

### P06.S9 · Cierre
- Protocolo §4.6. Tag `p06-done`.

**Criterios de salida P06:** Journey C (FLOW §20) funciona con fake-agent; forced kill pasa; ningún agente queda con dos runs abiertos (índice parcial + proptest).

---

## P07 · TUI y release v0.1

**Objetivo:** la experiencia principal en terminal (FLOW) y un primer binario usable.
**Docs:** FLOW §1–§7, §12, §13, §16, §18 (vistas 01–13, 22, 24, 30), §21; IDEA §5.4 (mensajes de espera).
**Tecnologías:** ratatui, crossterm, insta (TestBackend).
**Skills:** `api-and-interface-design`, `simplify`, `health`.

### P07.S1 · Arquitectura TUI
- **Qué:** crate `tui`. La TUI **solo** habla con el daemon por IPC (suscripción de eventos + requests); nunca abre la DB. Loop de render sin bloquear, con redibujado por eventos.
- **Verifica:** la TUI arranca contra un daemon con datos de prueba sin tocar el disco de estado.

### P07.S2 · Launch, First-run y Provider Setup
- **Qué:** vistas 01–03 con todas las ramas de FLOW §4 (proyecto reconocido, sin Symphony, no es repo, estado inconsistente → Recovery).

### P07.S3 · Home
- **Qué:** vista 04 con Agent List, Provider Strip, System Strip (placeholder de métricas hasta P08) y Command Bar. Estados de FLOW §5 (vacío, activo, completados, problema crítico).

### P07.S4 · New Agent y Model Picker básico
- **Qué:** vistas 05 y 13 (en este punto solo **modelo exacto**; los profiles llegan en P10). Ramas de FLOW §6.

### P07.S5 · Vista de agente
- **Qué:** vistas 06–09 y 12 (Overview, Conversation, Activity, Changes, History), con el separador de cambio de executor.

### P07.S6 · Providers y Recovery Center
- **Qué:** vistas 22, 24 y 30.

### P07.S7 · Snapshots y E2E
- **Qué:** `insta` para cada vista y estado. Journey A completo con fake-agent. Prueba manual con Claude Code real (con permiso).
- **Verifica:** snapshots aprobados; Journey A documentado con capturas o texto en la bitácora.

### P07.S8 · Release v0.1.0
- **Qué:** build release local para el OS de Leo, `CHANGELOG.md` con git-cliff y tag `v0.1.0`. (La pipeline de instaladores completa llega en P13.)

### P07.S9 · Cierre
- `health`, protocolo §4.6. Tags `p07-done` y `v0.1.0`.

### P07.S10 · Replanificación (gate)
- **Qué:** Leo usa v0.1 en trabajo real al menos 2 semanas, solo con Claude Code y Codex. Después se revisan P08–P16 con esa evidencia.
- **Por qué:** P08–P16 se escribieron antes de tener producto. El uso diario dice qué duele de verdad (¿recursos?, ¿handoff?, ¿merge?) y qué sobra.
- **Cómo:**
  1. Durante el uso, Leo (o el agente, con su permiso) anota en `docs/research/uso-v0.1.md`: qué usó, qué le faltó, qué falló, cuántos handoffs hubo y si funcionaron, y si la máquina se trabó.
  2. Al terminar, con `the-council`: por cada fase de P08–P16, decidir **mantener**, **recortar**, **reordenar** o **eliminar**, citando la evidencia.
  3. Actualiza el mapa de fases (§8) y el registro de cambios del plan. Quita la marca ⚠️ solo de las fases confirmadas.
- **Salida:** ADR `NNNN-replanificacion-post-v01.md`.
- **Verifica:** Leo aprueba el ADR. P08 no empieza sin él.

**Criterios de salida P07:** Leo puede abrir `symphony`, crear un agente con Claude Code o Codex, verlo trabajar, forzar un switch y revisar el diff, todo desde la TUI. Existe el ADR de replanificación de P07.S10 aprobado por Leo.

---

## P08 · Runtime multiagente

**Objetivo:** 3 agentes en paralelo sin que la máquina se trabe.
**Prerrequisito:** ADR de replanificación de P07.S10 aprobado. Si ese ADR cambió esta fase, gana el ADR (§1.2).
**Docs:** IDEA §5.4, §5.9–§5.11; STACK §7.2, §8, §23, §25.2; DB §3.B, §3.F, §3.H, §6 (Fase 2); FLOW §9, §10, §14, §15, Journeys B y D; ADR-0003.
**Tecnologías:** `tokio::sync::Semaphore`, BinaryHeap, sysinfo, Job Objects / cgroups v2 / process groups, notify (en P09), cargo-llvm-cov.
**Skills:** `test-driven-development`, `performance-optimization`, `deprecation-and-migration`, `the-council` (si el benchmark falla).

### P08.S1 · Migración 002
- **Qué:** tablas de Fase 2: task_dependencies, milestones, processes, resource_samples, validation_runs, validation_checks, merge_requests, merge_conflicts y reviews (DB §3).
- **Verifica:** migración 001 → 002 sobre una DB con datos de P07 sin pérdida.

### P08.S2 · Clasificador de operaciones y Resource Manager
- **Qué:** reglas de clase 0–4 (IDEA §5.4) sobre `tool_name` + comando; muestreo con sysinfo cada 5–10 s en `resource_samples`, agregado por minuto a las 24 h.
- **Verifica:** tabla de casos (`npm test` → 3, `pnpm build` → 4, `rg` → 1…).

### P08.S3 · Scheduler
- **Qué:** crate `scheduler` (STACK §8): HeavySemaphore, cola por prioridad, fairness entre agentes, perfiles `eco`/`balanced`/`performance`/`custom` desde config, y QueueExplainer (texto humano de FLOW §10.1). Integración con `hook emit`: `QUEUED` → retención → `ALLOW`. `tool_calls.blocked_by_tool_call_id` y `enforcement`.
- **Verifica:** simulación determinista + proptest: nunca más de N pesados; ningún agente espera indefinidamente con slots libres.

### P08.S4 · Enforcement a nivel OS
- **Qué:** Job Objects (Windows), cgroups v2 (Linux) y process groups (macOS) sobre el árbol de cada run. En los CLIs sin hook-hold (ADR-0003), el límite se aplica aquí. Llenar `processes`.
- **Verifica:** test que lanza una carga CPU-bound bajo límite y mide que se respeta (por OS en CI).

### P08.S5 · DAG de tareas y milestones
- **Qué:** dependencias con detección de ciclos; transiciones automáticas `WAITING_DEPENDENCY` ↔ `READY`; dependencia fallida → `BLOCKED` con decisión (FLOW §9.3); milestones con gate (FLOW §9.4).
- **Verifica:** proptest "un DAG nunca acepta ciclos".

### P08.S6 · Validation engine
- **Qué:** tiers 1–3 desde `project.toml` (STACK §23); corren a través del scheduler; guardan resultados; resumen de tests (colapso básico); feedback al agente (FLOW §15.2).
- **Verifica:** repo de prueba con un test que falla → resumen correcto + agente recibe el fallo.

### P08.S7 · Merge, conflictos y review agent
- **Qué:** preflight (FLOW §14.2), cola de integración, conflicto → `BLOCKED` + `merge_conflicts`, review agent como task `REVIEW` (FLOW §15.3). **Nunca merge a main sin política explícita.**
- **Verifica:** Journey D con dos fake-agents que tocan el mismo archivo.

### P08.S8 · Vistas TUI de Fase 2
- **Qué:** vistas 15–19 y 25–29 (FLOW §18) + System Strip real.
- **Verifica:** snapshots `insta`.

### P08.S9 · Benchmark principal (gate)
- **Qué:** harness E2E de STACK §25.2: 3 agentes, 3 worktrees, un test pesado. Primero con fake-agents y luego live (Claude Code + Codex + un tercero si ya existe, con permiso de Leo), mientras Leo usa la máquina normalmente.
- **Salida:** tabla de RAM total, CPU p50/p95, número de procesos, latencia de cola y latencia de eventos, más la opinión de Leo sobre la fluidez de la máquina.
- **Si falla la métrica principal:** `the-council` + ADR con el ajuste (perfiles, límites, slots). **No avanzar sin resolverlo.**

### P08.S10 · Cierre
- Cobertura (`cargo-llvm-cov`) de scheduler, estados y DAG anotada. `ponytail-review`. Protocolo §4.6. Tag `p08-done`.

**Criterios de salida P08:** Journeys B y D pasan; benchmark principal aprobado por Leo.

---

## P09 · Context Engine

**Objetivo:** handoffs más baratos y precisos, con recuperación reversible vía MCP.
**Docs:** IDEA §5.6, §5.12; STACK §13–§16; DB §3.G, §3.J, §6 (Fase 3); FLOW §11, vistas 20, 21, 32, 33, 35.
**Tecnologías:** tree-sitter (gramáticas iniciales: TypeScript/JavaScript, Rust, Python), SQLite FTS5/BM25, ignore, notify, rmcp.
**Skills:** `api-and-interface-design`, `security-review` (ctx://), `eval-harness`, `performance-optimization`.

### P09.S1 · Migración 003
- **Qué:** context_chunks, `context_fts` (FTS5 external content), handoff_items, context_retrievals, project_facts, skills y mcp_servers.

### P09.S2 · Context objects y URIs
- **Qué:** parser de `ctx://` con protección contra path traversal (STACK §47); creación de objetos desde tool outputs, diffs y archivos; chunks + FTS5.
- **Verifica:** fuzz target del parser preparado; tests de traversal (`ctx://file/../../etc/passwd`).

### P09.S3 · Compresores deterministas
- **Qué:** LogCollapser, TestSummaryCompressor, JsonStructuralCompressor, Deduplicator y GitDiffReducer (STACK §14.3). El original siempre queda en el object store.
- **Verifica:** corpus golden en `fixtures/context/` (logs de npm/vitest/cargo/pytest, JSON grandes) con snapshots; Criterion.

### P09.S4 · AST por niveles
- **Qué:** AstReducer L0–L5 (IDEA §5.6) con tree-sitter; fallback a chunks si no hay gramática.
- **Verifica:** snapshots por lenguaje y nivel.

### P09.S5 · File watcher
- **Qué:** un watcher por proyecto (no por agente) con debounce, respetando `.gitignore`.

### P09.S6 · Handoff assembler v2
- **Qué:** reemplaza el v1 de P06. Modos `raw`/`safe`/`balanced`/`aggressive`, secciones y niveles, estimación de tokens y `handoff_items`. Comandos `/context inspect`, `/context stats` y `/context raw`.
- **Verifica:** para un checkpoint fijo, `raw` ≥ `safe` ≥ `balanced` ≥ `aggressive` en tokens; `raw` no omite nada.

### P09.S7 · Consolidación de hechos
- **Qué:** `project_facts` con `CURRENT`, `SUPERSEDED` y `CONFLICT` (IDEA §5.6); `symphony fact set/list`; hechos incluidos en el handoff; conflicto → rama de FLOW §11.4.
- **Verifica:** índice parcial (un `CURRENT` por clave); test de reemplazo.

### P09.S8 · Broker MCP
- **Qué:** `symphony mcp serve` (rmcp, stdio) con `context.retrieve`, `context.search` y `context.lines`, reenviados al daemon por IPC (STACK §15). Los adapters lo registran en cada CLI por worktree. Registro en `context_retrievals` (`found = 0` = miss).
- **Verifica:** test de protocolo MCP; live opt-in: el CLI llama `context.search` y recibe el fragmento.

### P09.S9 · Skills y MCP compartidos
- **Qué:** skills canónicas en `~/.symphony/skills` y `.symphony/skills` (STACK §16) traducidas por cada adapter; registro `mcp.toml`; vistas 20, 21, 32, 33 y 35 (parte de contexto).

### P09.S10 · Evaluación del handoff
- **Qué:** repetir el forced kill de P06.S8 con `raw` contra `balanced`: tokens enviados, retrievals, misses y si terminó la tarea (`eval-harness`).
- **Salida:** tabla en la bitácora. Si `balanced` pierde efectividad, se ajustan los defaults y se documenta en un ADR.

### P09.S11 · Cierre
- Protocolo §4.6. Tag `p09-done`.

---

## P10 · Salud, failover y routing

**Objetivo:** failover automático correcto y selección por profiles, sin LLM.
**Docs:** IDEA §5.7, §5.8; STACK §19, §20; DB §3.D, §3.E, §6 (Fase 4); FLOW §8, §12, §13, vistas 13, 14, 22, 23, 24, 35.
**Tecnologías:** regex, proptest, cargo-fuzz (targets de parsers).
**Skills:** `test-driven-development`, `the-council` (pesos iniciales de profiles), `health`.

### P10.S1 · Migración 004
- **Qué:** provider_accounts, provider_health, usage_records, profiles, profile_models, routing_decisions y routing_candidates; se agregan las FKs pendientes de la 001.

### P10.S2 · Máquina de estados de salud
- **Qué:** estados de DB `provider_health` por provider + cuenta + modelo; parsers completos por adapter (un 429 ≠ agotado); `retry_after` y `reset_at`; `PROBING`.
- **Verifica:** fixtures de cada tipo de falla; fuzz targets de los parsers.

### P10.S3 · Cuota y uso
- **Qué:** `quota_certainty` `KNOWN`/`ESTIMATED`/`UNKNOWN` (sin falsa precisión, FLOW §12.3); reserva por proveedor desde config; `usage_records` reportados contra estimados; vista Usage.

### P10.S4 · Router determinista
- **Qué:** availability filter (IDEA §5.7) + scoring con pesos por profile (`weights_json`) + escasez y reserva; `routing_decisions` y `routing_candidates` con `reject_reason`; `/explain-route` generado desde los factores.
- **Verifica:** proptest "un modelo no elegible nunca gana"; tabla de casos de FLOW §8.4.

### P10.S5 · Failover completo y Model Picker
- **Qué:** reemplaza el failover básico de P06 por el de IDEA §5.7: política `none`/`same-provider`/`any`, modelo exacto nunca sobrescrito salvo que no esté disponible, y todas las ramas de FLOW §13.3. Model Picker con profiles (vista 13) y Explain Route (14).
- **Verifica:** Journey C con cada política; test de reserva de cuota.

### P10.S6 · Cierre
- `health`. Benchmark de routing (latencia en µs–ms). Protocolo §4.6. Tags `p10-done` y `v0.5.0`.

---

## P11 · Proveedores restantes

**Objetivo:** Kimi Code, Antigravity y Copilot CLI con el mismo contrato.
**Docs:** STACK §18.4–§18.6; P01.S2 como modelo de investigación.
**Skills:** `investigate`, `incremental-implementation`.

### P11.S1 · Investigación
- **Qué:** `docs/research/cli-kimi.md`, `cli-antigravity.md` y `cli-copilot.md` con los mismos puntos de P01.S2, más un mini Test C (retención) por CLI. Actualiza el valor de `hooks_can_hold` y ToS (Leo confirma).

### P11.S2 · Adapter Kimi
### P11.S3 · Adapter Antigravity
### P11.S4 · Adapter Copilot
- **Para cada uno:** implementación del trait, fixtures L1, suite de contrato compartida y live L3 opt-in. **Sin cambios en el core**: si un adapter necesita cambiar el core, se hace un ADR primero.

### P11.S5 · Matriz de handoff
- **Qué:** forced kill cruzado entre proveedores: con fake para todas las combinaciones y live para al menos Claude→Kimi, Codex→Copilot y Antigravity→Claude (con permiso de Leo).
- **Salida:** matriz en la bitácora.

### P11.S6 · Cierre
- Protocolo §4.6. Tag `p11-done`.

---

## P12 · Plugins

**Objetivo:** extensiones de terceros sin riesgo para el daemon.
**Docs:** STACK §17, §46, §47; IDEA §5.12.
**Skills:** `security-and-hardening`, `api-and-interface-design`.

### P12.S1 · Protocolo y manifiesto
- **Qué:** `plugin.toml` (nombre, versión, `plugin_api`, tipo: `validator`/`context-transformer`/`tool`/`provider-adapter`, ejecutable), protocolo JSON versionado por stdin/stdout.

### P12.S2 · Plugin host
- **Qué:** descubrimiento, arranque vía ProcessSupervisor (con límites), timeouts y políticas: **un plugin nunca ejecuta comandos sin pasar por las políticas de Symphony**.
- **Verifica:** un plugin que crashea no afecta al daemon; uno que excede el tiempo se mata.

### P12.S3 · Plugin de ejemplo
- **Qué:** `examples/plugins/validator-todo-check` (ejemplo: falla si el diff agrega `TODO` sin issue) con tests.

### P12.S4 · Cierre
- Protocolo §4.6. Tag `p12-done`.

---

## P13 · Endurecimiento y pre-release

**Objetivo:** que sea seguro, estable, medible, documentado e instalable.
**Docs:** STACK §22, §24–§32, §47–§49, §53–§54, §59; CONSTRAINTS.md.
**Tecnologías:** cargo-fuzz, cargo-llvm-cov, cargo-deny, typos-cli, lychee, mdBook, Mermaid, cargo-dist, git-cliff, GitHub attestations.
**Skills:** `security-review`, `security-and-hardening`, `cso`, `performance-optimization`, `observability-and-instrumentation`, `ponytail-audit`, `deslop`, `documentation-and-adrs`, `ci-cd-and-automation`, `artifact-diagramming`.

### P13.S1 · Revisión de seguridad
- **Qué:** IPC, hook emit, ctx://, plugins, redacción y permisos de socket. Corrige lo que salga.

### P13.S2 · Fuzzing
- **Qué:** targets de STACK §24.5 (stdout de CLIs, frames IPC, JSON/TOML, parsers de error, ANSI, URIs `ctx://`). Corre cada uno al menos 30 min localmente y agrega corpus a CI (corto).

### P13.S3 · Recuperación y estabilidad
- **Qué:** Journey E completo: matar el daemon durante escrituras, reiniciar a media sesión, checkpoint corrupto (FLOW §16). Pruebas de larga duración (4 h con fake-agents).

### P13.S4 · Rendimiento
- **Qué:** harness E2E completo (STACK §25.2), comparación contra CONSTRAINTS y optimización donde no cumpla. Retención de `events` y `resource_samples` (DB §8, pendientes).

### P13.S5 · Calidad del repo
- **Qué:** `cargo-deny`, cobertura en scheduler/estados/persistencia/routing/recovery/parsers, `typos`, `lychee` y `ponytail-audit` (borrar lo que sobra).

### P13.S6 · Documentación
- **Qué:** mdBook en `docs/book/`: arquitectura, protocolo, adapters, cómo escribir un plugin, troubleshooting, configuración y referencia de comandos. ER en Mermaid generado desde las migraciones (DB como explicación, migraciones como verdad, STACK §44).

### P13.S7 · Pipeline de release
- **Qué:** cargo-dist (zip/tar.xz, instalador shell y PowerShell, Homebrew), checksums SHA-256, attestations, CHANGELOG con git-cliff. **Publicar requiere permiso de Leo.**

### P13.S8 · Cierre
- Protocolo §4.6. Tags `p13-done` y `v0.9.0`.

---

## P14 · Sugerencias de modelo y experimentos

**Objetivo:** completar la funcionalidad de sugerencias (FLOW §8.5) con reglas y evaluar los experimentos opcionales **con evidencia**.
**Docs:** IDEA §5.7 (tabla de políticas), §8 Phase 6; STACK §19.1, §37; DB §3.E (`model_suggestions`, `learned_adjustment`).
**Skills:** `the-council`, `eval-harness`, `ponytail`.

### P14.S1 · Migración 005
- **Qué:** `model_suggestions`; activación de `profile_models.learned_adjustment`.

### P14.S2 · Sugerencias por reglas
- **Qué:** detectar cambios de tipo de tarea (por ejemplo, documentación → refactor multiarchivo, por archivos tocados y tipo de comandos) y proponer `[Switch] [Keep] [Don't suggest again]`. **Nunca cambiar automáticamente.**
- **Verifica:** tests de heurísticas; vista en TUI.

### P14.S3 · Desempeño aprendido
- **Qué:** ajuste por `(modelo, características de la tarea)` con un mínimo de muestras antes de influir (IDEA §5.7); el umbral queda definido en un ADR.

### P14.S4 · Experimentos ML (opcional, con gate)
- **Qué:** solo si P10 y P13 muestran que las reglas se quedan cortas: decision model local, compresión ML o embeddings, cada uno detrás de la feature `experimental-ml` y **medido** contra las reglas (latencia, RAM, acierto).
- **Salida:** ADR que acepta o rechaza cada uno. **Rechazar es un resultado válido.**

### P14.S5 · Cierre
- Protocolo §4.6. Tag `p14-done`.

---

## P15 · GUI de escritorio

**Objetivo:** GUI opcional que refleja las mismas vistas; ninguna función crítica vive solo aquí.
**Docs:** STACK §35, §55; FLOW §18 (inventario completo) y regla 9; STACKDIAG.
**Tecnologías:** Tauri 2, SolidJS, TypeScript, Vite, CSS con design tokens (Tailwind solo si se justifica).
**Skills:** `ultimate-frontend`, `design-review`, `api-and-interface-design`, `browser-testing-with-devtools`, `dataviz`.

### P15.S1 · Puente
- **Qué:** el lado Rust de Tauri usa el crate `protocol` como cliente IPC del daemon. **Sin lógica de negocio en la GUI.**

### P15.S2 · Scaffold
- **Qué:** `apps/gui` (Tauri 2 + Solid + TS + Vite), tema claro/oscuro y tokens.

### P15.S3 · Vistas
- **Qué:** las 35 vistas de FLOW §18 con la misma lógica y estados que la TUI. Prioridad: Home, Agent, Providers, System, Recovery, Tasks y Context.

### P15.S4 · Diseño y QA
- **Qué:** `design-review`; pruebas del WebView con DevTools; tests de componentes (Vitest).

### P15.S5 · Empaquetado
- **Qué:** bundles Tauri por OS dentro de la pipeline de release (opcional aparte del CLI).

### P15.S6 · Cierre
- Protocolo §4.6. Tag `p15-done`.

---

## P16 · Validación final y v1.0.0

**Objetivo:** demostrar que el programa completo funciona como dicen los documentos.
**Docs:** todos. En particular IDEA §2–§3, FLOW §18–§21, DB §5, STACK §25.2 y §64.
**Skills:** `the-council` (veredicto), `health`, `code-review-and-quality`, `make-pdf` (entregable opcional).

### P16.S1 · Checklist de aceptación
- **Qué:** crear `docs/phases/P16-acceptance.md` con una fila por cada elemento, su evidencia (comando, test o captura) y el resultado:
  - las 35 vistas de FLOW §18;
  - los 5 journeys (A–E) de FLOW §20;
  - las 10 reglas UX de FLOW §21;
  - los principios de IDEA §2;
  - las 42 tablas de DB (cada una escrita y leída por alguna función);
  - las consultas de DB §5;
  - los comandos de la spec.

### P16.S2 · Pruebas E2E finales
- **Qué:** suite E2E completa con fake-agents en CI (3 OS) y sesión live de Leo con los 5 proveedores:
  - 3 agentes simultáneos con proveedores distintos;
  - failover real por cuota (o simulado si no se agota);
  - forced kill;
  - conflicto de merge;
  - recovery tras cerrar la terminal y reiniciar la computadora.

### P16.S3 · Benchmark principal final
- **Qué:** métrica de IDEA §3 y todas las de STACK §25.2, comparadas contra CONSTRAINTS y contra la línea base de P08.
- **Salida:** tabla final + veredicto de Leo sobre la usabilidad de la máquina.

### P16.S4 · Instalación limpia
- **Qué:** instalar desde los artefactos de release en una máquina o VM limpia por OS: solo binario + git + un CLI de proveedor (STACK §64). Primer uso guiado (FLOW §4).

### P16.S5 · Documentación "as-built"
- **Qué:** actualizar `docs/spec/` con un anexo de desviaciones (lo que cambió respecto al spec y por qué, con links a los ADRs), el README final, la guía de usuario en mdBook y el CHANGELOG.

### P16.S6 · Release v1.0.0
- **Qué:** con permiso de Leo, tag `v1.0.0` y publicación.

### P16.S7 · Retrospectiva y cierre
- **Qué:** bitácora P16 con: qué funciona, qué quedó fuera, deuda técnica (si usaron `ponytail-debt`, inclúyela), qué agente hizo qué fases (resumen desde `SESSIONS.md`) y recomendaciones para v1.1. Tags `p16-done` y `v1.0.0`.

**Criterios de salida P16 (= proyecto terminado):** checklist de aceptación sin filas en rojo (o con ADR de exclusión aprobado por Leo); benchmark principal aprobado; release v1.0.0 instalable en Windows, Linux y macOS.

---

## Apéndice A. Plantilla de `STATUS.md`

```markdown
# STATUS — Symphony CLI

**Actualizado:** 2026-10-02 18:40 · por codex/gpt-5.x
**Fase actual:** P05 · Adapters y event bus (rama `phase/p05-adapters`)
**Paso actual:** P05.S4 · Adapter Claude Code
**Estado del paso:** EN CURSO | BLOQUEADO | LISTO PARA VERIFICAR
**En curso por:** codex/gpt-5.x desde 2026-10-02 14:10  ← bórralo al cerrar sesión

## Salud del repo
- `cargo xtask check`: ✅ / ❌ (detalle)
- CI en main: ✅ / ❌ / sin remoto
- Tests conocidos en rojo: (lista o "ninguno")

## Progreso de la fase actual
- [x] P05.S1 Trait ProviderAdapter
- [x] P05.S2 Event bus
- [x] P05.S3 symphony hook emit
- [ ] P05.S4 Adapter Claude Code   ← aquí
- [ ] P05.S5 Adapter Codex
- [ ] P05.S6 Registro de proveedores
- [ ] P05.S7 Cierre

## Próxima acción concreta
Implementar `parse_error` en `crates/adapters/claude/src/errors.rs` para los
fixtures `fixtures/providers/claude-code/errors/*.json`; hoy 3 de 7 pasan.

## Handoff para el siguiente agente
(llenar si la sesión se cortó a media tarea — ver PLAN §4.4)
- Estaba haciendo:
- Archivo/función a medias:
- Comandos corridos y resultado:
- Hipótesis descartadas:

## Bloqueos y preguntas para Leo
- (ninguno)

## Fases
| Fase | Estado | Tag |
|---|---|---|
| P00 | ✅ | p00-done |
| P01 | ✅ | p01-done |
| ... | | |
| P05 | 🟡 en curso | |
```

---

## Apéndice B. Plantilla de bitácora de fase

Archivo: `docs/phases/PNN-nombre.md`

```markdown
# PNN · Nombre de la fase

| Campo | Valor |
|---|---|
| Estado | EN CURSO / CERRADA / CERRADA CON PENDIENTES |
| Rama | phase/pNN-... |
| Inicio / cierre | 2026-.. / 2026-.. |
| Agentes que trabajaron | claude-code/opus (S1–S3), codex/gpt-5.x (S4–S7) |
| Tag | pNN-done (commit abc1234) |
| Docs usados | IDEA §5.3, DB §3.D, STACK §18 |

## Pasos

### PNN.S1 · Título — ✅ / 🟡 / ❌
- **Agente:** codex/gpt-5.x · **Fecha:** 2026-..
- **Qué se hizo:** …
- **Archivos clave:** `crates/...`
- **Cómo se verificó:** `cargo nextest run -p ...` → 34 passed
- **Commits:** abc1234, def5678
- **Pendiente / notas:** …

(repetir por paso)

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|

## Decisiones tomadas
- ADR-NNNN: …

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|

## Métricas
(benchmarks, tiempos, RAM, cobertura — con comando)

## Pruebas
- Comando(s): …
- Totales: N passed, M failed (cuáles y por qué)

## Estado final
Resumen de 3–5 líneas para Leo.

## Notas para el siguiente agente
Trampas, comandos útiles y lo que no vale la pena reintentar.
```

---

## Apéndice C. Plantilla de ADR

Archivo: `docs/adr/NNNN-titulo-corto.md`

```markdown
# ADR-NNNN · Título

- **Estado:** PROPUESTO / ACEPTADO / REEMPLAZADO por ADR-XXXX / RECHAZADO
- **Fecha:** 2026-..
- **Autor:** agente (herramienta/modelo) · **Aprobado por:** Leo (si aplica)
- **Fase/paso:** PNN.SX

## Contexto
Qué problema hay y qué dice el spec (con referencia).

## Opciones consideradas
1. …  2. …  3. …

## Decisión
Qué se elige.

## Evidencia
Datos, benchmarks, resultados del spike, links a docs/research.

## Consecuencias
Qué cambia en el plan, el spec o el código. Qué pasos se ven afectados.
```

---

## Apéndice D. Contenido inicial de `AGENTS.md`

```markdown
# AGENTS.md — Symphony CLI

Eres un agente de código trabajando en Symphony CLI, un runtime local en Rust que
coordina varios CLIs de IA (Claude Code, Codex, Antigravity, Kimi, Copilot).

## Antes de hacer nada
1. Lee `PLAN.md` §0–§7 si es tu primera sesión en este repo.
2. Lee `docs/progress/STATUS.md` → te dice fase, paso y próxima acción.
3. Lee la bitácora de la fase actual en `docs/phases/`.
4. Lee `docs/progress/LEARNINGS.md` y `CONSTRAINTS.md`.
5. Corre `cargo xtask check`.

## Reglas
- Un paso del plan a la vez. Verifica, commitea y actualiza STATUS + bitácora al terminar.
- Commits: Conventional Commits + `[PNN.SX]` + trailers `Plan-Step:` y `Agent:`.
- Nunca trabajes directo en `main`. Rama: `phase/pNN-...`.
- Si te vas a cortar: commit `wip`, llena "Handoff para el siguiente agente" en STATUS.
- Spec en `docs/spec/`; precedencia en PLAN §1.2. Contradicciones → bitácora/ADR.
- Prohibido: tocar credenciales de proveedores, telemetría, llamadas de red en el core,
  dependencias fuera de STACK sin justificar, `unwrap()` en runtime, stubs o TODOs como
  "terminado", debilitar tests para que pasen.
- CI nunca usa suscripciones reales: usa `fake-agent`. Live tests solo con
  `SYMPHONY_LIVE=1` y permiso de Leo.
- Pregunta a Leo antes de: usar su suscripción, crear o pushear el remoto, publicar
  releases, o cuando un gate falle.

## Identidad
Firma como `herramienta/modelo` (ej. `claude-code/opus`, `codex/gpt-5.x`).
```
