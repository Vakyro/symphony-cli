# P12 · Plugins

| Campo | Valor |
|---|---|
| Estado | CERRADA |
| Rama | phase/p12-plugins |
| Inicio / cierre | 2026-10-12 / — |
| Agentes que trabajaron | claude-code/sonnet-5.5 |
| Tag | p12-done |
| Docs usados | ADR-0015, STACK §17, §46, §47, IDEA §5.12, PLAN § P12 |

**Diseño (ADR-0015).** Plugins como **procesos aislados** con un protocolo JSON de una vuelta (stdin → stdout, una línea cada uno). Instalación global en `~/.symphony/plugins/<nombre>/` y activación por proyecto en `[plugins]` de `project.toml`. v1 ejecuta `validator` (tier 2 del gate de validación) y `context-transformer` (handoff); `tool` y `provider-adapter` se aceptan en el manifiesto y el host los rechaza (v1.1). Lo que devuelve un plugin pasa por `redact` y se acota; un plugin que muere, se cuelga o miente degrada su propio resultado, nunca al daemon.

## Pasos

### P12.S1 · Protocolo y manifiesto — ✅
- **Qué se hizo:** crate `symphony-plugins`. `manifest.rs`: `plugin.toml` con validación estricta (nombre `[a-z0-9-]` ≤ 40, `plugin_api` solo 1, `exec` sin shell, `exec[0]` relativo al plugin o nombre del PATH —sin rutas absolutas, `..` ni unidad—, `timeout_secs` 1–300, claves desconocidas rechazadas). `protocol.rs`: petición/respuesta v1 (`describe`, `validate`, `transform`), la respuesta es la última línea no vacía de stdout y cualquier cosa fuera de lo esperado es un `ProtocolError` (versión distinta, JSON roto, sin `ok`, más de 1 MiB).
- **Archivos clave:** `crates/plugins/src/{manifest,protocol}.rs`, `docs/adr/0015-plugins-procesos-aislados.md`.
- **Cómo se verificó:** tests unitarios del manifiesto (válido, defaults, `plugin_api` 2, nombre malo, clave desconocida, tipo desconocido, rutas peligrosas, tiempos fuera de rango) y del protocolo.

### P12.S2 · Plugin host — ✅
- **Qué se hizo:** `host.rs`: `discover` (`<root>/<nombre>/plugin.toml`; el nombre de la carpeta debe ser el del manifiesto; un plugin roto se reporta y no impide listar los demás) y `Plugin::call/describe/validate/transform`. Un proceso por llamada bajo `symphony-process`: grupo propio, 512 MiB y 16 procesos máximos, `timeout_secs` (al vencer se mata el árbol), stdout acotado a 1 MiB (se sigue drenando), el final de stderr (2 KiB, redactado) en el error. `validate` y `transform` redactan y acotan lo devuelto (`transform`: no vacío y ≤ 4× el original). **Daemon:** `[plugins]` en `project.toml` (`validators`, `context_transformers`) y `ProjectConfig::effective_validation` (un proyecto con solo validators de plugin valida con gate 2); los validators corren en el tier 2 y dejan un check `plugin` en `validation_checks` (**migración 006**, `CheckName::Plugin`, DB §3.H actualizado) con sus hallazgos en el resumen; un plugin que no corre o no está instalado deja `SKIPPED` con el motivo. Los transformers se aplican al handoff (`Runtime::prepare_handoff`) y un fallo deja el texto sin cambiar. IPC `plugin.list` y `plugin.check`; CLI `symphony plugin list|check`.
- **Archivos clave:** `crates/plugins/src/host.rs`, `crates/daemon/src/plugins.rs`, `crates/daemon/src/validation.rs`, `crates/core/src/config.rs`, `migrations/006_plugins.sql`, `crates/testkit/src/bin/fake-plugin.rs`.
- **Cómo se verificó:** `symphony-plugins` (16: un plugin que muere es `Crashed` con su stderr y el host sigue sirviendo; uno que se cuelga se mata en su `timeout` **sin dejar proceso vivo**; basura, mentira de `plugin_api`, respuesta enorme, silencio y error remoto son errores de protocolo; un secreto en la respuesta sale redactado; nombre distinto del manifiesto → `Mismatch`; `tool` no se ejecuta; transformer acotado; descubrimiento) y `symphony-daemon --test runtime` (5: validator que encuentra algo falla el gate con sus hallazgos; uno que pasa deja completar; uno que muere o no está instalado queda `SKIPPED`; `plugin.list/check`; un transformer reescribe el handoff y uno roto se ignora).
- **Desviación del spec:** DB §3.H gana `plugin` en `validation_checks.name` (ADR-0015).

