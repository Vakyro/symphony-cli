# P08 · Runtime multiagente

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p08-runtime-multiagente |
| Inicio / cierre | 2026-10-11 / — |
| Agentes que trabajaron | claude-code/sonnet-5.5 (S1–S6) |
| Tag | p08-done (pendiente) |
| Docs usados | ADR-0011, ADR-0013, DB §3.B/§3.C/§3.H, PLAN §P08, CONSTRAINTS A12 |

## Diseño (ADR-0013, aceptado por Leo el 2026-10-11)
- **Plan por agente (D1):** archivo temporal `.symphony/plan/<task_id>.md` en el worktree. Mientras la tarea corre documenta lo hecho y lo que falló; al terminarla se borran las instrucciones, aunque algo haya fallado. Antes de borrarlo, lo hecho y los fallos pasan al checkpoint y a la bitácora del agente (el archivo no sobrevive).
- **Sesión nueva por fase (D2):** el run saliente cierra con `PHASE_DONE` y estado `HANDED_OFF`; el daemon abre otro con el mismo proveedor y modelo y un handoff acotado. Migración 004.
- **Estados (D3):** se mantienen los 12; la fase es dato del plan, no estado del agente.
- **Alcance (D4):** F1–F4, F7–F11, F13. F12 (contabilidad de tokens) se mide antes de S3.
- **Benchmark live (S9):** todos los proveedores, 2 mensajes sencillos por agente, mínimo de tokens.
- **Instalación (CONSTRAINTS A12):** hooks, skills y MCP por invocación; nada se escribe en el proyecto del usuario salvo el worktree del agente.

## Pasos

### P08.S1 · Migración 004 — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-11
- **Qué se hizo:** migración `004_multiagent.sql` con `milestones`, `task_dependencies`, `processes`, `resource_samples`, `validation_runs`, `validation_checks`, `merge_requests`, `merge_conflicts` y `reviews`; FK `tasks.milestone_id → milestones` (reconstrucción de `tasks`); `PHASE_DONE` en el CHECK de `agent_runs.end_reason` (reconstrucción de `agent_runs`) y en `RunEndReason`; spec de DB actualizada.
- **Archivos clave:** `migrations/004_multiagent.sql`, `crates/store/src/lib.rs`, `crates/store/src/tests.rs`, `crates/core/src/enums.rs`, `docs/spec/symphony_database.md`.
- **Cómo se verificó:** `cargo nextest run -p symphony-store -p symphony-core` → 102 passed. El test `migration_004_keeps_data_and_adds_milestones_and_phase_done` parte de una DB en la 003 con tasks, agente, dos runs, tool call y mensaje; comprueba que no se pierde nada, `foreign_key_check` en 0, `PHASE_DONE` aceptado y un motivo inventado rechazado, un solo run abierto por agente, FK de milestone, y DAG sin auto-dependencias ni duplicados.
- **Pendiente / notas:** los enums de las tablas nuevas (estado de milestone, trigger de validación, etc.) se agregan al core en el paso que los usa (S5, S6, S7), con su fila en el spec.

