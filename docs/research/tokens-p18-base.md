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

## Benchmark realista · parte 3 (2026-10-31)

La tabla anterior queda como histórico: su carga sintética (mensajes de 800 `z`, diff de `x`) no admite ningún compresor. Desde esta parte el mismo test (`p18_routing_handoff_token_baseline`, mismo comando de reproducción) carga contenido real del repo, fijo en `fixtures/handoff-p18/` (`eol=lf` por `.gitattributes`, así que los conteos no dependen del sistema):

- `*.chat`: conversaciones con llamadas a herramientas (`→ Bash: …`) y sus salidas reales: compilación y `cargo nextest` del L2 de handoff, la suite de `symphony-context`, `git grep`, `git status`, un fallo de nextest con su pánico y un error de clippy. Las rutas absolutas se normalizaron a `C:\proj\symphony-cli`. `<<<usuario>>>`/`<<<asistente>>>` abren cada mensaje.
- `*.diff`: diffs unificados reales (`git show` de 672f940 + 77b7bb8 con `Cargo.lock`, a440358 y e41e0a6).
- `new_file_log.rs`: un archivo de código real como archivo nuevo sin commit.

El daemon arma el resto igual que en producción: objetivo = primer mensaje del usuario, `plan_tail` = últimos 2.000 caracteres del último mensaje del asistente (`checkpoint.rs::PLAN_TAIL_MAX`, así que el checkpoint repite ese final), archivos tocados y estado de git desde el diff, dos hechos vigentes, el último comando fallido y dos fallos.

### Línea base realista (código en e41e0a6, antes de los recortes de esta parte)

| Ruta | Mensajes | Chars conversación | Chars diff | RAW | SAFE | BALANCED | AGGRESSIVE |
|---|---:|---:|---:|---:|---:|---:|---:|
| rotation_context | 8 | 35.351 | 61.478 | 26.221 | 26.221 | 16.016 | 8.881 |
| fallback_quota | 8 | 25.799 | 35.841 | 16.247 | 14.542 | 6.390 | 5.190 |
| manual_switch | 3 | 1.782 | 3.621 | 2.114 | 2.114 | 1.602 | 1.602 |
| same_provider | 12 | 41.622 | 3.621 | 11.827 | 11.827 | 6.468 | 3.343 |

Lo que ya ahorra BALANCED sale del `GitDiffReducer` y del recorte ciego a 4.000 caracteres por mensaje. Ese recorte corta las salidas largas por el principio y puede dejar fuera justo lo importante (el pánico del test o el error de clippy al final de un bloque); la parte siguiente apunta a colapsar esas salidas con los compresores deterministas en vez de recortarlas, y a no repetir lo que el checkpoint ya trae.

### Antes/después de los recortes de la parte 3 (2026-10-31)

Recortes, todos deterministas y solo en BALANCED/AGGRESSIVE (RAW/SAFE no cambian):

- Bloques ``` de salida de herramientas dentro de los mensajes: el que ya sale igual en un mensaje posterior se cambia por una referencia, y el largo se resume con `compress` (colapso de logs y de salidas de tests) en vez del recorte ciego por el principio, así que el pánico y el error de clippy del final del bloque se conservan.
- Un final del mensaje igual al `plan_tail` del checkpoint se cambia por referencia, conservando la apertura del mensaje.
- `GitDiffReducer`: deja una línea de contexto alrededor de cada cambio (como `git diff -U1`) y cuenta el resto en un aviso.

Tokens estimados (mismo test, mismas fixtures); Δ = después frente a antes:

| Ruta | Modo | Antes (e41e0a6) | Después | Δ |
|---|---|---:|---:|---:|
| rotation_context | RAW | 26.221 | 26.221 | 0 % |
| rotation_context | SAFE | 26.221 | 26.221 | 0 % |
| rotation_context | BALANCED | 16.016 | 11.127 | **−30,5 %** |
| rotation_context | AGGRESSIVE | 8.881 | 7.601 | −14,4 % |
| fallback_quota | RAW | 16.247 | 16.247 | 0 % |
| fallback_quota | SAFE | 14.542 | 14.542 | 0 % |
| fallback_quota | BALANCED | 6.390 | 4.207 | **−34,2 %** |
| fallback_quota | AGGRESSIVE | 5.190 | 4.207 | −18,9 % |
| manual_switch | RAW | 2.114 | 2.114 | 0 % |
| manual_switch | SAFE | 2.114 | 2.114 | 0 % |
| manual_switch | BALANCED | 1.602 | 1.561 | **−2,6 %** |
| manual_switch | AGGRESSIVE | 1.602 | 1.561 | −2,6 % |
| same_provider | RAW | 11.827 | 11.827 | 0 % |
| same_provider | SAFE | 11.827 | 11.827 | 0 % |
| same_provider | BALANCED | 6.468 | 1.654 | **−74,4 %** |
| same_provider | AGGRESSIVE | 3.343 | 1.654 | −50,5 % |

Frente a RAW, BALANCED queda en −57,6 %, −74,1 %, −26,2 % y −86,0 %.

**Meta −40 % por ruta en BALANCED: cumplida solo en `same_provider`.** `rotation_context` (−30,5 %) y `fallback_quota` (−34,2 %) quedan cortas: lo que sobra es sobre todo el diff, ya en contexto mínimo; bajar más exige quitar hunks enteros (perder cambios) o resumir código, que no es determinista sin pérdida. `manual_switch` es una conversación de 3 mensajes cortos sin salidas repetidas ni final igual al plan: no hay nada que colapsar sin perder lo necesario. En las rutas grandes AGGRESSIVE ya coincide con BALANCED porque los recortes dejaron todo por debajo de sus topes. Los tests de regresión (`p18_balanced_keeps_failures_openings_and_authors` y los de `compress`) comprueban que se conservan errores, aperturas y autores, sin relajar los existentes.

## Fixtures

Hasta la parte 2 se reutilizaba el fixture fijo `fixed()` del módulo de handoff; se fija texto sintético (`x` para el diff), número de mensajes y 800 caracteres por mensaje. La tabla se regenera desde el test; no depende del repo de trabajo, reloj, proveedor, configuración del usuario ni red.
