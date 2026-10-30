# P17 · TUI al día

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p17-tui (sale de `main` el 2026-10-28, después del hotfix del CI) |
| Inicio / cierre | 2026-10-28 / — |
| Agentes que trabajaron | opencode-go (delegación desde Symphony) |
| Tag | — |
| Docs usados | PLAN § P17, `docs/research/backlog-2026-10-27.md` § Interfaz |

## Pasos

### P17.S8 · Marcadores de color del chat — ✅ (2026-10-29)
- **Agente:** codex/gpt-6.
- **Implementación:** parser puro en `crates/tui/src/markers.rs` para los seis tipos del ADR-0023. Aperturas desconocidas, cierres incorrectos y anidamientos permanecen literales. Render compartido por chat, conversación del agente y streaming; colores del tema (brand/ok/accent/warn/error/muted), sin dependencias nuevas y respetando NO_COLOR. Texto no marcado conserva el estilo normal de la terminal, igual que antes.
- **Persistencia:** revisión de `recorder::message` y `repo::insert_message`: guardan el original inline o como blob, sin eliminar delimitadores. Prueba de reapertura con dos App nuevas y la misma respuesta IPC: transcript intacto y colores reconstruidos. No se modifica store ni recorder.
- **Pruebas:** casos puros de Unicode, vacíos, multilineales, tipos desconocidos, cierres incompletos/incorrectos y anidamientos; buffer con los seis colores y NO_COLOR; snapshots a 80 y 160 columnas revisados.
- **Verificación:** nextest de TUI y clippy `-D warnings` en verde. La primera ejecución excedió el límite de 120 s de la herramienta; el reintento esperó el candado y terminó. Por error se ejecutó también el binario de integración `symphony-tui::daemon` (pasó), contrario a la instrucción; la comprobación final lo excluye explícitamente: 146 tests pasaron. SYMPHONY_LIVE eliminado antes de Cargo; sin xtask ni suscripciones reales.
- **Pendiente:** integración y gate de fase a cargo del integrador.


### P17.S5 · Herramientas shell observadas por el daemon — ✅ (2026-10-29)
- **Decisión:** el daemon no ejecuta el shell del agente; el CLI proveedor ejecuta las herramientas. No existe un proceso shell administrado que Symphony pueda interceptar. Esta parte construye una vista derivada de los eventos `ToolRequested`/`ToolFinished` y salida reportada por el proveedor, persistidos vía `tool_calls` y `context_objects` (`TOOL_OUTPUT`), sin tabla nueva. IPC: `agent.processes` es una vista derivada de los `tool_calls` shell (no una tabla de procesos), y `agent.process.output` es una suscripción de **solo lectura** por agente, **sin stdin** ni shell interactiva.
- **Alcance:** agregado `AgentEvent::ToolOutput`, parseo común de `tool_response`, almacenamiento redactado en object store, lista `agent.processes` derivada de tool calls shell, `Subscribe` versionado con `agent_id` opcional y tópicos `agent.processes` / `agent.process.output`; fake-agent L2 valida adapter → bus → recorder → vista y salida en vivo por bus. No hay stdin ni shell interactiva.
- **Fuera de alcance:** interceptar el shell del daemon (P15.5.S4 pendiente) no aplica porque el CLI proveedor es quien lo ejecuta. No se crean procesos gestionados ni filas nuevas.
- **Pendiente (seguimiento):** test de socket IPC end-to-end donde el fake-agent del test sea el proveedor configurado del daemon; revisar parsers de proveedores que reportan resultados fuera de hooks para emitir `ToolOutput` cuando esté disponible.
- **Verificación:** clippy `-D warnings` de los cuatro crates (protocol, daemon, testkit, adapter-common) en verde (let-else con `?` en `recorder.rs`; contract suite del fake_adapter ajustado a `ToolOutput`). `cargo nextest run` de los cuatro crates excluyendo el binario `daemon` (flaky en Windows): 201/204 en la corrida completa y los 3 que fallaron por carga (rotan) pasaron al reintentarlos sueltos; el test L2 `shell_tool_output_is_listed_and_streamed_for_its_agent` pasa por filtro de nombre.
- **Nota de entorno:** al correr los tests **dentro** del harness de Symphony hay que quitar `SYMPHONY_USER_HOME` del ambiente: si está heredada, el daemon de los tests descubre plugins en el home real del usuario y los tests de plugins fallan deterministas («`todo-check` no está instalado»). Ver `docs/progress/LEARNINGS.md`.
- **Estado:** cerrado; pendientes de seguimiento anotados arriba.