### P08.S2 · Clasificador de operaciones y Resource Manager — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-11
- **Qué se hizo:** crate `symphony-scheduler` con `classify(tool_name, command) -> OpClass` (clases 0–4 de IDEA §5.4): reglas por programa y subcomando (`npm`/`pnpm`/`yarn`/`bun`, `cargo`, `git`, `go`, `docker`, `pip`…), costo del segmento más caro en comandos compuestos (`&&`, `;`, `|`), prefijos (`sudo`, `FOO=1`, `cmd /c`, `pwsh -Command`, `bash -c`) y rutas/extensiones ignorados; un comando desconocido es «medio», nunca «gratis». El recorder ya clasifica con él (antes todo comando era 2). Resource Manager en el daemon: muestreo de CPU y RAM con `sysinfo` cada 5 s hacia `resource_samples` y agregado por minuto de lo que pasa de 24 h (cada 120 muestras).
- **Archivos clave:** `crates/scheduler/src/classify.rs`, `crates/daemon/src/resources.rs`, `crates/store/src/resources.rs`, `crates/daemon/src/recorder.rs`.
- **Cómo se verificó:** `cargo xtask check` → 474 passed. Tabla de casos del PLAN (`npm test` → 3, `pnpm build` → 4, `rg` → 1) y más de 30 comandos; proptest: cualquier texto cae en 0–4 y añadir un segmento nunca baja la clase; test de compactación (idempotente, promedio de CPU, máximo de slots); test del daemon con muestras reales de CPU/RAM.
- **Cambio de comportamiento:** `git status` y similares pasan de clase 2 a 1 (lectura); se ajustó el test `session_mirrors_conversation_and_tool_calls`.
- **Pendiente / notas:** `heavy_slots_used` queda en 0 y el total sale del perfil (1, o 3 en `performance`) hasta que S3 traiga el semáforo; `process_count` espera a S4 (árbol de procesos). El primer refresco de CPU de `sysinfo` tarda cientos de ms en Windows (el test espera por sondeo, no por tiempo fijo).

### P08.S3 · Scheduler — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-11
- **Qué se hizo:** en `symphony-scheduler`, la cola síncrona y determinista (`Scheduler`): solo las clases 3–4 ocupan un slot (`Limits::max_heavy`), a lo sumo una clase 4 a la vez, techos de CPU/RAM que retienen a los pesados (y que dejan de retener tras `max_wait_ms`), orden por prioridad → menos pesados corriendo del agente → antigüedad, `explain()` con el texto humano de la espera, y `agent_concurrency()` (F4: el tope de agentes del lote baja según la cuota **conocida** restante; sin dato no se inventa un recorte). En el daemon, `Scheduling` (esperas por `oneshot`, latido de 1 s, arrendamiento de 30 min por slot, liberación cuando el agente cambia de estado) y el gate de `hook.emit`: un `PreToolUse` pesado se registra en `tool_calls` (`QUEUED`/`RUNNING`, `enforcement = HOOK_HOLD`, `queue_reason`, `blocked_by_tool_call_id`), el agente pasa a `WAITING_RESOURCE` mientras espera, y si el turno no llega en el tope del hook se rechaza (`DENIED`) con la razón; `symphony hook emit` imprime el `deny` con esa razón. Tiempos por proveedor en `HoldTimes` (Claude: retiene 100 s, el cliente espera 110 s, `timeout` del hook 120 s; Codex: 40/50/60 s, ADR-0003). Config nueva en `[performance]`: `max_heavy`, `cpu_ceiling_pct`, `ram_ceiling_pct`, `hold_max_secs`. El Resource Manager (S2) alimenta la presión.
- **Archivos clave:** `crates/scheduler/src/{queue,concurrency}.rs`, `crates/daemon/src/scheduling.rs`, `crates/daemon/src/server.rs` (`gate_heavy_tool`), `crates/cli/src/hook.rs`, `crates/adapters/common/src/hooks.rs` (`HoldTimes`), `crates/store/src/repo.rs`.
- **Cómo se verificó:** `cargo xtask check` → 485 passed. Proptest sobre secuencias arbitrarias (envíos, liberaciones, abandonos, presión, reloj): nunca más de N pesados, nunca dos clase 4, y nadie espera con slots libres. L2 con `symphonyd` real (`crates/cli/tests/hook_scheduler.rs`): corre con slot libre, espera con el slot ocupado (`QUEUED`, bloqueado por el `tool_call` correcto, agente en `WAITING_RESOURCE`), se concede al liberar, y se rechaza tras el tope con la razón en la salida del hook.
- **Pendiente / notas:** (1) Sin `tool_use_id` (algunos CLIs) o sin `run_id` el hook pasa sin retención; la capa OS (S4) los cubre. (2) El rechazo funciona con el formato `hookSpecificOutput` común a Claude y Codex (P01 Test B), pero la retención real con un CLI vivo no se probó en L3 (gasta cuota; ⛔ Leo). (3) F12 (contabilidad de tokens) sigue sin medirse en vivo: `usage_records` distingue `REPORTED`/`ESTIMATED` y F4 solo usa cuota `KNOWN`, así que no depende de él. (4) `agent_concurrency()` queda lista; la consume S5 (plan de lote).

