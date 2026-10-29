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

## Backlog / ideas futuras de P17 (documentadas, no implementadas)

- **S8 · Renderizado rico del chat (pendiente):** colores en las respuestas estilo OpenCode. Un MD temporal que vive solo durante la sesión, con colores asignados por importancia (info/warn/error). Dos opciones a decidir con ADR corto: (A) colorear todo el markdown del asistente (requiere parser como pulldown-cmark, justificar dependencia en STACK) o (B) solo bloques especiales marcados por el agente (protocolo documentado en el harness). Decidir si el color lo asigna el agente o la TUI por heurística, y si el MD se persiste en el transcript o solo en memoria. No confundir con el tema de la TUI (S1), ya implementado.
- **Política configurable de Symphony/harness (pendiente, fuera de P17):** análogo a los permisos de Claude Code (allow/deny/ask, modos default/acceptEdits/plan/dontAsk/bypassPermissions, archivos ignorados estilo .claudeignore, política por proyecto en .symphony/policy.toml). Requiere ADR nuevo y decisión de alcance (solo archivos vs. también shell/git). Documentado en PLAN.md bajo "Ideas futuras (sin fase)".

