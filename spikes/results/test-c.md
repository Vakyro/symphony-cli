# Test C · Retener comandos con `PreToolUse` (P01.S5)

- **Fecha:** 2026-09-24 · **Agente:** claude-code/opus-5.5
- **CLIs:** Claude Code 2.1.281 (`--model haiku`), codex-cli 0.154.0 (`-m gpt-5.6-luna`), en Windows 11.
- **Montaje:** `spike-hook` con `SPIKE_HOLD_SECS` y `SPIKE_HOLD_MATCH="npm test"`. El hook de `PreToolUse` duerme N segundos y luego responde `permissionDecision: "allow"`. Hooks inyectados como en Test B (Claude con `--settings`, Codex con `-c hooks.*`). Tarea: "Run the command: npm test". Los tiempos salen de los `ts_ms` de `events.jsonl`.

## Resultados

| CLI | Retención | `timeout` del hook | ¿Qué pasó? | Duración total |
|---|---|---|---|---|
| Claude | 120 s | 180 s | ✅ **Esperó** los 120 s, luego corrió el comando y terminó bien | 142 s |
| Claude | 60 s | 30 s | ⚠️ Mató el hook a los ~36 s y **corrió el comando igual** (falla en modo abierto) | 49 s |
| Codex | 30 s | 180 s | ✅ Esperó y corrió | 61 s |
| Codex | 60 s | 180 s | ✅ Esperó y corrió | 93 s |
| Codex | 120 s | 180 s | ❌ Después de esperar, **el tool call falló** ("failed at the shell-process boundary"). El modelo reintentó con otra herramienta, **el hook volvió a retener 120 s** y después sí corrió | 282 s |
| Codex | 60 s | 30 s | ⚠️ A los ~38 s corrió el comando igual (falla en modo abierto). **El proceso del hook no se mató**: siguió vivo y mandó su último evento 20 s después de que terminó la sesión | 78 s |

**En ningún caso el modelo se enteró de la espera.** Todos reportaron que el comando "corrió en menos de un segundo" o "en unos 7 segundos". La retención no desvía al modelo ni lo lleva a hacer nada raro, salvo el reintento de Codex cuando la herramienta falla.

## Conclusiones

1. **Claude: `hooks_can_hold = true`**, siempre que la espera sea menor que el `timeout` del hook, que Symphony fija explícitamente (por defecto son 600 s; no se probó un máximo mayor). Si se vence, el comando **se ejecuta igual**: el scheduler tiene que liberar o denegar (`deny` con razón) **antes** del timeout, nunca dejarlo vencer.
2. **Codex: `hooks_can_hold = partial`.** Aguanta hasta ~60 s. Con 120 s, la herramienta de ejecución interactiva falla y el modelo reintenta, lo que duplica la espera. El umbral real está entre 60 y 120 s (no se buscó con más precisión). Para esperas largas en Codex hay que **denegar con razón** ("comando en cola, reintenta en N s") en lugar de retener, o recurrir a la capa del SO.
3. **Un hook vencido no se mata del mismo modo en los dos CLIs.** Claude lo cancela; Codex lo deja corriendo. El hook de Symphony (`symphony hook emit`) tiene que tener su propio límite interno menor que el `timeout`, para no dejar procesos colgados.
4. Consecuencia para P08: la capa 1 del scheduler (retener por hook) sirve para colas cortas: ≤ 60 s en Codex y hasta el `timeout` en Claude. Las esperas largas se hacen con `deny` + razón, o con la capa 2 (Job Objects / cgroups).

## Valor para `providers.hooks_can_hold` (DB §3.D)

| Proveedor | Valor | Nota |
|---|---|---|
| claude-code | `true` | Con `timeout` explícito > espera máxima; el scheduler responde antes |
| codex | `false` para esperas > 60 s (usar `deny` + razón) | Se puede tratar como `true` con un límite de 60 s si la DB admite un máximo por proveedor (ver ADR-0003) |
