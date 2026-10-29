# Changelog

Cambios de Symphony CLI por versión. Se genera con `git cliff` (config en `cliff.toml`).

## [0.9.0] - 2026-10-27

### Funcionalidades

- [P09.S1] migración 003: chunks con FTS5, items de handoff, retrievals, hechos, skills y MCP
- [P09.S2] direcciones ctx:// con protección contra traversal, chunks con FTS5 y objetos indexados
- [P09.S3] compresores deterministas (log, tests, JSON, dedup, diff) que nunca pierden el original ni inflan
- [P09.S6] handoff v2 con items y fidelidad, diff resumido en balanced/aggressive y symphony context inspect|stats|raw; fix: la poda de checkpoints borra los chunks de los diffs
- [P09.S4] AST L0–L5 con tree-sitter (Rust, Python, JS, TS) y esqueleto L3 en el handoff para archivos nuevos que no caben
- [P09.S5] file watcher por proyecto con notify e ignore (debounce 300ms) e indexación ctx://file/
- [P09.S7] consolidacion de hechos (project_facts), symphony fact set/list e inclusion en el handoff
- [P09.S8] métodos IPC context.search, context.retrieve y context.lines con registro de retrievals
- [P09.S8] broker MCP del contexto (symphony mcp serve, rmcp) registrado por invocación en Claude y Codex
- [P09.S9] skills canónicas y servidores MCP compartidos (mcp.toml) con índice, interruptores y traducción por invocación
- [P09.S9] vistas 20, 21, 32, 33 y 35 de la TUI (contexto, skills, MCP, uso) con snapshots
- [P11.S7] OpenCode como proveedor (ADR-0012): adapter, ids de modelo, permiso de worktree, broker MCP y live gratuito
- [P10.fix] un run que muere tras un límite se cierra con esa causa (F5) y resume reabre un agente FAILED (F6)
- [P08.S1] migración 004 (Fase 2) y PHASE_DONE
- [P08.S2] clasificador de operaciones y Resource Manager
- [P08.S3] scheduler con slots pesados, retención por hook y tope por cuota
- [P08.S4] límites del SO por agente y tabla processes
- [P08.S5] DAG de tareas, milestones y plan por fases
- [P08.S6] motor de validación, gate de la tarea y preparación del worktree
- [P08.S7] integración a la rama del chat, conflictos, review agent y lotes
- [P08.S8] vistas TUI de Fase 2 y System Strip real
- [P11.5.S1-S2] ctx:// con @ en la ruta y permisos de proveedores en config
- [P11.5.S3] respuesta del chat construyéndose (deltas de texto)
- [P11.5.S4] el router recibe el tamaño del handoff y se avisa del checkpoint obsoleto
- [P11.5.S5] el tope de simultáneos de un lote sigue a la cuota de ahora
- [P11.5.S6] integrar el chat en main con review agent y confirmación
- [P11.5.S7] pulido de la TUI: milestone nuevo, enviar el fallo, avisos que caducan
- [P12.S1-S2] crate symphony-plugins: manifiesto, protocolo v1 y host de procesos aislados (ADR-0015)
- [P12.S2-S3] plugins en el gate y el handoff, CLI y plugin de ejemplo
- [P14.S1-S4] sugerencias de modelo por reglas y desempeño aprendido
- [P15.S1] puente de la GUI: cliente IPC sin lógica de negocio
- [P15.S2-S3] GUI de escritorio: scaffold Tauri 2 + Solid y las 36 vistas
- [P15.S4-S5] datos de ejemplo para la GUI en el navegador y job de bundles en CI
- [P15.5.S1] modelos actualizados de Codex, Antigravity y Kimi
- [P15.5.S2] acceso local total por defecto en los proveedores
- [P15.5.S3] un daemon y un estado por proyecto (ADR-0018)
- [P15.5.S3] aprendizaje compartido y symphony migrate split
- [P15.5.S4] harness de Symphony: las mismas herramientas para todos los modelos
- [P15.5.S5] symphony para documentos, multimedia y lo que sea (ADR-0020)
- [P13.S0] harness de Symphony por defecto y modelos Claude/GPT-OSS en Antigravity
- [P13.S6] cargo xtask er genera el ER desde las migraciones
- [P13.S4] cargo xtask metrics exporta métricas por run
- [P13.S4] retención de events y resource_samples al arrancar el daemon
- [P13.S3] checkpoint corrupto: se marca, se avisa y el handoff usa el anterior
- [P13.S4] completar harness E2E de benchmark (STACK 25.2)

