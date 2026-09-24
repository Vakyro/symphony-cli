# P01 · Spike de viabilidad

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p01-spike |
| Inicio / cierre | 2026-09-24 / — |
| Agentes que trabajaron | claude-code/opus-5.5 |
| Tag | — |
| Docs usados | IDEA §5.2–§5.6, §7, §8; STACK §7, §40, §60; FLOW §13 |

## Pasos

### P01.S1 · Rama y bitácora — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** rama `phase/p01-spike` desde `main` (aa2e22a) y esta bitácora. Leo dio permiso para usar sus suscripciones en P01 (2026-09-24).
- **Cómo se verificó:** `git branch --show-current` → `phase/p01-spike`.

### P01.S2 · Investigar los contratos de los CLIs — ✅ (ToS pendiente de Leo)
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** documentación oficial + `--help` de Claude Code 2.1.281 y codex-cli 0.154.0. Hooks, timeouts, inyección por worktree, headless, transcript, modelos, errores/cuota y modos de interacción.
- **Archivos clave:** `docs/research/cli-claude-code.md`, `docs/research/cli-codex.md`, `docs/research/tos.md`
- **Cómo se verificó:** citas a docs oficiales y a la salida de `--help`. Lo no probado queda marcado "(docs)" y en la lista "Pendiente de verificar" de cada archivo.
- **Hallazgos clave:** (1) en los dos CLIs un `PreToolUse` vencido **no bloquea**: retener por hook falla en modo abierto; (2) `--bare` de Claude no usa la suscripción, así que no sirve; (3) hooks por sesión con `claude --settings` y, en Codex, `.codex/hooks.json` + `--dangerously-bypass-hook-trust`; (4) errores tipados (`StopFailure`, `api_retry`) y cuota KNOWN en ambos; (5) mensajes a media tarea: `--input-format stream-json` (Claude) y `codex queue` (Codex); (6) Claude tiene `--bg` + `claude attach`.
- **Pendiente / notas:** Leo tiene que confirmar `tos.md`.

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
