# ADR-0003 · Retener comandos por hook (capa 1 del scheduler)

- **Estado:** ACEPTADO
- **Fecha:** 2026-09-24
- **Autor:** claude-code/opus-5.5 · **Aprobado por:** — (resultado de spike; no cambia el alcance)
- **Fase/paso:** P01.S8

## Contexto
IDEA §5.4 define un scheduler en dos capas: (1) los hooks retienen las operaciones pesadas antes de que empiecen, según clases 0–4; (2) límites del SO (Job Objects / cgroups). IDEA §7 pregunta si los CLIs aguantan que un hook `PreToolUse` los haga esperar. DB §3.D tiene `providers.hooks_can_hold`.

## Opciones consideradas
1. Retener siempre por hook, en los dos CLIs.
2. Retener por hook con un límite por proveedor y, por encima, **denegar con razón** (`deny` + "en cola, reintenta en N s").
3. No usar hooks para retener; solo la capa del SO.

## Decisión
Opción 2.

| Proveedor | `hooks_can_hold` | Espera máxima por hook | Por encima del máximo |
|---|---|---|---|
| claude-code | `true` | La espera del scheduler, con el `timeout` del hook **mayor** que esa espera (se fija explícito en el hook; por defecto son 600 s) | `deny` con razón |
| codex | `true` hasta **60 s** | 60 s | `deny` con razón |

Reglas:
- El hook **siempre** responde antes de su `timeout`. En los dos CLIs, un hook vencido **no bloquea**: el comando se ejecuta igual.
- `symphony hook emit` tiene su propio límite interno, menor que el `timeout`, porque Codex no mata los hooks vencidos.
- La capa del SO (ADR-0002) sigue siendo obligatoria: los hooks no cubren todos los caminos (las herramientas alojadas de Codex, por ejemplo) y los hooks pueden fallar.

## Evidencia
`spikes/results/test-c.md`:
- Claude: con 120 s de retención y `timeout` 180 esperó y corrió. Con 60 s de retención y `timeout` 30, el hook se canceló a los ~30 s y el comando corrió igual.
- Codex: 30 s y 60 s OK. Con 120 s el tool call falló después de la espera y el modelo reintentó, lo que duplicó la espera (282 s). Con `timeout` 30, el comando corrió igual y el proceso del hook siguió vivo.
- Ningún modelo notó la retención ni cambió de comportamiento.

## Consecuencias
- DB §3.D: `providers.hooks_can_hold` alcanza para Claude. Para Codex hace falta un máximo por proveedor. Propuesta: una columna o clave de config `hooks_max_hold_secs` (desviación de DB, se registra en P03 al escribir la migración).
- P08.S*: `QueueExplainer` redacta la razón del `deny` para el modelo ("comando en cola detrás de X; reintenta en N s") y el scheduler tiene que tolerar el reintento.
- El umbral de Codex (entre 60 y 120 s) se vuelve a medir con cada versión mayor del CLI (test live en P05).
- **Inyección de hooks (cambia P05.S4–S5):** en lugar de "un archivo local excluido de git en el worktree", los hooks se pasan **por invocación**: Claude con `--settings <archivo-en-~/.symphony>` y Codex con `-c hooks.<Evento>=[…]` + `--dangerously-bypass-hook-trust` (Test B). Codex **no cargó** `<worktree>/.codex/hooks.json`. Así no queda nada escrito en el worktree. El comando del hook usa la sintaxis del shell de cada CLI en Windows: bash en Claude, **PowerShell** en Codex (`& 'ruta' args`).