### P12.S3 · Plugin de ejemplo — ✅
- **Qué se hizo:** `examples/plugins/validator-todo-check` (miembro del workspace): falla si el diff agrega un `TODO` sin issue (`TODO(#123)`, `TODO #123`, `TODO(ABC-12)` valen). Solo mira líneas agregadas y atribuye cada hallazgo a su archivo. QUICKSTART § Plugins explica instalación, activación y el protocolo.
- **Cómo se verificó:** tests unitarios de la lógica y `tests/host.rs`, que instala el binario y su `plugin.toml` como lo haría un usuario y lo ejecuta con el host real (`describe` y `validate`).

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|
| Un plugin que muere/se cuelga no afecta al daemon | tests del host y del gate | ✅ |
| Un validator de plugin gobierna el gate (tier 2) | L2 con `fake-plugin` | ✅ |
| El ejemplo funciona con el host real | `examples/…/tests/host.rs` | ✅ |

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|
| `tool` y `provider-adapter` no se ejecutan | Quien los declare ve «todavía no se soportan» | `plugin.toml` con `kind = "tool"` + `symphony plugin check` | v1.1 (ADR-0015) |
| Un proceso por llamada | Cada validación paga el arranque del plugin | medir con `symphony plugin check` | aceptable en v1; servidor persistente si hace falta |

## Decisiones tomadas
- ADR-0015 (modelo de confianza, protocolo, alcance, migración 006; la de P14 pasa a 007).

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|
| `symphony-plugins` (propio) | — | host de plugins | STACK §17 / árbol §5 (`crates/plugins`) |

### P12.S4 · Cierre — ✅
- **Revisión de código** del diff de la fase (`code-review`, nivel alto): 10 hallazgos.

| # | Hallazgo | Decisión |
|---|---|---|
| 1 | un validator que no pudo correr queda `SKIPPED` y el gate «pasa» | **Decisión consciente + mitigación.** Igual que un comando que no arrancó (P08.S6); bloquear al agente por un plugin roto lo haría «arreglar» algo que no es suyo. Ahora la validación lo dice: el preflight de integración (agente y chat) pasa de `PASS` a `WARN` («pasó, pero un check no pudo correr») y `Validation::has_skipped` lo expone. Se corrigió el comentario que prometía lo contrario |
| 2 | escribir la petición a stdin quedaba fuera del tiempo máximo | **Corregido.** La escritura cuenta para `timeout_secs` (se mata el árbol si el plugin no lee) |
| 3 | el plugin hereda el entorno del daemon (claves, tokens) y el diff va sin redactar | **Corregido.** Las variables con `KEY/TOKEN/SECRET/PASSWORD/CREDENTIAL/AUTH` llegan vacías al plugin y el diff pasa por `redact` y tiene tope (512 KiB). El ADR ya decía que el aislamiento no es un sandbox contra un plugin malicioso: sigue siendo cierto (puede leer archivos con tus permisos) |
| 4 | el diff omitía archivos nuevos sin seguimiento y no tenía tope | **Corregido.** Los archivos nuevos entran como adiciones (hasta 2 000 líneas cada uno, solo texto) y el diff tiene tope; test `a_validator_plugin_sees_untracked_new_files` |
| 5 | un plugin no resuelto se llamaba `plugin:?` | **Corregido** (`Unavailable { name, why }`) |
| 6 | tras un timeout el árbol podría sobrevivir | **No era cierto:** el handle mata el árbol al soltarse y el test `a_hanging_plugin_is_killed_at_its_timeout` comprueba que no queda ningún proceso vivo |
| 7 | varios transformers lentos retrasan el failover | **Corregido en parte:** presupuesto total de 30 s para los transformers de un handoff; cada uno sigue con su `timeout_secs` dentro de ese tope |
| 8 | un `exec[0]` sin `./` se buscaba solo en el PATH | **Corregido.** Un archivo con ese nombre en la carpeta del plugin gana; el mensaje de error y el comentario lo dicen |
| 9 | activar un validator enciende un gate por defecto (gate 2) | **Aceptado y documentado** (ADR-0015, QUICKSTART § Plugins): activar un validator *es* pedir ese gate; si se quita el plugin, el gate desaparece |
| 10 | la migración usaba `SELECT *` posicional | **Corregido:** columnas explícitas en el `INSERT … SELECT` |

- **Criterios de salida:** un plugin que muere no afecta al daemon (✅ tests del host y del gate); uno que excede el tiempo se mata (✅, sin huérfanos); plugin de ejemplo con tests (✅).

## Estado final
P12 deja plugins como procesos aislados con un protocolo JSON de una vuelta (ADR-0015): `plugin.toml` validado, host con tiempo, memoria, procesos y salida acotados, validators en el tier 2 del gate (check `plugin`, migración 006) y context transformers sobre el handoff, `[plugins]` por proyecto sobre instalación global, `symphony plugin list|check` y un plugin de ejemplo que corre contra el host real. `tool` y `provider-adapter` quedan reservados para v1.1. Un plugin que falla degrada su propio resultado y la integración lo avisa (`WARN`); nunca tira al daemon.

## Notas para el siguiente agente
- Un `exec[0]` absoluto se rechaza en el manifiesto a propósito: los tests del host construyen el `Plugin` a mano o copian el binario a la carpeta del plugin.
- Si añades un tipo de plugin, hay que decidir su punto de enganche en el daemon antes de aceptarlo en el host (`Kind::supported`).
