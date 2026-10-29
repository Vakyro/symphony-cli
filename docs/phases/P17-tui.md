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
