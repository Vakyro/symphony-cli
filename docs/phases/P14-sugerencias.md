# P14 · Sugerencias de modelo y experimentos

| Campo | Valor |
|---|---|
| Estado | EN CURSO (S1–S4 hechos; cierre pendiente) |
| Rama | phase/p14-sugerencias |
| Inicio / cierre | 2026-10-12 / — |
| Agentes que trabajaron | claude-code/sonnet-5.5 |
| Tag | p14-done (pendiente) |
| Docs usados | IDEA §5.7, FLOW §8.5, DB §3.E, ADR-0016, ADR-0017 |

**Diseño.** Las sugerencias son **reglas deterministas** sobre lo que ya hay en la base (checkpoints, puntajes del router): nunca cambian el modelo por sí solas. El desempeño aprendido es un ajuste acotado por (profile, modelo) que solo cuenta con 10 muestras (ADR-0016). Los experimentos de ML se rechazan para v1 con evidencia (ADR-0017).

## Pasos

### P14.S1 · Migración 007 — ✅
- `model_suggestions` (DB §3.E, con índice único de una pendiente por agente) y `profile_models.outcome_score_sum`. La 007, no la 006: la 006 es la de checks de plugin (ADR-0015). `core::SuggestionStatus` verificado contra DB §3.E; `store::suggestions` (insertar, pendiente por agente/proyecto, resolver, silenciar, `record_outcome`).
- **Verifica:** `symphony-store --test suggestions` (una pendiente a la vez, resolución única, `MUTED` apaga las siguientes, muestras y ajuste) y los tests de esquema (`user_version` 7).

### P14.S2 · Sugerencias por reglas — ✅
- **Qué se hizo:** `router::suggest` (puro): `classify` por los archivos tocados (`Docs`, `Config`, `Tests`, `Code`, `MultiFile` = ≥ 4 archivos de código en ≥ 3 carpetas), `change_reason` (solo de lo liviano a lo pesado: documentación o configuración → varios archivos de código) y `better_model` (el elegible de mayor puntaje `@code` con ventaja ≥ 0,10 sobre el actual; confianza = 0,5 + ventaja, tope 0,95). **Daemon:** tras cada checkpoint tomado (`EventBus::checkpoints_taken`), `suggestions::spawn` evalúa al agente — solo si está vivo, sin pendiente ni «no sugerir», con datos que ya están en la base (A11) — y guarda una sugerencia `PENDING`. IPC `suggestion.list|resolve`; `agent.inspect` trae `suggestion`. `switch` cambia al modelo sugerido (y la marca `ACCEPTED`), `keep` la descarta, `mute` apaga las próximas de ese agente. **Superficies:** banner en la vista Chat y en la del agente (`Ctrl+T`/`Ctrl+K`/`Ctrl+N` en el chat; `S`/`K`/`N` en el agente) y `symphony suggestion list|switch|keep|mute`.
- **Verifica:** tests unitarios de las reglas; L2: documentación → refactor propone un modelo mejor sin cambiar nada, aceptarla hace un run nuevo con el modelo sugerido, «no sugerir» no vuelve a proponer; sin cambio de tipo o con el mejor modelo ya elegido no hay sugerencia; vista TUI.
- **Límite conocido:** «cómo empezó» es el checkpoint más viejo con archivos que queda (se conservan 20): en una tarea muy larga puede ser uno posterior al arranque.

### P14.S3 · Desempeño aprendido — ✅
- **Qué se hizo:** ADR-0016. Cada run que termina bajo un profile aporta una muestra (1,0 limpio, 0,5 con reintentos de validación, 0,0 si muere); cuota, auth, cambio o parada del usuario y fase terminada no cuentan. El ajuste `clamp((media − 0,75)·0,4, ±0,10)` solo existe con ≥ 10 muestras y el router compite con `base_score + ajuste`.
- **Verifica:** tests de `learning` y L2 `learned_performance_counts_each_finished_run_and_raises_the_score_only_with_enough_samples` (9 muestras: puntaje igual; 10: sube).

### P14.S4 · Experimentos ML — ✅ (rechazados)
- ADR-0017: modelo de decisión, compresión ML y embeddings rechazados para v1, sin crear la feature `experimental-ml`. Motivo: no hay evidencia de que las reglas se queden cortas y los tres chocan con CONSTRAINTS (determinismo, sin embeddings en v1) o con lo medido (BALANCED ≈ 7 % de RAW; router en µs). Se reabre con una medición, no antes.

## Estado final
(pendiente: revisión de código y cierre)

## Notas para el siguiente agente
- Los puntajes de `profile_models` de fábrica dependen de heurísticas: los tests de sugerencias los fijan con `set_code_score`.
- Un agente en `COMPLETED` ya no recibe sugerencias (no hay a quién cambiar); un chat en `READY` sí.
