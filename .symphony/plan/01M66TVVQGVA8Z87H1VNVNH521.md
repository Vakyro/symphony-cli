# Plan: Recortar contexto de router y handoff
## Fase 1: Reducir y medir tokens sin perder información necesaria
Verifica: cargo nextest run -p symphony-context -p symphony-daemon -E 'not(binary(daemon))'; cargo clippy -p symphony-context -p symphony-daemon -- -D warnings; benchmark fijo documentado en docs/research/tokens-p18-base.md (con SYMPHONY_LIVE desactivado).
- [x] Leer instrucciones, paso P18.S1 y benchmark; identificar repetición y alcance.
- [!] Aplicar recortes deterministas y verificar tests, clippy y benchmark.
  > Segundo intento fallido (2026-10-30): entorno SYMPHONY_* aislado; transformador aprobado, pero handoff_combines_the_checkpoint_with_live_git falla porque falta **Asistente:** Voy por partes. (runtime.rs:1686). 168 passed, 1 failed, 3 skipped, 42 no ejecutados. Parada solicitada tras dos fallos; no relajar aserciones. cargo xtask check pasó fmt/clippy del workspace pero excedió 600 s compilando tests; benchmark posterior y clippy acotado pendientes.
- [x] Registrar la medición previa, el bloqueo y el relevo en benchmark, bitácora, STATUS y SESSIONS; crear commit `1ab9bff` (`perf: [P18.S1]`).
- [ ] Completar la tabla antes/después tras resolver el gate fallido con autorización. Fase 1 incompleta; no iniciar otra fase.
