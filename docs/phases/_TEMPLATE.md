# PNN · Nombre de la fase

| Campo | Valor |
|---|---|
| Estado | EN CURSO / CERRADA / CERRADA CON PENDIENTES |
| Rama | phase/pNN-... |
| Inicio / cierre | 2026-.. / 2026-.. |
| Agentes que trabajaron | claude-code/opus (S1–S3), codex/gpt-5.x (S4–S7) |
| Tag | pNN-done (commit abc1234) |
| Docs usados | IDEA §5.3, DB §3.D, STACK §18 |

## Pasos

### PNN.S1 · Título — ✅ / 🟡 / ❌
- **Agente:** codex/gpt-5.x · **Fecha:** 2026-..
- **Qué se hizo:** …
- **Archivos clave:** `crates/...`
- **Cómo se verificó:** `cargo nextest run -p ...` → 34 passed
- **Commits:** abc1234, def5678
- **Pendiente / notas:** …

(repetir por paso)

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|

## Decisiones tomadas
- ADR-NNNN: …

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|

## Métricas
(benchmarks, tiempos, RAM, cobertura — con comando)

## Pruebas
- Comando(s): …
- Totales: N passed, M failed (cuáles y por qué)

## Estado final
Resumen de 3–5 líneas para Leo.

## Notas para el siguiente agente
Trampas, comandos útiles y lo que no vale la pena reintentar.
