# ADR-0016 · Desempeño aprendido: un ajuste acotado, con mínimo de muestras

- **Estado:** ACEPTADO
- **Fecha:** 2026-10-12
- **Autor:** agente (claude-code/sonnet-5.5) · **Aprobado por:** Leo (delegó alcance y orden en ADR-0014)
- **Fase/paso:** P14.S3

## Contexto
IDEA §5.7: el model registry puede guardar «desempeño aprendido (futuro): condicionado por características de la tarea, con un mínimo de muestras antes de influir en el router». DB §3.E ya reserva `profile_models.learned_adjustment` y `sample_count` («se aplica solo si `sample_count` ≥ umbral»). PLAN P14.S3 pide dejar el umbral en un ADR. CONSTRAINTS A6/A7: el router es determinista y sin LLM; lo «inteligente» va detrás de config con fallback.

## Opciones consideradas
1. **Aprendizaje por (modelo, tipo de tarea) con características extraídas de cada tarea** (archivos, comandos…). Más fino, pero necesita muchas más muestras por celda y un clasificador de tareas propio.
2. **Por (profile, modelo)**: el profile ya es el «tipo de tarea» que el usuario o el sistema eligió (`@code`, `@docs`, `@debug`…), y es la clave que usa `profile_models`. Pocas celdas, convergen rápido y no hay clasificador que mantener.
3. No aprender nada.

## Decisión
Opción 2.

- **Qué es una muestra.** Un run que termina bajo un profile (su decisión de routing lo nombra): `COMPLETED` sin validaciones fallidas del agente → **1,0**; `COMPLETED` con validaciones fallidas → **0,5**; `CRASH`/`NO_HEARTBEAT` → **0,0**. Cuota, rate limit, auth, cambio o parada del usuario y fase terminada **no** son muestras: no hablan del modelo. Un modelo elegido a mano (sin profile) tampoco.
- **Dónde.** `profile_models.sample_count` (+1) y `outcome_score_sum` (+ resultado), dentro de la transacción que cierra el run (una consulta y una actualización; A11).
- **Ajuste.** `ajuste = clamp((media − 0,75) · 0,4, ±0,10)` y **solo con `sample_count` ≥ 10**; antes queda `NULL` y el router usa el puntaje base.
- **Cómo influye.** El router compite con `base_score + ajuste` (acotado a 0–1). No cambia filtros de disponibilidad, salud ni cuota: solo el desempate fino entre modelos elegibles.
- **Por qué esos números.** 10 muestras dan un intervalo de confianza amplio pero ya descartan un caso aislado; 0,75 es «cumple lo esperado» (3 de cada 4 runs limpios); ±0,10 hace que lo aprendido corrija sin mandar (el puntaje base y los pesos del profile siguen siendo la verdad). Son constantes en `symphony_core::learning`, con tests; cambiarlas requiere nuevo ADR.

## Evidencia
Tests de `learning` (nada antes del mínimo, el tope vale con cualquier cantidad de muestras, datos no finitos se ignoran), de `suggestions::record_outcome` y L2 `learned_performance_counts_each_finished_run_and_raises_the_score_only_with_enough_samples` (9 muestras: puntaje igual al de base; 10: sube).

## Consecuencias
- Migración 007 añade `profile_models.outcome_score_sum`.
- Explicabilidad: el puntaje que muestra `/explain-route` ya incluye el ajuste; el detalle (muestras y media) queda en `profile_models`.
- Sin cambios de comportamiento hasta que un (profile, modelo) junte 10 runs.
- No se aprende de proyectos distintos por separado: el registro de modelos es global (v1).
