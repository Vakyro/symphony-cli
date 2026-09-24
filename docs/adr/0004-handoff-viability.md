# ADR-0004 · Viabilidad del handoff entre proveedores (AGENT ≠ MODEL)

- **Estado:** PROPUESTO (lo acepta Leo en el gate de P01)
- **Fecha:** 2026-09-24
- **Autor:** claude-code/opus-5.5 · **Aprobado por:** Leo (pendiente)
- **Fase/paso:** P01.S8

## Contexto
IDEA §7, fila 4: "Un checkpoint sin resumen permite que otro CLI continúe". Si falla, Symphony queda como gestor paralelo de CLIs. Es el supuesto que decide el producto.

## Opciones consideradas
1. **Seguir con el diseño de IDEA §5.5:** checkpoints incrementales + handoff a otro proveedor (P06, P10).
2. Quedarse como gestor paralelo de CLIs, sin handoff.

## Decisión
Opción 1. El handoff funciona con un checkpoint que:
- se actualiza **en cada evento de hook**, sin pedirle nada al modelo;
- guarda sesión, `cwd`, `transcript_path`, modelo, último comando y su resultado, plan estructurado si existe, último mensaje del asistente (del `Stop` o de la cola del transcript) y `git status`;
- al hacer el handoff se combina con **el git vivo del worktree** (diff + archivos nuevos). Git manda (H2).

## Evidencia
- P00.S0 (manual): 2/2 (`docs/research/prevalidacion.md`).
- P01.S6 (automatizado): **6/6**. Tres tareas medianas, en los dos sentidos, con kill tras una edición, durante la suite de tests y a mitad de trabajo. En todos los casos el sucesor continuó sin reexplicación, no rehízo el trabajo (los archivos de A quedaron idénticos), dejó todos los tests en verde y no hubo huérfanos (`spikes/results/test-d.md`).
- Ningún CLI dejó un plan estructurado en headless (H1). El sucesor se orienta con el objetivo + git y, cuando existe, el último mensaje.

## Consecuencias
- **P06 (checkpoints):** el diseño de IDEA §5.5 se confirma, con estos ajustes:
  - el "qué seguía" sale del **último mensaje del asistente**; los TODOs estructurados son opcionales;
  - el snapshot de git en el checkpoint es una ayuda; el handoff siempre vuelve a leer git en vivo;
  - el formato del prompt de `spike-hook handoff` es el punto de partida del `HandoffAssembler` (P06/P09).
- **P10 (salud):** hace falta un **watchdog de inactividad** por agente. Un CLI sin red espera indefinidamente sin fallar (Test D, T2). Los mensajes `Reconnecting… n/5` del stream de Codex y `system/api_retry` de Claude se pueden usar como señales de `ProviderError`.
- **P05 (Codex):** el sandbox `workspace-write` de Windows provoca `EPERM` con `node --test`. Hay que evaluar `--add-dir` / permisos en el adapter.
- Límite conocido: tareas de 6–8 archivos en un repo pequeño. Se revalida con tareas más largas en el benchmark de P08.