### P08.S4 · Enforcement a nivel OS — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-11
- **Qué se hizo:** `crates/process` ya ponía cada CLI en un grupo del SO (Job Object en Windows, cgroup v2 o process group en Unix, ADR-0002) y tenía kill-tree, suspend y límites; S4 lo conecta. (1) Cada lanzamiento aplica `[performance]` al árbol: CPU por perfil (`ECO` 1 núcleo, `BALANCED` 2, `PERFORMANCE` sin límite), y memoria y procesos solo si se piden (`os_cpu_cores`, `os_max_memory_mb`, `os_max_processes`); el config se relee en cada lanzamiento. Donde el SO no puede aplicarlos se avisa (`warn`) y queda el control por concurrencia (S3). (2) `Supervised::process_tree()` da la foto del árbol (pid, padre, comando, RSS, CPU) y el executor la guarda cada 5 s en `processes` (comando redactado, pico de memoria, tiempo de CPU, `ended_at` al desaparecer, atribución a la `tool_call` cuando solo hay una corriendo); al terminar el run se cierra todo lo abierto.
- **Archivos clave:** `crates/process/src/lib.rs` (`ProcInfo`, `process_tree`), `crates/store/src/processes.rs`, `crates/scheduler/src/queue.rs` (`OsLimits`), `crates/daemon/src/executor.rs` (`os_limits`, `record_processes`), `crates/core/src/config.rs`.
- **Cómo se verificó:** `cargo xtask check` → 494 passed. Test de carga CPU-bound (`node` en bucle) con 0,5 núcleos: el SO lo respeta (Windows, Job Object; en Unix sin cgroup delegado el test lo reporta y no afirma). Test del árbol de 10 procesos. Tests de la tabla `processes` (altas con padres, picos, cierres, pid reutilizado) y L2 con `symphonyd` real: la raíz del run queda registrada y cerrada.
- **Pendiente / notas:** (1) La CI en ubuntu y macos corre el mismo test de CPU: en Linux solo afirma si el cgroup está delegado; en macOS no hay límites (ADR-0002) y solo cuenta la concurrencia. (2) La foto usa `sysinfo` sobre todos los procesos cada 5 s por run: si pesa con 5+ agentes, pasar a refrescar solo los pids del grupo. (3) Un límite de memoria duro puede matar al CLI; por eso no hay uno por defecto. (4) Falta la vista del árbol (S8).
- **Corrección de paso:** el proptest de S2 encontró que una línea vacía se clasificaba «media» y bajaba al añadirle un comando barato; una línea vacía es clase 1 (test de regresión).

