# ADR-0017 · Experimentos de ML (modelo de decisión, compresión ML, embeddings): rechazados en v1

- **Estado:** ACEPTADO (rechaza los tres; reevaluable con evidencia)
- **Fecha:** 2026-10-12
- **Autor:** agente (claude-code/sonnet-5.5) · **Aprobado por:** Leo (delegó alcance en ADR-0014)
- **Fase/paso:** P14.S4

## Contexto
PLAN P14.S4: evaluar, «solo si P10 y P13 muestran que las reglas se quedan cortas», un modelo de decisión local, compresión ML o embeddings, cada uno detrás de la feature `experimental-ml` y **medido** contra las reglas (latencia, RAM, acierto). «Rechazar es un resultado válido.» CONSTRAINTS (A6, A7, A8) y STACK: sin embeddings en v1, sin LLM en el camino crítico, sin dependencias de red.

## Opciones consideradas
1. **Modelo de decisión local** para elegir/sugerir modelos.
2. **Compresión ML** del contexto del handoff.
3. **Embeddings** para búsqueda semántica en el broker de contexto.
4. **Seguir con reglas deterministas** y medir.

## Decisión
Se **rechazan 1, 2 y 3** para v1; no se crea la feature `experimental-ml` ni se añade ninguna dependencia.

- **Modelo de decisión.** El router puntúa en microsegundos (8–23 µs, P10) y se explica sin otro modelo. Las sugerencias por reglas (P14.S2) y el ajuste aprendido (ADR-0016) cubren lo que IDEA §5.7 pide. No hay evidencia de que las reglas se queden cortas: no existe todavía un conjunto de decisiones etiquetadas contra el cual medir acierto, y entrenar sobre las propias decisiones del router sería circular.
- **Compresión ML.** La compresión determinista de P09 deja BALANCED en ~7 % de RAW y AGGRESSIVE en ~4 % con el objetivo, el «qué seguía», los archivos y el cambio real intactos (P09.S10) y es reversible por `ctx://`. Un compresor ML sería no determinista, sin reversibilidad garantizada y de coste de RAM/latencia que el daemon (16,5 MB en reposo, ADR de P07) no justifica.
- **Embeddings.** Prohibidos en v1 por STACK y CONSTRAINTS; la búsqueda FTS5/BM25 (P09.S2) resolvió en vivo el caso real (el modelo encontró `src/zafiro.rs` con `context_search`, P11.5.S9).

## Evidencia
P09.S10 (L2: tamaños y contenido por modo), P10.S4 (µs del router), P11.5.S9 (broker MCP en vivo), ADR-0016 (aprendizaje determinista). P13 (benchmarks de cierre) puede reabrir esto si muestra p95 de handoff o de búsqueda fuera de CONSTRAINTS.

## Consecuencias
- Sin código nuevo. Si P13 o el uso real muestran un hueco concreto, se abre un ADR con la medición (latencia, RAM, acierto) **antes** de añadir cualquier dependencia.