### P17.S1 · Tema y barra de estado — ✅ (`30c0bfe`, `98283c9`)
- **Objetivo:** paleta morada apagada en un módulo de tema, sin colores sueltos por `ui.rs`; barra
  de estado siempre visible con modelo:proveedor, uso de la sesión de 5 h y semanal con hora de
  reinicio, tokens de sesión y de contexto, carpeta, rama y modo (plan/auto/loop/yolo).
- **Verificación:** snapshots `insta` a 80 y 160 columnas; `cargo nextest run -p symphony-tui`.
- **Pendiente / notas:** los campos que aún no tienen dato en `App` se muestran como «—» y se
  anotan aquí; no se añade IPC nuevo en este paso.

#### S1a · Tema — ✅ (`30c0bfe`)
- **Agente:** opencode-go/beam-free (delegado) + revisión e integración del orquestador.
- **Qué se hizo:** `crates/tui/src/theme.rs` con `Theme` (`brand`, `accent`, `ok`, `warn`, `error`,
  `muted`, `border`, `text`, todos `Option<Color>`) y `Theme::no_color()` / `from_env()` (lee
  `NO_COLOR`). `App.theme` se inicializa con `from_env()` y `render()` lo publica en un
  `thread_local` mientras dibuja, así que `ui.rs` no arrastra el tema por todas las firmas.
  Los ~30 colores sueltos pasan a roles del tema; los 3 `Color::Magenta` que quedaron se
  cambiaron a `accent` en la revisión. Solo sobreviven `Color::White` y `Color::Reset` en el
  diff, que no son paleta sino contraste de los marcadores `+++`/`---`.
- **Pruebas:** los 33 snapshots de texto **no cambian** (solo cambia el color) y por eso no
  probaban nada del tema; se añadieron tres pruebas que miran el buffer crudo del `TestBackend`:
  la cabecera sale con el morado de marca, ninguna celda se sale de la paleta y con `NO_COLOR`
  no se pinta ninguna celda. Más snapshots del inicio a 80 y 160 columnas.
- **Verificado:** `cargo nextest run -p symphony-tui` → 88 ✅; `clippy --all-targets -D warnings` ✅;
  `cargo fmt --all -- --check` ✅.

#### S1b · Barra de estado — ✅ (`98283c9`)
- **Agente:** opencode-go/beam-free (delegado) + revisión, pruebas y corrección del proveedor del
  orquestador.
- **Qué se hizo:** `render()` pasa a cuatro trozos (cabecera 1, cuerpo min 1, **barra 1**, pistas
  `bottom`) y la barra se pinta fuera del `match` de pantallas, así que sale en todas. `status_line`
  construye campos con etiqueta (`modelo`, `cuota`, `rama`, `tokens`, `modo`) y la carpeta sin
  etiqueta, separados por `  |  `, con los colores del tema (`brand` para el modelo, `muted` para
  el resto).
- **Responsive:** a partir de 110 columnas caben los seis; de 80 a 109 se caen carpeta, `tokens` y
  `modo`; por debajo de 80 solo quedan modelo y cuota y el modelo se recorta con «…» si no cabe. La
  caída es de derecha a izquierda por prioridad, así que la línea nunca se parte. El «reinicia en …»
  solo se pinta desde 100 columnas (en 80 se comía el resto).
- **Corrección en la revisión:** el delegado sacaba el proveedor del prefijo del id (`claude/opus`
  → `claude`), y ese no es el id de ningún proveedor, así que la cuota salía siempre «—». Ahora manda
  el `provider_id` del catálogo (`anthropic`), luego el `current_run.provider_id` del `inspect` y,
  solo como último recurso, el prefijo. Por eso `view_00_chat_empty` y `view_13_model_picker` pasan
  de «cuota —» a «cuota 15 %»: es la salud real del fixture.
- **Campos a «—» (a propósito):** `tokens` no tiene dato de sesión en `App` (solo uso de 7 días por
  proveedor), y `modo` (plan/auto/loop/yolo) todavía no existe como estado. Este paso no añade IPC;
  quedan anotados para P17.S2/S3, cuando haya de dónde leerlos.
- **Pruebas:** seis nuevas en `views.rs`: los seis campos a 160, la caída a 90, 60 sin `rama`, el
  recorte con «…» a 30, la barra en Launch/Chat/Home/Stats y el caso sin datos (todo «—» y sin
  romper a 40). Los 36 snapshots cambian solo en la línea nueva.
- **Verificado:** `cargo nextest run -p symphony-tui` → 94 ✅; `clippy --all-targets -D warnings` ✅;
  `cargo fmt --all -- --check` ✅.