### Correcciones

- [P09.S11] los tests e2e del CLI no dependen de un CLI real instalado (claude falso en el PATH)
- [P09.S11] correcciones de la revisión de código (redacción en el broker, raw por proyecto, prune seguro, plugin por contenido, fidelidad, NEEDED_RETRIEVAL, watcher, constantes AST)
- [P11.5.S10] el reactor de dependencias se recupera de un Lagged; deuda cerrada
- [P11.5.S9] quita los archivos que escribió un modelo fuera de su worktree y guarda el repo en los live
- [P12.S4] revisión de código de P12 y cierre de la fase
- [P14.S5] revisión de código de P14 y cierre de la fase
- [P15.S6] revisión de código de P15 y cierre de la fase
- [P15.5.S6] revisión de código de P15.5 y carrera del cupo de lotes
- [P13.S1] tokio 1.54.1 por RUSTSEC-2026-0386
- [P13.S4] Symphony ligero en disco: worktrees, target compartido e índice
- [P13.S1] harness acotado en memoria, Kimi en UTF-8 y causa de las caídas
- [P13.S3] el watchdog cuenta la CPU del árbol como latido
- [P13.S3] el latido por CPU compara contra la muestra anterior
- [P13] un modelo sin créditos no es un 429 transitorio; tabla IPC
- [P13] errores de cuota y de plan que se veían como caídas; menos espera de Cargo
- [P13.S4] los agentes comparten el build-dir de Cargo, no el target/
- [P13.S2] redact idempotente con un valor ya redactado pegado a texto
- [P13] el watcher indexa lo que hay en una carpeta nueva o movida
- [P13.S5] merge que no se abre con config ilegible y scheduler con una sola identidad por agente
- [P13.S1] zstd 0.14.3, zstd-safe 8.2.0 y zstd-sys 2.1.2 por RUSTSEC-2026-0391/0398/0403/0410
- [P13.S3] el daemon arranca aunque no pueda listar agentes para validar checkpoints
- [P13.S2] redact se repite hasta que no cambia
- [P13.S5] executor: no se cuelga sin poder publicar y no mata agentes pausados
- [P13.S5] store: eventos descartados quedan en el log y los mensajes tienen tope
- [P13.S5] juntar fases del plan no pierde comandos Verifica
- [P13.S5] harness: escritura atómica y read/grep/glob/shell acotados
- [P13.S5] motor de contexto: presupuesto de archivos nuevos, cálculo fuera del writer y watcher acotado
- [P13.S8] revisión de código: el GC no borra archivos nuevos sin commitear
- [P13.S5] soporte de error no reconocido en result de Claude con TurnFinished
- [P13.S5] uso de has_token en Claude y Codex para evitar falsos positivos de errores
- [P13.S5] priorizar límites transitorios sobre cuota en kimi, antigravity, copilot y opencode

### Refactor

- [P13.S5] ponytail-audit: una sola now_ms en el daemon y fuera fake_agent_path

### Pruebas

- [P09.S10] evaluación del handoff por modo con forced kill; docs: bitácora, STATUS, SESSIONS y por qué tardó la sesión (LEARNINGS)
- [P09.S11] el test del cambio de proveedor del chat no depende del orden de los ids (falló en macOS)
- [P08.S9] benchmark principal (L2 con fake-agents y L3 con los seis proveedores)
- [P11.5.S8-S9] mediciones y pruebas live con OpenCode gratuito; symphony review
- La evaluación de handoff tolera ±64 tokens entre RAW y SAFE (flake del CI de Windows)
- [P13.S2] targets de fuzzing y propiedades proptest
- [P13.S3] prueba de larga duración con reinicios
- [P13.S5] el límite de 5 h de Kimi es cuota aunque traiga 429

### Build y CI

- [P13.S5] typos y lychee en CI
- [P13.S5] el job docs no bloquea hasta revisar su primera corrida
- [P13.S5] job de cobertura con cargo-llvm-cov
- [P13.S2] job de fuzzing corto y corpus semilla
- [P13.S7] pipeline de release con cargo-dist en dry run
- [P13.S5] typos con las palabras en español del repo
- [P13.S5] docs bloqueante: lychee excluye los prompts grabados del spike
- [P13.S5] typos: condicional
- [P13.S5] typos: secretas
## [0.5.0] - 2026-10-02

