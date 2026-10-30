# P18 · Contexto y ahorro de tokens

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p18-contexto |
| Inicio / cierre | 2026-10-30 / — |
| Agentes que trabajaron | codex/gpt-6.1 (P18.S1 parte 1) |
| Tag | — |
| Docs usados | PLAN § P18, CONSTRAINTS §6, LEARNINGS.md |

## P18.S5 · Init de proyecto actualizado — ✅
- **Agente:** codex/gpt-5.x · **Fecha:** 2026-10-30.
- **Qué se hizo:** `symphony init` ahora genera en `project.toml` el estilo del harness, las animaciones de UI y el compactado automático cada 3 tareas; el `.gitignore` del proyecto incluye `.symphony/brain/`; la CLI imprime un resumen de lo creado.
- **Archivos clave:** `crates/core/src/config.rs`, `crates/daemon/src/server.rs`, `crates/cli/src/main.rs`, `crates/cli/tests/projects.rs`.
- **Pruebas:** `unset SYMPHONY_LIVE; cargo nextest run -p symphony-cli --test projects` (3 passed); `cargo clippy -p symphony-cli -- -D warnings` (verde). La corrida completa del crate tuvo fallos ambientales previos en hooks/MCP y dejó LEAK; no se atribuyen a este cambio.
- **Commit:** pendiente.

## Pasos

### P18.S2 · Flintstone por defecto — ✅
- **Agente:** codex/gpt-5.x · **Fecha:** 2026-10-30.
- **Qué se hizo:** el prompt del harness añade por defecto instrucciones Flintstone (respuestas comprimidas y directas, sin relleno). `[harness] style = "normal"` lo apaga; `mode` y los proveedores no cambian.
- **Archivos clave:** `crates/core/src/config.rs`, `crates/daemon/src/executor.rs`, `crates/adapters/common/src/lib.rs`, `crates/adapters/claude/src/lib.rs`, `docs/book/src/configuracion.md`.
- **Medición:** el sufijo Flintstone mide 152 caracteres, aproximadamente 38 tokens con la estimación fija de 4 caracteres/token usada por P18.S1. Es el coste del prompt inicial; no se añadió al benchmark de handoff porque ese benchmark no construye prompts de proveedor.
- **Pruebas:** `env -u SYMPHONY_LIVE cargo nextest run -p symphony-adapter-common` (13 passed); `env -u SYMPHONY_LIVE cargo clippy -p symphony-adapter-common -- -D warnings` (verde).
- **Commit:** `9d9ba05`.

### P18.S1 · Medición inicial de routing y handoff — 🟡
- **Agente:** codex/gpt-6.1 · **Fecha:** 2026-10-30.
- **Qué se hizo:** se añadió una prueba de benchmark ignorada, reproducible y sin red/cuota en el ensamblador puro de contexto. Mide cargas fijas que representan rotación por contexto, fallback por cuota, cambio manual y ruta limitada al mismo proveedor en RAW/SAFE/BALANCED/AGGRESSIVE. No cambia runtime.
- **Archivos clave:** `crates/context/src/handoff.rs`, `docs/research/tokens-p18-base.md`.
- **Cómo se verificó:** test del baseline → 1 passed; pendientes los nextest y clippy indicados por el plan de sesión.
- **Commits:** pendiente.
- **Pendiente / notas:** los escenarios miden el valor de handoff que consume el router, pero no ejercitan la selección de candidatos del daemon; se describe este límite en el baseline. La siguiente parte de S1 puede añadir cobertura de rutas reales antes de optimizar.

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|
| Ensamblado de cuatro modos y salida estable por escenario | `p18_routing_handoff_token_baseline` | 1 passed; tabla guardada en `docs/research/tokens-p18-base.md` |

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|
| No hay aún prueba integrada de selección de candidatos en daemon | El baseline mide handoff consumido por routing, no la decisión del router | Revisar alcance anotado en baseline | Continuar P18.S1 antes de cambiar comportamiento |

## Decisiones tomadas
- El benchmark usa la estimación determinista ya usada en runtime; no agrega tokenizador ni dependencia.
- Se reutiliza el fixture y el patrón de test ignorado existentes; no se añade comando xtask.

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| Petición P18.S1 | Cubrir las rutas del router y del handoff | Primera parte mide los tamaños de handoff consultados por routing; no llama a SQLite ni selecciona candidatos | Mantener esta parte pura y determinista; la cobertura integrada queda explícitamente pendiente |

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|
| — | — | Ninguna | — |

## Métricas
Comando de baseline arriba; conteos completos en `docs/research/tokens-p18-base.md`.

## Pruebas
- Comando: `cargo nextest run -p symphony-context p18_routing_handoff_token_baseline --run-ignored only --no-capture` → 1 passed.
- Gates de fase pendientes al cierre de este borrador.

## Estado final
Parte 1 establece una medición de handoff fija para cuatro cargas que representan rutas de routing y los cuatro modos del ensamblador. No se ha modificado comportamiento. Falta verificar los crates completos y completar la medición integrada de routing si se requiere antes de cerrar S1.

## Notas para el siguiente agente
El comando exacto para reproducir la tabla está en `docs/research/tokens-p18-base.md`. No lanzar CLIs reales; el benchmark es sintético y local.
