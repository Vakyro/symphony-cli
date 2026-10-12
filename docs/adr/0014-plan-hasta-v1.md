# ADR-0014 · Plan hasta v1.0: pendientes primero, P12–P16 dejan de ser provisionales

- **Estado:** ACEPTADO
- **Fecha:** 2026-10-12
- **Autor:** claude-code/opus-5.5 · **Aprobado por:** Leo (delegó la decisión: «tienes libertad de hacer todo como creas que es mejor para el proyecto», 2026-10-12)
- **Fase/paso:** replanificación tras `p08-done`

## Contexto
P00–P11 están cerradas (P08 con `p08-done` y el benchmark de 6 proveedores aprobado el 2026-10-12). El PLAN marca P12–P16 como provisionales (⚠️, PLAN §8) y pide revisarlas al llegar, con ADR. Además quedan pendientes repartidos en las bitácoras de P07.5–P11 que ninguna fase recoge: `/compact` entre tareas (ADR-0013 D2), los live de P09 (broker MCP, S10), el merge chat → `main` con review agent (ADR-0006), `needed_context` sin pasar al router, permisos de proveedores fuera de `config.toml`, deltas de texto de Claude en el chat, el bug de `@` en `ctx://`, deuda de `ponytail-review` y un STATUS que creció hasta ser difícil de leer.

Leo pidió un plan para **todo** lo que falta, ejecutado de forma continua por un agente (Sonnet 5.5) hasta agotar su cuota, y delegó el orden.

## Opciones consideradas
1. **Seguir el orden numérico P12 → P16.** Plugins y GUI antes que endurecer; la revisión de seguridad de P13 no vería la GUI y los pendientes siguen sin dueño.
2. **Cortar el alcance de v1 (sin plugins ni GUI, ADR de exclusión).** Más rápido, pero Leo pidió «absolutamente todo».
3. **Fase de pendientes primero y endurecimiento al final, antes de la validación.** Lo que el usuario ya usa (el chat) mejora primero; las funciones nuevas (plugins, sugerencias, GUI) entran antes de P13, para que la revisión de seguridad, el fuzzing, la documentación y la pipeline de release cubran el producto entero.

## Decisión
Opción 3. Mismo mecanismo que ADR-0010 (orden de ejecución distinto del número, sin renumerar fases ni tags):

**P11.5 → P12 → P14 → P15 → P13 → P16**

| Orden | Fase | Por qué en este lugar |
|---|---|---|
| 1 | **P11.5 · Pendientes y pulido** (nueva) | Cierra lo que dejaron P07.5–P11 y mejora el chat, que es el valor principal (STATUS, uso-v0.1). Barata y sin dependencias |
| 2 | P12 · Plugins | Superficie nueva: tiene que existir antes de la revisión de seguridad de P13 |
| 3 | P14 · Sugerencias de modelo | Solo reglas. Migración **006** (la 005 ya es la de validación de P08). S4 (ML) se resuelve con evidencia; rechazar es válido |
| 4 | P15 · GUI de escritorio | Último bloque de funciones; su empaquetado (S5) entra a la pipeline de P13.S7 |
| 5 | P13 · Endurecimiento y pre-release | Endurece el producto completo. Tag `v0.9.0` local |
| 6 | P16 · Validación final y v1.0.0 | Sin cambios |

Reglas de ejecución para el agente que lo implementa:
- **Corre sin parar entre pasos y fases.** Al cerrar una fase (PLAN §4.6) sigue con la siguiente del orden de arriba.
- **Puntos ⛔ (necesitan a Leo):** se documentan en STATUS § «Para Leo» con el comando exacto, y el agente sigue con el siguiente paso que no dependa de ellos. Nunca se queda esperando.
- **Permisos vigentes** (sin preguntar): merge `--no-ff` de la rama de fase a `main`, tag `pNN-done` y push de `main` y del tag (memoria de Leo: «merge a main OK»); live **solo** con modelos gratuitos de OpenCode (permiso del 2026-10-10).
- **Siempre ⛔:** live que gaste cuota (Claude, Codex, Kimi, Antigravity, Copilot); push de ramas que no sean `main`; publicar releases (GitHub Release, Homebrew, crates.io); tag `v1.0.0`; cambiar defaults de permisos de proveedores; ToS.
- Un paso que exija tocar enums o transiciones del core: ADR primero (como siempre). Si la decisión es clara y no cambia comportamiento visible, el agente la escribe como ACEPTADO y la lista en «Para Leo» para revisión; si cambia comportamiento visible, PROPUESTO y ⛔.

## Evidencia
- Pendientes, con su origen: bitácoras P07.5 (§ S7b, S9), P08 (S5–S9, cierre), P09 (S8–S11, tabla de problemas), P10 (S4–S5), P11 (S2–S7, problemas conocidos); `docs/research/uso-v0.1.md` § Día 2 e «ideas de producto»; STATUS § «Pendientes arrastrados».
- Precedente del reordenamiento sin renumerar: ADR-0010 (P10 → P09 → P08).
- `graphify-smart-out/GRAPH_REPORT.md`: la única tabla del spec sin migrar es `model_suggestions` (P14).

## Consecuencias
- PLAN: nueva sección **P11.5**; P12–P16 dejan de ser provisionales; P14.S1 pasa a «Migración 006»; P15.S5 se integra en P13.S7; registro de cambios y mapa de fases actualizados.
- STATUS se reescribe corto: fase actual P11.5, cola en el orden de este ADR y una sección «Para Leo» con todos los ⛔.
- Quedan **fuera de v1** (anotados en P16.S7 como recomendaciones para v1.1): bridge de notificaciones push al móvil (idea de octubre de Leo; el acceso por SSH a la TUI se documenta en P13.S6) y cualquier experimento de ML que P14.S4 no apruebe con evidencia.