### Funcionalidades

- *(daemon)* [P07.5.S1] continuar la sesión del CLI con un mensaje tras el turno
- *(context)* [P07.5.S3] el handoff lleva la conversación, recortada por modo
- *(daemon)* [P07.5.S4] agente «chat general» idempotente que espera mensaje tras el turno
- *(daemon)* [P07.5.S5] commit por turno del chat y cambios de estado en el bus
- *(daemon)* [P07.5.S6] cambio de proveedor del chat con mensaje, failover y umbral de tokens opcional
- *(tui)* [P07.5.S7] vista Chat como inicio, chat.get, FLOW y Journey F
- *(tui)* [P07.5.S7b] proceso del agente en vivo en el chat (herramientas intercaladas y «pensando… N s»)
- *(tui)* [P07.5.S7b] copiar la respuesta (Ctrl+Y), pegado multilínea y modo selección (F2)
- *(daemon,tui)* [P07.5.S7b] exportar la conversación completa a .md (agent.export, Ctrl+E / e)
- *(daemon)* [P07.5.S8] las skills funcionan en el chat, también tras un cambio de proveedor
- *(daemon,tui)* [P07.5.S9] autocompletado de skills en el chat y selección de texto con el ratón
- [P11.S2] adapter de Kimi Code y ADR-0009 propuesto (stderr al adapter)
- [P11.S2] el executor entrega stderr al adapter (ADR-0009) y Kimi retoma su sesión
- [P11.S3] adapter de Antigravity (agy) y live L3 de Kimi y Antigravity
- [P11.S4] adapter de Copilot CLI; detección de proveedores en paralelo
- [P10.S1] migración 002: salud, uso y routing (con las FKs pendientes de 001)
- [P10.S2] máquina de estados de salud, uso y cuota conectados a los runs
- [P10.S3] cuota con certeza, estimación opcional, vista de proveedores con salud y symphony usage
- [P10.S4] crate symphony-router: filtro de disponibilidad, puntaje por profile y explicación
- [P10.S5] el router decide el failover y los profiles; Model Picker y Explain Route

### Correcciones

- *(tui)* Don't parse a leading path as a model in :spawn [P07.S9]
- *(tui)* [P07.5.S7] modelo al reabrir el chat, tope del scroll y cola visible del mensaje
- *(tui)* [P07.5.S7] la conversación del agente ajusta el texto en vez de cortarlo
- *(tui)* [P07.5.S7b] scroll de la conversación del agente (↑/↓ invertidos, sin tope ni rueda)
- *(deps)* Actualiza yoke-derive a 0.8.4 (la 0.8.3 fue retirada de crates.io y rompía cargo deny)
- *(git)* Serializa el commit frente a las lecturas del índice del mismo worktree
- *(daemon)* Un CLI que sale antes de leer su prompt ya no cuenta como fallo de arranque
- [P11.S6] correcciones de la revisión de código (stderr estricto, pinned_bin, errores de Antigravity) y docs
- [P10.S6] correcciones de la revisión de código; docs de profiles, salud y configuración

### Pruebas

- *(cli)* Fake claude on PATH so agents lifecycle test runs without real CLIs [P07.S9]
- *(context)* [P07.5.S9] tabla de coste en tokens del handoff por modo y largo de conversación
- *(tui)* [P07.5.S9] medición live del coste real de los cambios de proveedor del chat
- [P11.S5] matriz de handoff entre proveedores (L2 y L3) y copia fija de binarios en los tests
## [0.1.0] - 2026-09-25

### Funcionalidades

