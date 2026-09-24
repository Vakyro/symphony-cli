# STATUS — Symphony CLI

**Actualizado:** 2026-09-24 · por claude-code/opus-5.5
**Fase actual:** P03 · Persistencia (rama `phase/p03-store`)
**Paso actual:** P03.S5 · Recuperación al arrancar
**Estado del paso:** EN CURSO
**En curso por:** claude-code/opus-5.5 desde 2026-09-24

## Salud del repo
- `cargo xtask check`: ✅ (72 tests)
- `cargo deny check`: ✅
- CI en main: ✅ (ubuntu, windows, macos, msrv, deny)
- Tests conocidos en rojo: ninguno

## Progreso de la fase actual
- [x] P03.S1 Crate `store` y migración 001
- [x] P03.S2 Store writer
- [x] P03.S3 Repositorios
- [x] P03.S4 Crate `object-store`
- [ ] P03.S5 Recuperación al arrancar   ← aquí
- (resto de P03 en PLAN.md)

## Próxima acción concreta
Al iniciar el daemon: sesiones ACTIVE sin daemon vivo → INTERRUPTED + recovery_items(SESSION_INTERRUPTED). Test: crear sesión, matar el daemon, reiniciar y ver el recovery item.

## Handoff para el siguiente agente
(llenar si la sesión se cortó a media tarea — ver PLAN §4.4)
- Estaba haciendo:
- Archivo/función a medias:
- Comandos corridos y resultado:
- Hipótesis descartadas:

## Bloqueos y preguntas para Leo
- (ninguno)

## Pendientes arrastrados
- P05: verificar que el servidor del named pipe pertenece al usuario actual antes de mandar datos sensibles (ver bitácora P02.S8).

## Fases
| Fase | Estado | Tag |
|---|---|---|
| P00 | ✅ | p00-done |
| P01 | ✅ | p01-done |
| P02 | ✅ | p02-done |
| P03 | 🟡 en curso | |
| P04–P16 | ⏳ (P08–P16 provisionales hasta P07.S10) | |
