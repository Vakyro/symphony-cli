# Agentes desatendidos: energía, cuota agotada y cierre de la terminal

- **Fecha:** 2026-10-11
- **Estado:** investigación. No es una decisión; las opciones de diseño pasan a un ADR antes de implementarse.
- **Relacionado:** [ADR-0013](../adr/0013-p08-agentes-con-plan-y-delegacion.md) (P08), [ADR-0002](../adr/0002-process-layer.md) (capa de procesos), P10 (salud, router y failover).

## Preguntas de Leo

1. ¿Los agentes pueden seguir trabajando con la computadora encendida y solo la pantalla apagada?
2. Cuando un agente se queda sin cuota, ¿se le puede preguntar al usuario si cambia de agente/modelo, o si reinicia la tarea cuando la cuota vuelva?
3. Si dejo agentes trabajando, ¿hace falta tener la terminal o Symphony abiertos?

## Respuestas cortas

| Pregunta | Respuesta | Certeza |
|---|---|---|
| 1. Pantalla apagada | Sí. Apagar solo la pantalla no suspende la CPU ni los procesos. Lo que sí los detiene es la **suspensión del sistema** (Windows: «Suspender después de…» en el plan de energía). Symphony hoy no evita la suspensión. | Alta para la pantalla; la suspensión no está probada en Symphony |
| 2. Cuota agotada | **Failover automático** ya existe (P10: `needs_failover`, cambio al mejor modelo del router). **Preguntar** al usuario no existe. **Reiniciar al volver la cuota** no existe, aunque el dato sí se guarda (`reset_at` y `retry_after_at` en la salud del proveedor). | Alta (leído en el código; no probado en vivo por cuota) |
| 3. Terminal abierta | No hace falta. El daemon se lanza desacoplado (P02: `DETACHED_PROCESS` en Windows, `process_group(0)` en Unix), así que cerrar la terminal o la TUI no detiene a los agentes. **Sí** se detienen con cerrar sesión de Windows, reiniciar o apagar la computadora, porque no hay arranque automático. | Alta para cerrar la terminal; reinicio y cierre de sesión no probados |

## Estado actual, con referencias

- **Daemon desacoplado:** `crates/cli/src/client.rs` lo arranca si no está corriendo (`symphony` autoarranca el daemon, P02). Cerrar la TUI no mata al daemon.
- **Sin prevención de suspensión:** no hay llamadas a `SetThreadExecutionState` (Windows) ni `caffeinate` (macOS) en el código. Un agente en curso puede quedar congelado si el sistema suspende.
- **Qué pasa al despertar:** el executor vigila el latido de cada run. Si el proceso estuvo congelado y el latido se corta, el run termina con `NO_HEARTBEAT`. Pero F5 (`death_cause`) busca primero un límite de proveedor reciente; si no lo hay, queda `NO_HEARTBEAT` y el agente pasa a `FAILED` con un item en el Recovery Center. **Esto no lo he probado con una suspensión real.**
- **Cuota:** `crates/daemon/src/health.rs` guarda `reset_at` y `retry_after_at` por ventana. Nada lee esos campos para reactivar un agente. `resume` (F6) es manual.
- **Motivos de cierre:** `QUOTA_EXHAUSTED` y `RATE_LIMITED` ya están en `RunEndReason`. Falta un estado o un motivo para «esperando decisión del usuario».

## Opciones de diseño

### A. Preguntar al usuario cuando se agota la cuota

- El agente pasa a un estado de espera (`WAITING_RESOURCE` o `BLOCKED` con un item de Recovery) con las opciones: **cambiar a otro modelo** (lista del router, con su razón) o **esperar al reinicio de la cuota**.
- La pregunta se guarda en la base de datos, así que sobrevive a cerrar la TUI. La respuesta llega por la TUI o por `symphony`.
- **Costo:** el usuario tiene que estar atento. Sin una notificación fuera de la TUI, la pregunta espera hasta que alguien la vea.
- **Cambio de core:** un estado o motivo nuevo exige ADR (CLAUDE.md, «Adding a state»). Puede resolverse con un item de Recovery del tipo `QUOTA_DECISION`, sin estado nuevo, para evaluar primero.

### B. Reiniciar la tarea cuando vuelva la cuota

- El daemon programa un `resume` (F6) para `reset_at` del proveedor que se agotó. Reusa `resume` y `reclaim`; no hay lógica nueva de checkpoints.
- **Condición:** el daemon tiene que estar vivo a esa hora. Si la computadora está suspendida, el temporizador no corre. Opciones:
  - Windows: `powercfg` / «Activar temporizadores de reactivación» en el plan de energía, o una tarea programada con «Despertar el equipo para ejecutar esta tarea». Requiere configuración del usuario.
  - macOS: `pmset schedule wake`, también con permiso del usuario.
- **Recomendación:** reiniciar automático solo si el usuario lo activó por agente o por proyecto, con un límite de reintentos.

### C. Symphony sin terminal (desatendido)

- **Ya cubierto:** cerrar la TUI o la terminal.
- **Falta:** arranque automático al iniciar sesión (Windows: entrada en `Inicio` o tarea programada «Al iniciar sesión»; macOS: `launchd`; Linux: `systemd --user`).
- **Falta:** evitar la suspensión mientras haya agentes en curso (`ES_SYSTEM_REQUIRED` en Windows, `caffeinate` en macOS), configurable porque consume batería.
- **Falta:** notificación fuera de la TUI cuando hay una decisión pendiente (toast de Windows o notificación de macOS).

## Recomendación

1. **Primero, probar la suspensión** en la máquina real: arrancar un agente con `fake-agent` o un proveedor gratuito, suspender el equipo 5 minutos, despertarlo y ver qué pasa con el run, el latido y el Recovery Center. Sin esto no se sabe cuánto de C hace falta.
2. **Después, A (pregunta al usuario)** como Recovery item, sin cambio de core: es lo que más evita trabajo perdido y no depende del reloj.
3. **Luego, B (reinicio automático)** solo como opción por agente, con temporizador y sin despertar la computadora por defecto.
4. **C completo** (arranque automático, anti-suspensión y notificaciones) va al final de P08 o en una fase propia, porque toca cada sistema operativo.

## Lo que necesito de Leo

1. ¿Quieres que la pregunta de cuota sea un Recovery item (A sin cambio de core) o un estado nuevo con ADR?
2. ¿El reinicio automático (B) debe estar apagado por defecto?
3. ¿Permiso para despertar la computadora con temporizadores del sistema, o solo reactivar cuando ya esté encendida?
4. ¿Prefieres que la prueba de suspensión (paso 1) se haga con un proveedor gratuito de OpenCode o con `fake-agent`? (Con `fake-agent` no cuesta cuota.)

## Lo que no está verificado

- Comportamiento real de una suspensión de Windows con un run en curso.
- Que `needs_failover` cambie de modelo en una cuota agotada real (se verificó con pruebas de P10 con `fake-agent`, no con un proveedor sin cuota).
- Si el daemon sobrevive a cerrar sesión de Windows.
