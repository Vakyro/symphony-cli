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

### P01.S2 · Investigar los contratos de los CLIs — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** documentación oficial + `--help` de Claude Code 2.1.281 y codex-cli 0.154.0. Hooks, timeouts, inyección por worktree, headless, transcript, modelos, errores/cuota y modos de interacción.
- **Archivos clave:** `docs/research/cli-claude-code.md`, `docs/research/cli-codex.md`, `docs/research/tos.md`
- **Cómo se verificó:** citas a docs oficiales y a la salida de `--help`. Lo no probado queda marcado "(docs)" y en la lista "Pendiente de verificar" de cada archivo.
- **Hallazgos clave:** (1) en los dos CLIs un `PreToolUse` vencido **no bloquea**: retener por hook falla en modo abierto; (2) `--bare` de Claude no usa la suscripción, así que no sirve; (3) hooks por sesión con `claude --settings` y, en Codex, `.codex/hooks.json` + `--dangerously-bypass-hook-trust`; (4) errores tipados (`StopFailure`, `api_retry`) y cuota KNOWN en ambos; (5) mensajes a media tarea: `--input-format stream-json` (Claude) y `codex queue` (Codex); (6) Claude tiene `--bg` + `claude attach`.
- **Pendiente / notas:** Leo aceptó `tos.md` para uso personal (2026-09-24).

### P01.S3 · Test A: recursos — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `spikes/spike-resources` (workspace aparte en `spikes/`) lanza 1–3 CLIs en worktrees de un repo Vite+pnpm y muestrea el árbol de procesos con sysinfo. Se midió también `pnpm install` en frío, con store compartido y `npm install`.
- **Archivos clave:** `spikes/spike-resources/src/main.rs`, `spikes/results/test-a.md`, `spikes/results/test-a.raw.md`
- **Cómo se verificó:** 8 corridas (Claude y Codex × N=1,2,3, más la repetición de N=1).
- **Resultado:** ~200–350 MB reales por agente; con 3 agentes, +~1 GB y CPU media del 80 % solo en el arranque. El arranque de Claude se degrada de 5 s a 18 s. pnpm con store compartido: 2.2 s por worktree.
- **Dependencias agregadas (solo en spikes):** `sysinfo 0.39.6` (STACK §58).

### P01.S4 · Test B: event bus de hooks — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `spikes/spike-hook` (collect + hook) con `interprocess` y `serde_json`. Hooks inyectados sin tocar el repo: Claude con `--settings`, Codex con `-c hooks.*` + `--dangerously-bypass-hook-trust`.
- **Archivos clave:** `spikes/spike-hook/src/main.rs`, `spikes/results/test-b.md`
- **Cómo se verificó:** una sesión de cada CLI produce `ToolRequested`/`CommandRequested`, `CommandFinished`, `FileModified` y `TurnFinished` en `events.jsonl`. Sin `SYMPHONY_AGENT_ID`: 0 eventos. Latencia del hook: mediana 20 ms.
- **Hallazgos:** Codex corre los hooks en PowerShell (las rutas con espacios necesitan `&`); `.codex/hooks.json` del proyecto no carga; `exec --json` no trae cuota (hay que leer el rollout); Claude trae `rate_limit_event` con 5h/7d.
- **Dependencias agregadas (solo en spikes):** `interprocess 2.4`, `serde_json 1.0.151` (STACK §58).

### P01.S5 · Test C: retener comandos — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `SPIKE_HOLD_SECS`/`SPIKE_HOLD_MATCH` en `spike-hook`; 6 corridas (Claude 120/180 y 60/30; Codex 30/180, 60/180, 120/180 y 60/30).
- **Archivos clave:** `spikes/results/test-c.md`, `spikes/results/test-c.raw.md`, `spikes/scripts/test-c.ps1`
- **Resultado:** Claude retiene hasta el `timeout` y si se vence falla en modo abierto. Codex retiene bien ≤ 60 s; con 120 s el tool falla y el modelo reintenta. Codex no mata los hooks vencidos. Valor propuesto: `hooks_can_hold` claude=true, codex=false para > 60 s.

### P01.S7 · Gate de ProcessKit — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-24
- **Qué se hizo:** `spikes/spike-process` prueba kill del árbol, drop vía wrapper, streaming de 200k líneas, overhead de spawn, límites (memoria, CPU, procesos) y suspend/resume. Corre local (Windows ×3) y en CI (workflow `spike-processkit`, 3 OS).
- **Archivos clave:** `spikes/spike-process/src/main.rs`, `spikes/results/test-processkit.md`, `docs/adr/0002-process-layer.md`, `.github/workflows/spike-processkit.yml`
- **Resultado:** kill, streaming y overhead ✅ en los 3 OS. Límites ✅ en Windows (Job Object); en Linux sin cgroup delegado y en macOS, error tipado. Decisión: ProcessKit detrás de `ProcessSupervisor` (ADR-0002).
- **Nota:** dos filas ❌ de la primera corrida en CI eran errores de conteo del spike (hilos/zombies en Linux, marcador en el wrapper). Se corrigió la medición, no ProcessKit.
- **Dependencias agregadas (solo en spikes):** `processkit 3.3` (features `limits`, `stats`), `tokio 1.53` (STACK §58).

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
