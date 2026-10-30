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

## Fixtures

Se reutiliza el fixture fijo `fixed()` del módulo de handoff; se fija texto sintético (`x` para el diff), número de mensajes y 800 caracteres por mensaje. La tabla se regenera desde el test; no depende del repo de trabajo, reloj, proveedor, configuración del usuario ni red.
