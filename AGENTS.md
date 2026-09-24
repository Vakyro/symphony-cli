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
