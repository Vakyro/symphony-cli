# Baseline fijo de tokens de contexto · P18.S1

Medición de referencia previa a cualquier cambio de comportamiento. El contador es la estimación local determinista de Symphony (~4 caracteres por token), no la tokenización de un proveedor. No se invoca ningún CLI real ni se consume cuota.

## Reproducción

```sh
env -u SYMPHONY_LIVE cargo nextest run -p symphony-context p18_routing_handoff_token_baseline --run-ignored only --no-capture
```

## Escenarios y resultados

Cada escenario representa la carga del handoff que las rutas de routing consultan mediante `handoff_tokens()` para evaluar tamaño de contexto o pasar el handoff al executor nuevo. El ensamblador recibe la misma carga fija en los cuatro modos.

| Ruta representada | Carga fija | RAW | SAFE | BALANCED | AGGRESSIVE |
|---|---:|---:|---:|---:|---:|
| rotation_context | 1 mensaje, diff 70.000 chars | 18.026 | 18.026 | 15.546 | 5.546 |
| fallback_quota | 8 mensajes, diff 30.000 chars | 9.460 | 9.460 | 9.460 | 6.980 |
| manual_switch | 3 mensajes, diff 8.000 chars | 2.936 | 2.936 | 2.936 | 2.936 |
| same_provider | 12 mensajes, diff 1.000 chars | 3.029 | 3.029 | 3.029 | 3.029 |

## Alcance de la medición

El test invoca directamente `symphony_context::handoff::assemble`, la misma función usada por `daemon/handoff.rs`, y reporta `tokens_sent`, el valor consumido por `daemon/executor.rs::handoff_tokens` y `routing.rs::Choose::needed_context`. Los casos cubren las cargas fijas elegidas para rotación por contexto, fallback tras cuota, cambio manual y selección restringida al mismo proveedor. No construye un daemon/SQLite ni prueba cuál candidato gana: las etiquetas describen rutas de consumo, y routing no transforma el prompt. Esta separación mantiene el benchmark puro, rápido y sin procesos ni credenciales.

## Recortes de P18.S1 · parte 2

**Validación incompleta (2026-10-30):** se reprodujo la tabla inicial sin cambios (1 test pasado). La suite posterior se detuvo con 111 tests pasados y un fallo en `a_context_transformer_rewrites_the_handoff_and_a_broken_one_is_ignored`: esperaba el prompt en mayúsculas y recibió el original. Los dos tests nuevos de deduplicación pasaron. Por la regla de parada ante un gate fallido, falta repetir el benchmark después del cambio y correr clippy; no se afirma un porcentaje medido de ahorro ni el cumplimiento del 40 %. Las cifras anteriores son únicamente el antes.

El ensamblador sustituye en BALANCED y AGGRESSIVE mensajes completos idénticos al objetivo o al último plan por una referencia a esa sección, solo cuando la referencia es más corta. Conserva el autor, el orden, el texto completo del checkpoint y las instrucciones posteriores distintas. La fidelidad de la conversación pasa a `COMPRESSED` (o sigue en `CLIPPED` si también hay recortes). RAW y SAFE conservan el texto original.

El router no arma otro prompt: `executor.rs::handoff_tokens` consulta `prepare_handoff`, que llama a `handoff::assemble`; su `tokens_sent` llega a `routing.rs::Choose::needed_context`. Por eso el recorte compartido afecta tanto al contexto enviado como al tamaño consultado para routing, sin modificar puntuaciones ni selección de candidatos.

La carga fija no contiene mensajes iguales al objetivo o al plan, y su diff de `x` no es un diff unificado que pueda reducir el compresor existente. No se cambian fixtures, estimador ni límites para aparentar ahorro; los tests existentes fijan además los recortes y el contenido RAW. La deduplicación se verifica por separado con dos tests nuevos, sin sustituir el benchmark ni relajar aserciones anteriores. No se descartan hechos por coincidencias parciales ni archivos por ausencia en un checkpoint que puede estar atrasado: git vivo sigue siendo la fuente de verdad.

### Continuación: segundo intento (2026-10-30)

Con `SYMPHONY_*` y `CARGO_BUILD_BUILD_DIR` eliminados del entorno del comando, el transformador pasó. La suite solicitada terminó con **168 passed, 1 failed, 3 skipped y 42 no ejecutados**: `handoff_combines_the_checkpoint_with_live_git` exige `**Asistente:** Voy por partes.`, pero recibe la referencia al plan. El texto completo permanece en el plan, pero el contrato existente exige también ese prefijo en conversación. Es una regresión del recorte; no se cambió la aserción.

Se detuvo el trabajo tras el segundo intento fallido, según la instrucción de esta sesión. No se ejecutó el benchmark posterior: la tabla antes/después y el porcentaje siguen pendientes, no son 0 % medido. `cargo xtask check` completó fmt/clippy del workspace, pero la herramienta lo interrumpió a los 600 s durante la compilación de tests. El clippy acotado tampoco se volvió a ejecutar.

## Fixtures

Se reutiliza el fixture fijo `fixed()` del módulo de handoff; se fija texto sintético (`x` para el diff), número de mensajes y 800 caracteres por mensaje. La tabla se regenera desde el test; no depende del repo de trabajo, reloj, proveedor, configuración del usuario ni red.