### P08.S5 · DAG de tareas, milestones y plan por fases — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-11
- **Qué se hizo:**
  - **DAG** (`symphony-scheduler::dag`, `store::deps`): detección de ciclos (pura y con CTE recursivo en SQLite), olas por profundidad, `add_dependency` que rechaza ciclos, autodependencias y proyectos distintos. Un agente `READY` con dependencias sin cumplir pasa a `WAITING_DEPENDENCY` con «espera a T-12 y T-16»; al terminar la última pasa a `READY` y arranca solo si ya tenía modelo exacto o profile (si es «decidir después», espera). Una dependencia que falla o se cancela deja a quien la espera `BLOCKED` con la decisión pendiente (recovery item `DEPENDENCY_FAILED`); también si ya había fallado antes de declararla. Resolver: `ignore`, `replan` (quita la arista; la razón lo deja anotado) o `cancel`.
  - **Milestones:** `OPEN → IN_REVIEW` (o `APPROVED` sin aprobación manual) cuando todas sus tareas están `DONE`; el usuario decide `approve`, `request_changes` o `hold`; con cambios pedidos y todo hecho de nuevo vuelve a revisión.
  - **Plan por fases** (`symphony-scheduler::plan`, `daemon::plan`): formato Markdown (`## Fase N`, `- [ ]/[x]/[!]`, `[id]`, `(needs: …)`, `Verifica:`, notas `> …`) que sirve igual para un lote de agentes (ADR-0013 F3: `waves()`). La primera sesión recibe la tarea más las reglas del plan; al salir el CLI con exit 0 el daemon lee `.symphony/plan/<task_id>.md`: fase pendiente → cierra el run con `PHASE_DONE`/`HANDED_OFF` y abre otro con el mismo modelo y un prompt acotado a la fase (`auto`) o deja al agente `READY` hasta `agent.next_phase` (`ask`); tarea fallida → el agente se detiene (`FAILED`, recovery item) y el plan queda; plan completo → lo hecho y lo que falló pasan a la conversación como mensaje de sistema y el archivo se borra. Una fase con menos de 2 tareas se junta con la siguiente. El archivo se excluye de git de forma local.
  - **Config:** `[plan] enabled, phase_start, min_tasks_per_phase`.
  - **IPC nuevos:** `task.depend`, `task.undepend`, `task.deps`, `task.resolve_dependency`, `milestone.create|list|assign|decide`, `agent.next_phase`; `agent.create` acepta `depends_on`.
  - **fake-agent:** paso `plan` (una fase por sesión) y `SYMPHONY_TASK_ID` en el entorno.
- **Archivos clave:** `crates/scheduler/src/{dag,plan}.rs`, `crates/store/src/deps.rs`, `crates/daemon/src/{dependencies,plan}.rs`, `crates/daemon/src/executor.rs` (`finish_natural`, `start_successor`), `crates/daemon/src/runtime.rs` (`CreateAgent.depends_on`).
- **Cómo se verificó:** `cargo xtask check` → 522 passed. Proptest «un DAG nunca acepta ciclos» (también: toda arista que `creates_cycle` rechaza cerraría un ciclo); store: espera, ciclos, liberación, bloqueo, milestones; L2 con `symphonyd` real y `fake-agent`: B espera a A y arranca al terminar; A falla → B `BLOCKED` → `ignore` lo arranca y `cancel` lo cancela; plan de 2 fases = 2 runs (`PHASE_DONE`, `COMPLETED`), el plan se borra y la conversación guarda el resumen; `ask` deja `READY` hasta confirmar; una tarea `[!]` detiene al agente; sin plan completa como antes; el plan no llega a git.
- **Decisiones que cambié respecto de ADR-0011/0013 (por evidencia o por seguridad):**
  - `/compact` entre tareas **no se implementa**: ADR-0013 D2 pide medirlo antes de dejarlo como default y eso necesita un live que gasta cuota (⛔ Leo). No hay opción de config que no haga nada.
  - La conversación guarda lo que pidió el usuario, no las reglas del plan (van al CLI): así la vista del agente y los handoffs no cargan ~250 tokens de boilerplate.
  - La primera sesión planifica **y** ejecuta la fase 1 (una sesión de solo planificación pagaría dos veces el arranque del CLI, ADR-0010).
- **Pendiente / notas:** (1) Que un CLI real escriba un plan en este formato y se detenga al final de la fase es una instrucción en el prompt: no se probó en L3 (gasta cuota; ⛔ Leo). Si no lo hace, el agente termina como siempre. (2) Un agente `FAILED` cuenta como dependencia fallida aunque se pueda recuperar; al completarse después, quien estaba bloqueado por eso se libera. (3) No hay vista TUI de dependencias ni de milestones todavía (S8).

