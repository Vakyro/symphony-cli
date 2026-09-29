# ADR-0007 · El gate de uso de P07.S10 pasa de «≥ 2 semanas» a un criterio de contenido

- **Estado:** ACEPTADO
- **Fecha:** 2026-09-29
- **Autor:** claude-code/sonnet-5.5 · **Aprobado por:** Leo (2026-09-29)
- **Fase/paso:** P07.S10 (gate) · P07.5

## Contexto
PLAN P07.S10 exigía que Leo usara v0.1 **al menos 2 semanas** antes de revisar P08–P16. La razón de fondo era conseguir evidencia de qué duele al usar Symphony, no cumplir un calendario.

Conversación del 2026-09-29 (resumen fiel):
- **Leo** probó el chat de S7 (Claude y Codex, con cambio de proveedor) y dejó sus observaciones en `docs/research/uso-v0.1.md` § Día 2. Después dijo: «ya probé todas las funcionalidades del CLI, no creo que sea necesario hacerlo 18 días más, ¿qué opinas?» (llevaba ~3 días de uso, desde 2026-09-26; los «18 días» son lo que faltaba de las 2 semanas).
- **El agente** opinó que sí tenía sentido acortarlo, con condiciones: (1) el gate es un medio para tener datos y ya hay señales concretas (Día 1: chat con failover; Día 2: lista de mejoras); (2) el ADR-0006 ya había adelantado la replanificación del chat, así que el gate ya no bloquea lo que se hace; (3) cuidado: ~3 días de uso casi todos en el chat no dicen nada del resto de P08–P16 (scheduler multiagente, Context Engine, más proveedores), y cambiar el gate cambia el PLAN, lo que exige ADR.
- **Propuesta del agente:** (a) cambiar el gate a criterio de contenido; (b) cerrar P07.S10 después de P07.5, revisando P08–P16 con lo que vaya saliendo; (c) conservar el gate de tiempo solo para lo que dependa de uso prolongado.
- **Decisión de Leo:** «Estoy de acuerdo» y pidió documentarlo y empezar.

## Opciones consideradas
1. **Mantener las 2 semanas.** Seguro, pero retiene a Leo por calendario aunque ya hay evidencia; no aporta más señal sobre el chat.
2. **Eliminar el gate.** Rápido, pero P08–P16 quedarían decididas sin datos de lo que casi no se ha usado.
3. **Criterio de contenido (elegida).** El gate se cumple por la evidencia recogida, no por los días; el tiempo solo se exige donde el uso prolongado sea la evidencia.

## Decisión
1. **P07.S10 se cumple por criterio, no por 14 días.** Criterio: Leo usó v0.1 y el chat con **los dos proveedores** (Claude Code y Codex), incluido un cambio de proveedor, y anotó qué le duele en `docs/research/uso-v0.1.md`. **Cumplido para el chat** (Día 1 y Día 2).
2. **La revisión de P08–P16 (`the-council` + ADR de replanificación) se hace después de cerrar P07.5**, con la evidencia acumulada hasta entonces. Hasta ese ADR, P08–P16 **siguen provisionales** y nadie crea sus crates, tablas o vistas.
3. **El gate de tiempo se conserva solo para lo que dependa de uso prolongado** (p. ej. el scheduler multiagente y sus recursos, P08): no se replanifica ni se empieza sin haberlo usado el tiempo que ese ADR fije.
4. **Antes de S8, se atienden las observaciones del Día 2** por prioridad (`uso-v0.1.md` § Priorización): textos cortados (bug), proceso del agente en vivo con medición de rendimiento, copiar/scroll de respuesta, exportar a `.md`. Animaciones y mascota quedan para después.

## Evidencia
`docs/research/uso-v0.1.md` (Día 1, recomendación clave del chat; Día 2, uso del chat en terminal) y `docs/phases/P07.5-chat.md` § S7 (prueba live Claude → Codex con contexto conservado).

## Consecuencias
- PLAN P07.S10 y el aviso ⚠️ de §8 se actualizan para citar este ADR.
- STATUS: el gate de ≥ 2 semanas ya no es un bloqueo para P07.5; la próxima revisión de fases es el ADR posterior al cierre de P07.5.
- El ADR-0006 (P08–P16 diferidas «hasta cumplir el gate de uso») sigue vigente, con el gate redefinido aquí.
- Riesgo asumido: pocos datos de uso sobre las fases no relacionadas con el chat; se mitiga con el punto 3.
