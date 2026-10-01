# P11 · Proveedores restantes

| Campo | Valor |
|---|---|
| Estado | EN CURSO (adelantada antes de P08–P10 por ADR-0008) |
| Rama | phase/p11-proveedores |
| Inicio / cierre | 2026-10-01 / — |
| Agentes que trabajaron | claude-code/sonnet-5.5 (S1–S2) |
| Tag | p11-done (pendiente) |
| Docs usados | STACK §18.4–§18.6, `docs/research/cli-p11.md`, ADR-0008 |

## Pasos

### P11.S1 · Investigación — ✅
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-01
- **Qué se hizo:** reconocimiento por `--help` y pruebas en vivo autorizadas por Leo (un turno, un `resume` y un modelo inexistente por CLI; `--session-id` en Copilot). Los tres hacen headless con salida estructurada y `resume` conserva el contexto.
- **Archivos clave:** `docs/research/cli-p11.md`, `fixtures/providers/{kimi,copilot,antigravity}/`
- **Cómo se verificó:** 7 turnos reales con exit 0; las fixtures no contienen rutas ni usuario.
- **Pendiente / notas:** ToS de los tres (Leo); hooks/Test C no investigado (no hace falta para el chat); errores de cuota solo sintéticos; eventos de herramientas sin observar.

### P11.S2 · Adapter Kimi — 🟡 (falta que el runtime lea el id de sesión, ADR-0009)
- **Agente:** claude-code/sonnet-5.5 · **Fecha:** 2026-10-01
- **Qué se hizo:** crate `symphony-adapter-kimi` (id de proveedor `moonshot`): `spawn`/`resume`/`attach` con `-S`, prompt por stdin, eventos de texto y herramientas (Shell → comando, `WriteFile`/`StrReplaceFile` → edición), `parse_error`, modelo `moonshot/default` (sin `-m`; los modelos salen de la config de Kimi). Registrado en `providers.rs`. Sin dependencias nuevas.
- **Archivos clave:** `crates/adapters/kimi/{src/lib.rs,tests/kimi.rs}`, `crates/daemon/src/providers.rs`, `fixtures/providers/kimi/`
- **Cómo se verificó:** `cargo xtask check` → 257 passed (7 nuevos: suite de contrato con fixtures reales, eventos de herramientas, specs, modelo por defecto).
- **Pendiente / notas:** el id de sesión solo sale por stderr y el executor descarta stderr → **sin ADR-0009 un chat con Kimi no puede retomar la sesión**; skills de Kimi (prefijo y catálogo) sin investigar; live L3 sin correr (necesita el cambio anterior); `parse_error` de cuota/auth es sintético.

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|
| Turno headless estructurado en Kimi, Copilot y Antigravity | live mínimo | ✅ |
| `resume` conserva contexto en los tres | live: «¿qué palabra te pedí?» → `ok` | ✅ |
| Copilot acepta `--session-id` para un UUID nuevo | live | ✅ |
| Adapter de Kimi pasa la suite de contrato con salidas reales | `cargo nextest run -p symphony-adapter-kimi` | ✅ 7/7 |

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|
| Kimi da el id de sesión solo por stderr y el executor lo descarta | sin id no hay `resume` en el chat | `kimi --print -p x` | ADR-0009 |
| Copilot cambia de modelo al retomar con `auto` | rompe «modelo exacto» | `copilot --resume=<id> -p x` sin `--model` | S4: pasar siempre `--model` |

## Decisiones tomadas
- ADR-0008: P11 antes de P08–P10.

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| PLAN P11.S1 | tres archivos `cli-*.md` | uno solo, `cli-p11.md` | contrato común |
| PLAN P11.S1 | mini Test C por CLI | no hecho | el chat no lo necesita (ADR-0008 §2) |

## Dependencias agregadas
Ninguna.
