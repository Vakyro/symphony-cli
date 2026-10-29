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

### P17.S1 · Tema y barra de estado — 🟡 (en curso)
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