- *(protocol)* Length-framed versioned JSON IPC codec [P02.S1]
- *(core)* ULID ids, DB enums and pure state transitions [P02.S2]
- *(core)* Global and project config with comment-preserving edits [P02.S3]
- *(core,daemon)* Central secret redactor and redacting rolling logs [P02.S4]
- *(daemon)* Single-instance symphonyd with user-only IPC [P02.S5]
- *(cli)* Symphony status and daemon start|stop|status with autostart [P02.S6]
- *(store)* Migration 001 with the 19 phase-1 tables [P03.S1]
- *(store)* Single writer thread with batched events and read-only readers [P03.S2]
- *(store)* Hand-written repositories for the phase-1 lifecycle core [P03.S3]
- *(object-store)* Content-addressed blobs with BLAKE3, zstd and atomic writes [P03.S4]
- *(daemon)* Open the store and recover interrupted sessions at startup [P03.S5]
- *(git)* Git CLI wrapper for worktrees, porcelain status, diff and merge preflight [P04.S1]
- *(git)* Per-worktree dependency strategy; never delete through links [P04.S2]
- *(process)* Contained process groups with streaming, stdin, stats and tree kill [P04.S3]
- *(core)* ANSI sanitizer for CLI output (plain and color-only modes) [P04.S4]
- *(testkit)* Fake-agent provider CLI simulator driven by TOML scripts [P04.S5]
- *(adapters)* ProviderAdapter contract, canonical events and fake adapter [P05.S1]
- *(daemon)* Event bus with persistence, broadcast and per-agent state [P05.S2]
- *(cli,daemon)* Symphony hook emit feeds CLI hooks into the event bus [P05.S3]
- *(adapters)* Claude Code adapter with per-invocation hooks and stream-json IO [P05.S4]
- *(adapters)* Codex adapter with -c hook overrides and rollout quota [P05.S5]
- *(daemon,cli)* Provider registry and symphony providers [P05.S6]
- *(daemon)* Agent creation flow with rollback and executor launch [P06.S1]
- *(daemon)* Mirror conversation and tool calls from canonical events [P06.S2]
- *(daemon)* Incremental checkpoints from significant events [P06.S3]
- *(context)* Handoff assembler v1 from checkpoint + live git [P06.S4]
- *(daemon)* Replace executors without losing agent state [P06.S5]
- *(daemon)* Heartbeat watchdog with reclaim and restart [P06.S6]
- *(cli)* Agent control subcommands over daemon IPC [P06.S7]
- *(daemon)* Event subscription and read methods for the TUI [P07.S1]
- *(tui)* Terminal UI over daemon IPC with views 01-09, 12, 13, 22, 24, 30 [P07.S6]
- *(attach)* Open the agent's session in the official CLI [P07.S5]

### Correcciones

- *(protocol)* Skip socket fchmod on macOS, rely on the private 0700 dir [P02.S6]
- *(cli)* Wait for the instance lock, not the socket, when stopping or autostarting [P03.S5]
- *(cli)* Report a daemon that dies mid-call as stopped once its lock is free [P03.S5]
- *(process)* Count the whole tree in stats when the OS group does not [P04.S3]
- *(object-store)* Grace-period comparisons are inclusive [P04.S3]
- *(protocol)* Only talk to the daemon that owns this home [P05.S7]
- *(adapters)* Hook security review fixes [P05.S7]
- *(protocol)* Verify the peer uid when the OS gives no pid (macOS) [P05.S7]
- *(daemon)* An unrequested signal death is a crash, not a kill [P06.S1]
- *(runtime)* Claude turns end, and slow CLIs are not taken for hung [P07.S7]
- *(windows)* Daemon console, long paths and uncancellable agent creation [P07.S7]

### Rendimiento

- *(store)* Criterion baseline for event inserts and the Home query [P03.S6]

### Pruebas

- *(spike)* Test A resource measurements for Claude Code and Codex [P01.S3]
- *(spike)* Test B hook event bus for Claude Code and Codex [P01.S4]
- *(spike)* Test C command holding via PreToolUse [P01.S5]
- *(spike)* ProcessKit gate spike and 3-OS workflow [P01.S7]
- *(spike)* ProcessKit gate reports unsupported limits instead of aborting [P01.S7]
- *(spike)* Test D forced handoff, 6/6 pass [P01.S6]
- *(cli)* E2E daemon restart after a hard kill [P02.S7]
- *(testkit)* Worktree + fake-agent + stream + tree kill + cleanup [P04.S6]
- *(cli)* Live L3 sessions for Claude Code and Codex, gated by SYMPHONY_LIVE [P05.S7]
- *(daemon)* Forced kill acceptance test (Test D) [P06.S8]
- *(tui)* Journey A live with Claude Code, and agent view fixes [P07.S7]

### Build y CI

- Cargo workspace skeleton with xtask check [P00.S6]
- GitHub Actions matrix, MSRV and cargo-deny jobs [P00.S8]
- *(deny)* Allow 0BSD license used by interprocess dependencies [P02.S5]