### P17.S3 · Arranque — ✅ (`388f532`, `af30a90`)
- **Objetivo:** arte ASCII de sinfonía a pantalla completa, «para empezar» en proyectos nuevos, recuadro de versión/anuncios (solo local) y `?` abre atajos.
- **S3a (`388f532`):** `LAUNCH_ART` (8 líneas, 17 columnas) en `ui.rs`, centrado con `current_theme().brand`; degrada a la palabra `SYMPHONY` cuando el cuerpo tiene menos de 66 columnas o 20 filas. Debajo de las opciones `[i]/[o]/[q]` se pinta `PARA EMPEZAR` con las teclas reales (`i`, `o`, `:`, `q`) y su explicación. No se inventaron comandos no implementados (`/init`, `/usage`, `/status`, `/permissions`, `/harness`).
- **S3b (`af30a90`):** `?` abre `Screen::Help` (recuerda la pantalla de origen para volver con `Esc` o `q`) en todas las pantallas salvo el agente (donde `?` ya explica el routing), `FirstRun`/`NewAgent`/`ProviderSetup` (entrada de texto) y la barra de comandos. El recuadro muestra la versión del crate (`env!("CARGO_PKG_VERSION")`) y, si existe, `.symphony/anuncios.md` (hasta 8 líneas), leído una sola vez al arrancar y sin red ni telemetría.
- **Pruebas:** dos nuevas en `views.rs` (el arte cabe y se oculta en un terminal pequeño; el bloque «PARA EMPEZAR» sale en el proyecto no inicializado y no en el inicializado). Los snapshots `view_01_launch_*` solo cambian en el arte y el bloque nuevo.
- **Verificado:** `cargo nextest run -p symphony-tui` → 103 ✅; `clippy --all-targets -D warnings` ✅; `fmt` ✅.

### P17.S4 · Mascota — ✅
- **Objetivo:** mini piano de una octava, con 7 teclas blancas, 5 negras y frases rotativas; solo en el chat principal mientras el agente trabaja.
- **Qué se hizo:** se integró la mascota en `crates/tui/src/mascot.rs`, usando el tick existente de 1 s. Respeta `SYMPHONY_NO_ANIM`, `NO_COLOR`, `TERM=dumb` y `[ui].animations`; se añadieron snapshots a 80 y 160 columnas y pruebas de aislamiento del chat.
- **Corrección:** `home_app()` no respondía el nuevo `config.ui`, dejando `inflight` ocupado y haciendo que `bus_events_refresh_on_the_next_tick_without_piling_up` devolviera cero llamadas. El test ahora responde la llamada inicial; no se relajó la aserción porque el comportamiento correcto sigue siendo refrescar una vez en el siguiente tick.
- **Verificado:** `unset SYMPHONY_LIVE; cargo nextest run -p symphony-core` → 73 ✅; `unset SYMPHONY_LIVE; cargo nextest run -p symphony-tui --test views` → 101 ✅; `unset SYMPHONY_LIVE; cargo clippy -p symphony-core -p symphony-tui --all-targets -- -D warnings` ✅. No se ejecutaron de nuevo los tests del binario `daemon`.
- **Objetivo:** streaming de la respuesta, pensamientos, ediciones con `+N −M` y archivos tocados.
- **Verificación:** test L2 con `fake-agent` que emite deltas y diffs.
- **Agente:** opencode-go/beam-free (delegado) + revisión del orquestador.
- **Streaming: no había que hacerlo.** Los adaptadores ya emiten `AssistantDelta` desde P11.5.S3,
  el bus lo guarda en memoria (`Bus::partial`, tope 64 KiB) y `agent.logs` devuelve `partial`, que
  la TUI pinta con el cursor ▍ (`partial_lines`). Lo que faltaba era la prueba y la resolución: lo
  que se percibe como «no streaming» es el tick de 1 s. La decisión y la opción 2 (publicar el
  `AgentEvent` en `agent.event`) quedan en ADR-0022.
- **Pensamientos: fuera de este paso.** Exigirían `AgentEvent::Thinking` y migrar el `CHECK` de
  `events.type`; al backlog (ADR-0022).
- **Ediciones:** `FileStat { path, added, removed }` y `pub fn diff_stats(&str) -> Vec<FileStat>`
  en `app.rs`, función pura sobre el texto del diff. `ui::changes` pinta el título
  «Cambios · N archivo(s) +A −R» (menos U+2212) y una línea por archivo con la ruta, `+A` en `ok` y
  `−R` en `error`. Sin stats el título sigue siendo «Cambios» y con el diff vacío se mantiene
  «Sin cambios contra la base.».
