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