### P08.S6 · Validation engine y preparación del worktree — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-11
- **Qué se hizo:**
  - **Config** en `project.toml`: `[validation] tier1/tier2/tier3, gate (por defecto 2), max_attempts (3), timeout_secs (900)` y `[worktree] prepare = [...]` (F7, F8).
  - **Motor** (`daemon::validation`): cada comando corre en el worktree del agente por el scheduler (si es pesado espera su turno), con los límites del SO; su salida (redactada) se guarda como objeto de contexto `TEST_OUTPUT` y el resultado en `validation_runs`/`validation_checks` (nombre del check por comando o por tier, conteos de pasadas y falladas, resumen con las líneas que importan). Cada comando queda también en `tool_calls` (`validation:tierN`). Un tier corre solo si el anterior pasó.
  - **Gate**: al salir el CLI bien, el agente pasa a `TESTING` y se valida hasta el tier del gate; si pasa, completa; si falla, el fallo vuelve **al mismo agente** por su sesión (`continue_session`; sin sesión, un run nuevo con el mismo modelo) y la tarea sigue; a los `max_attempts` fallos la tarea queda `BLOCKED` con la decisión pendiente y un recovery item `VALIDATION_FAILED` (migración 005).
  - **Preparación**: los comandos de `[worktree] prepare` corren por el scheduler antes de lanzar al agente; si uno falla no se crea nada (workspace_failed con la cola de la salida).
  - **IPC**: `validation.run {agent, tier}` (a pedido, disparador `USER`) y `validation.get {agent}`.
  - **Core**: enums `ValidationStatus`, `ValidationTrigger`, `CheckStatus`, `CheckName` contra el spec.
- **Archivos clave:** `crates/daemon/src/validation.rs`, `crates/scheduler/src/validation.rs`, `crates/store/src/validation.rs`, `migrations/005_validation.sql`, `crates/core/src/config.rs`.
- **Cómo se verificó:** `cargo xtask check` → 536 passed. L2 con `symphonyd`+`fake-agent`: tier 1 pasa y tier 2 falla → el agente arregla en la sesión retomada → cuatro pasadas (PASS, FAIL, PASS, PASS) y la tarea `DONE`; un check que nunca pasa con `max_attempts = 2` → `BLOCKED` y `VALIDATION_FAILED`; sin `[validation]` todo completa como antes; la validación a pedido para tras el primer fallo; preparación OK (`prepared.txt` en el worktree) y preparación fallida sin agente ni worktree. Unitarios: nombres de check, resúmenes de Rust/Jest/pytest.
- **Pendiente / notas:** (1) Los comandos se ejecutan con `cmd /C` (Windows) o `sh -c` sin escapado propio: comillas anidadas complejas van mejor en un script del proyecto (`npm run …`). (2) El review agent y el merge gate (tiers 3 y milestone) son S7. (3) No hay vista TUI de la validación todavía (S8; las vistas 15–19). (4) Que un CLI real arregle el fallo con el mensaje de validación no se probó en L3 (⛔ Leo).

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| PLAN P08.S1 | «Migración 002» | Es la **004** | 002 (salud) y 003 (contexto) se ejecutaron antes por ADR-0010 |
| DB §3.C `agent_runs.end_reason` | 8 valores | 9: se agrega `PHASE_DONE` | ADR-0013 D2 |

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|
| `symphony-scheduler` (crate propio) | — | Clasificador y, desde S3, cola y slots | Sí (STACK §8, crate `scheduler`) |
| `sysinfo` en el daemon | 0.39.6 (ya en el workspace) | Muestreo de CPU y RAM | Sí |

## Notas para el siguiente agente
- Reconstruir una tabla padre: patrón `defer_foreign_keys` + `_bak` + `DROP` + reinsertar con su nombre (002, 003, 004). No uses `RENAME`.
- Siguiente: P08.S7 (merge, conflictos, review agent y modo delegado del chat).
- En los scripts de Python con cadenas que llevan barras invertidas (`\n` de Rust, TOML) no uses heredocs de la herramienta Bash: se pierden. Escribe el script con la herramienta Write y ejecútalo.
- En un `r#"…"#` de Rust, `"#` termina la cadena: un guion TOML con `"# Plan…` necesita `r##"…"##`.
- Un test que falla y deja un `symphonyd` huérfano cuelga nextest (queda esperando al proceso con fugas): `Get-Process symphonyd* | Stop-Process` antes de reintentar.
- Los ids del daemon se parsean como ULID: en los tests, siembra proyecto, sesión, tareas, agentes y runs con `XxxId::new()`, no con `'a1'`.