- **Corrección en la revisión:** la primera versión tomaba `--- `/`+++ ` como cabecera por el
  prefijo, así que una línea de contenido que empieza por `-- ` (comentario SQL) se perdía, y las
  rutas entrecomilladas por git (`"b/caf\303\251"`) no se desescapaban. Ahora el parser lleva estado
  de hunk: `diff --git` abre archivo, `@@` entra en el hunk y las cabeceras `---`/`+++` solo lo son
  antes del primer `@@`; después cuentan como contenido. Se añaden `unquote` (escapes C y octal),
  el nombre en trozos sin `diff --git` (del `+++`, o del `---` en un borrado a `/dev/null`), los
  binarios como 0/0 y la suma de bloques repetidos del mismo path. El alta se centraliza en
  `record`.
- **Pruebas:** cuatro nuevas: dos archivos con cabeceras que no cuentan, contenido que parece
  cabecera dentro de un hunk, trozo sin `diff --git` y borrado, y rutas entrecomilladas + bloques
  repetidos + binario; más el resumen en la vista, el caso sin diff y el streaming (con
  `partial: "Voy a"` se ve «Voy a▍»; al llegar el ASSISTANT completo el cursor desaparece y el
  texto sale una sola vez).
- **Snapshot:** solo cambia `view_09_agent_changes`, y solo el título y la línea de resumen (el
  diff se desplaza dos líneas por el viewport fijo; el resto del frame es idéntico).
- **Verificado:** `cargo nextest run -p symphony-tui` → 101 ✅; `clippy --all-targets -D warnings` ✅;
  `cargo fmt --all -- --check` ✅.

### P17.S5 · Procesos en la vista del agente (TUI) — ✅ (2026-10-29)
- **Qué se hizo:** módulo `crates/tui/src/processes.rs` con lógica pura para listar comando, estado e inicio; nueva pestaña `Procesos`; llamada `agent.processes`; suscripción de solo lectura a `agent.process.output` y acumulación de salida en vivo, sin stdin ni shell interactiva.
- **Pruebas:** 114 tests de `symphony-tui`, incluidos los tests puros del módulo; snapshots actualizados para 80 y 160 columnas. Clippy `-D warnings` en verde. No se ejecutaron tests del binario `symphony-daemon`.
- **Archivos:** `app.rs`, `ui.rs`, `io.rs`, `lib.rs`, `processes.rs`, tests y snapshots.

### P17.S6 · IPC global y ramas de Git (daemon) — ✅
- **Agente:** codex/gpt-6.1.
- **Qué se hizo:** `usage.global` lee el registro `~/.symphony/symphony.db` y abre cada DB de proyecto en `SQLITE_OPEN_READ_ONLY`; agrega tokens del mes por proveedor/modelo y por fecha (`YYYY-MM-DD`). Proyectos sin DB o con esquema incompleto aparecen en `incomplete_projects`. `git.branches` devuelve ramas locales con current, ahead/behind respecto al upstream y si están integradas en HEAD; `git.checkout` cambia la rama; `git.delete_branch` nunca fuerza y devuelve `branch_not_merged` si no está integrada.
- **Contrato:** structs serde públicos en `symphony-protocol`, bajo `PROTOCOL_VERSION` existente; no cambia la versión wire.
- **Pruebas:** `cargo nextest run -p symphony-git branch_status_checkout_and_safe_delete` y `cargo nextest run -p symphony-daemon --lib usage_global_reads_and_aggregates_registered_project_databases` ✅. `cargo clippy -p symphony-daemon -- -D warnings` ✅. No se ejecutó el binario `daemon` ni `cargo xtask check`, como pidió la tarea.
- **Límite conocido:** la lista de proyectos disponible en el home es la tabla `projects` del respaldo `symphony.db` dejado por `migrate split`; si se elimina ese respaldo no existe aún otro registro global que permita descubrir proyectos automáticamente.

### P17.S6 · Vistas TUI: avance, ramas y status global — ✅ (2026-10-29)
- **Agente:** kimi-cli/k2.
- **Qué se hizo:** tres vistas nuevas detrás de los IPC ya integrados (`usage.global`, `git.branches`, `git.checkout`, `git.delete_branch`):
  - **Avance (36):** % del plan (tareas DONE/total), fases (milestones) con su barra y agentes con rama, tarea y estado (`a` desde Inicio o `:avance`). Rama por agente: `agent.list` ahora devuelve `branch` (LEFT JOIN con `worktrees` en `list_agents_rows`, store + daemon).
  - **Ramas (37):** lista con estado (actual/integrada/sin integrar) y divergencia con el upstream; Enter cambia **sin confirmación**; `d` borra **siempre con confirmación** y, si git responde `branch_not_merged`, muestra el aviso y exige una segunda confirmación que fuerza (`b` desde Inicio o `:branches`).
  - **Status global (38):** uso del mes por modelo/proveedor entre proyectos (solo lectura, ADR-0022) y heatmap mensual estilo GitHub: días × semanas (lunes primero, Sakamoto sin dependencias) con intensidad por tokens relativa al pico del mes (`g` desde Inicio o `:global`).
- **Módulos nuevos:** `crates/tui/src/plan.rs` (%, barra, fases), `crates/tui/src/branches.rs` (etiqueta, divergencia y máquina de confirmaciones `delete_key`), `crates/tui/src/heatmap.rs` (rejilla e intensidad). Enganches mínimos: `app.rs` (3 pantallas, 4 `Req`, teclas y barra de comandos), `ui.rs` (render con el tema), `io.rs` sin cambios (el `Call` ya es genérico).
- **Extensión de IPC (necesaria y mínima):** `GitBranchRequest` gana `force: bool` (`#[serde(default)]`, sin cambio de versión wire) y el daemon lo pasa a `repo.delete_branch`; sin él la «segunda confirmación» no podía forzar el borrado nunca. `agent.list` gana `branch`.
- **Pruebas:** tests puros de los tres módulos (rejilla de octubre 2026, niveles, transiciones de `delete_key`, % y fases) y en `views.rs`: snapshots a 80 y 160 columnas de las tres vistas + interacción (checkout sin confirmar, la actual no se borra, borrar pregunta, `branch_not_merged` exige la segunda confirmación con `force: true`, otra tecla caduca la confirmación). Los snapshots de Inicio solo cambian en la línea de atajos nuevos.
- **Verificado:** `cargo nextest run -p symphony-tui` → 143 ✅; `-p symphony-store` → 73 ✅; `-p symphony-daemon --lib` + `-p symphony-protocol` → 54 ✅; `cargo clippy -p symphony-tui -p symphony-store -p symphony-daemon -p symphony-protocol -- -D warnings` ✅; `cargo fmt --all -- --check` ✅. No se corrió el binario `daemon` ni `cargo xtask check`.
- **Pendiente / notas:** el % del plan es de tareas (DONE/total); el avance por fases del `.symphony/plan/*.md` de cada agente no se expone por IPC y queda para el planeador (P19). La línea de agente en Avance se corta visualmente a 80 columnas (el párrafo envuelve, como en el resto de vistas).

## Backlog / ideas futuras de P17 (documentadas, no implementadas)

- **S8 · Renderizado rico del chat:** resuelto con la opción B del ADR-0023; ver el paso cerrado arriba.
- **Política configurable de Symphony/harness (pendiente, fuera de P17):** análogo a los permisos de Claude Code (allow/deny/ask, modos default/acceptEdits/plan/dontAsk/bypassPermissions, archivos ignorados estilo .claudeignore, política por proyecto en .symphony/policy.toml). Requiere ADR nuevo y decisión de alcance (solo archivos vs. también shell/git). Documentado en PLAN.md bajo "Ideas futuras (sin fase)".



- **Ideas futuras de Leo (2026-10-29), documentadas en PLAN.md:** (1) versión cooperativa en línea de Symphony (invitar compañeros/amigos, cada uno suma proveedores/cuentas/modelos/tokens; requiere ADR de transporte, auth, aislamiento y cuotas — mucho más allá de P20.S4). (2) Context compaction con Jev o equivalente (similar a fast-jev-compaction pero preservando más contenido importante: decisiones, constraints, archivos tocados) — integrable en P18.S3 (/compact) o P18.S4; requiere el trait DecisionModel de P19.S1. (3) Documentar cuándo usar git merge vs git rebase (guía en docs/book o skill; no es código). (4) Ingeniería de grafos en los agentes (línea de producción: nodos = etapas/tareas, aristas = dependencias/handoffs, con reintentos, paralelismo y checkpoint por nodo) — complementa el planeador (P19.S3) y la vista de avance (P17.S6); requiere ADR de representación y persistencia.


- **Ideas futuras de Leo (2026-10-29, adicional):** Symphony con su propia linea de agentes basados en LLMs ya existentes - un agente para codigo, uno para tareas sencillas, uno para investigacion, etc. - cada uno con system prompt, herramientas y perfil optimizados para su rol (en vez de un unico agente generico). Requiere ADR de roles, prompts y seleccion automatica por tipo de tarea; complementa los perfiles @best-coding/@best-reasoning de P19.S2 y el planeador P19.S3. Sin codigo por ahora.
