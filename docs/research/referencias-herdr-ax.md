# Referencias externas: Herdr y AX

Registro de conversaciones entre Leo, ChatGPT y Claude (2026-09) sobre repos similares y sobre el estado de Symphony. Es análisis de terceros para alimentar el ADR de P07.S10. **No son decisiones aprobadas ni cambios al spec.** Las afirmaciones sobre los repos vienen de esas conversaciones; solo una de las respuestas de Claude dice haber verificado el repo de Herdr y AX, y no se re-verificó aquí.

Regla acordada en las conversaciones: usar ambos como **referencia de ingeniería**, no como dependencia ni fork. Congelar el diseño hasta tener datos del uso real (P07.S10) y aplicar los cambios una sola vez, vía un único ADR "Referencias: Herdr y AX".

## 1. Herdr (github.com/herdrdev/herdr)

**Qué es:** runtime en Rust (Apache 2.0) donde viven los agentes de código: servidor local con terminales persistentes, detach/reattach, detección de estado (`working` / `blocked` / `idle` / `done`), CLI + socket API, plugins como procesos externos, worktrees, restore de sesiones nativas (`claude --resume`, `codex resume`, etc.). Stack casi idéntico al de Symphony (Tokio, Ratatui, Crossterm, `interprocess`, `portable-pty`, `windows-sys`).

**Diferencia de fondo:** Herdr es centrado en terminal/proceso (el agente es un proceso en un pane). Symphony es centrado en trabajo (`AGENT ≠ PROCESS ≠ MODEL ≠ CLI`). Su diferencial defendible: continuidad entre proveedores, contexto/handoff, routing por cuota/salud, scheduler de recursos, validación.

**Decisión de las conversaciones:** no usar Herdr como backend ni hacer fork. Estudiarlo como referencia.

### Ideas a adoptar como principios
- **Server owns runtime, clientes solo presentan/controlan** (`symphonyd` autoridad; TUI, futura GUI y automatización son clientes).
- **State ≠ runtime:** estructuras de datos separadas de PTY/proceso/conexiones.
- **Código de plataforma aislado** (`platform/{windows,linux,macos}`); el core solo ve traits.
- **Observabilidad por capas con autoridad ordenada:** eventos estructurados > hooks > estado del proceso > parseo de stdout > detección por pantalla (fallback).
- **Protocolos versionados** con negociación cliente/daemon (ya existe en protocol v1).
- **Plugins como procesos externos** con manifiesto; tratarlos como código con los privilegios del usuario (mostrar qué ejecutan antes de instalar).
- **Restauración por niveles:** (1) daemon vivo, (2) proceso vivo, (3) resume nativo del CLI, (4) reconstrucción entre proveedores con checkpoint. Hoy el spec salta al nivel 4; el nivel 3 es más barato y fiel cuando el CLI murió por crash y no por cuota (`agent_runs.cli_session_id` ya existe).
- **Persistencia defensiva:** nunca destruir el último estado bueno (backup antes de migraciones, integrity check).
- **Regla de rendimiento "por evento × por agente":** camino barato por evento; trabajo pesado (diff, contexto, DB) diferido y agrupado. Candidato a `CONSTRAINTS.md`.
- **No actualizar automáticamente con agentes corriendo.**

### Matices / qué no copiar
- Headless tiene costo: "entrar a ver al agente" es ver el stream renderizado por Symphony, no la TUI real del CLI; algunos CLIs se comportan distinto en headless. Probar ambos modos por proveedor (ADR-0005 ya decidió headless/pipes para v0.1).
- Detección por pantalla con manifests TOML: buena idea, pero no para v1; dejarla para P11 y solo para CLIs sin hooks suficientes.
- No competir en terminal multiplexer, tabs/panes, mouse, SSH/multi-máquina ni marketplace de plugins.
- Licencia: estudiar es libre; si se copia código, conservar aviso y atribución Apache 2.0.
- Idea extra: Herdr trae una skill para que agentes lo controlen; Symphony podría exponer una skill `symphony` (consultar checkpoint, pedir contexto, marcar bloqueo).

### Discrepancia sobre ProcessKit
ChatGPT sugirió primero reemplazar ProcessKit por supervisor propio (`tokio::process` + `portable-pty` + `windows-sys`/`nix` + `sysinfo`). En la revisión final del repo retiró esa sugerencia: **ADR-0002 ya validó ProcessKit** (kill del árbol, 200k líneas de streaming, overhead comparable, Job Objects en Windows) y está aislado tras `crates/process`. Postura vigente: mantener ProcessKit; reevaluar solo si aparece un problema real.

## 2. AX (github.com/google/ax)

**Qué es:** orquestador declarativo de workloads de agentes para clusters (Kubernetes, Agent Substrate, Redis, gRPC, contenedores). Recursos: `Task`, `Workspace`, `Model`; comandos `apply/get/describe/watch/suspend/resume`. La API aún cambiará antes de estable.

**Decisión de las conversaciones:** no usar su infraestructura (K8s, Redis, gRPC, contenedores por agente, secretos, telemetría externa). Sí adoptar ideas de modelado.

### Ideas a adoptar
1. **Suspend / resume + suspensión por inactividad.** Considerada la mejor idea: un agente suspendido solo cuesta estado en disco (p. ej. 10 agentes, 3 vivos, 7 suspendidos). Flujo: flush de checkpoint y contexto, guardar session id del CLI, parar el executor, liberar RAM; al reanudar, mismo CLI o incluso otro executor. Tentativamente: suspend manual en P06, automático (`idle_suspend_after`) en P08. Depende de que cada CLI pueda hacer resume nativo; si no, el resume degrada a handoff. Medir por CLI cuánto cuesta en tokens recargar la sesión.
2. **`phase` + `conditions` en lugar de 12 estados exclusivos.** Híbrido propuesto: `phase` corta (`PENDING/CREATED`, `ACTIVE/RUNNING`, `SUSPENDED`, `COMPLETED`, `FAILED`, `CANCELLED`) + tabla `agent_conditions` (WorkspaceReady, ExecutorReady, ProviderAvailable, ResourceAvailable, DependenciesReady, ValidationPassed, NeedsInput). El estado visible (`WAITING_PROVIDER`, etc.) se deriva con regla de prioridad. Cambia DB §3.C y FLOW §7; decidir antes de P08.
3. **`Workspace` como entidad propia:** worktree, branch, cwd, entorno, skills, MCP y estado de dependencias preparados una vez y heredados al cambiar de executor. Separar "Workspace Setup" de "Executor Runtime". `worktrees` pasaría a `workspaces`.
4. **`TaskSpec` vs `TaskStatus` con revisiones:** `task_revisions`; cada `agent_run` apunta a la revisión que ejecutó.
5. **Health vs readiness:** un executor puede estar vivo pero no listo (workspace preparando, hooks instalándose, sesión restaurándose).
6. **Process group con SIGTERM → gracia → SIGKILL** como requisito explícito de `terminate_tree` (periodo de gracia explícito).

### Diferidas o descartadas
- **Un `Runner` genérico + 5 adapters** en lugar de `ClaudeRunner`, `CodexRunner`… (evita duplicar piezas).
- Fork de agentes desde un checkpoint (`/fork 4 --model ...`) y modo declarativo (`symphony apply symphony.yaml`): después de v1.0.
- AX prefiere Tasks baratas y árboles de tasks; Symphony mantiene **Agent persistente** (`Task` = trabajo, `Agent` = dueño persistente, `Run` = intento). No copiar en esto.

## 3. Evaluación del repo de Symphony (ChatGPT)

Veredicto: **buen camino**; la hipótesis central está validada con evidencia.

**Fortalezas:** ADR-0004 (6/6 handoffs Claude↔Codex, incluyendo kills durante edición y tests); `AGENT ≠ MODEL` reflejado en el esquema (el modelo vive en `agent_run`); crates bien separados; process layer probado y aislado (ADR-0002); headless/pipes sin PTY (ADR-0005); `HandoffAssembler` determinista donde el worktree manda sobre el checkpoint; lints estrictos y suite de contrato de adapters; daemon idle 16.5 MB frente a presupuesto de 100 MB.

**Puntos a vigilar (candidatos para el ADR de S10):**
1. **Modelo de estado del Agent:** 12 estados exclusivos escalan mal con P08 (dependencias, scheduler, validación) y P10 (salud de proveedores). Evaluar `phase + conditions`. No cambiar antes del gate.
2. **Event bus incompleto:** los cambios de estado de un agente no pasan por el bus (la TUI sondea cada 3 s, ya anotado en STATUS). Volverlo consecuencia normal de toda mutación de dominio antes del scheduler.
3. **Archivos grandes:** `crates/store/src/repo.rs` (~52 KB), `crates/tui/src/app.rs` (~38 KB), `crates/tui/src/ui.rs` (~31 KB). Dividir por entidad (`store/`) y por pantalla (`tui/screens/`) antes de que P08–P11 los engorden; no refactorizar de forma preventiva.
4. **README:** distinguir "arquitectura objetivo" de "implementado en v0.1" (scheduler, context engine completo, failover automático y más providers siguen pendientes).
5. **Uso real:** no saltarse las 2 semanas. Añadir la métrica **duración de idle por agente** para valorar suspensión de executors idle antes que un scheduler sofisticado.

**Preguntas que el uso real debe responder:** frecuencia y motivo de los handoffs, contexto que realmente necesitan, qué molesta más (RAM, CPU, UX), cuántos agentes activos se usan, si worktrees y attach se sienten naturales, fallos con repos grandes, cuándo se quiere suspend/pause/stop, qué información de la TUI se usa y cuál nunca.

## 4. Cambios propuestos a documentos (pendientes de ADR)

| Documento | Cambio propuesto |
|---|---|
| IDEA §2 | Principios: server owns runtime, state ≠ runtime, restore por niveles, suspend barato, camino barato por evento |
| DB | `phase` + `agent_conditions`; `worktrees` → `workspaces`; `task_revisions`; estado `SUSPENDED` |
| FLOW §7 | Estado visible derivado de conditions; acción Suspend/Resume; mostrar nivel de restauración usado |
| STACK | Periodo de gracia explícito en terminate; (ProcessKit se mantiene por ADR-0002); `vt100` solo si hace falta detección por pantalla (P11) |
| PLAN P00.S7 | Nota de investigación de Herdr (este archivo) |
| PLAN P01 | Probar headless vs PTY por CLI y costo del resume nativo |
| PLAN P05/P06 | `resume` por adapter; política de restauración nivel 3 → nivel 4; suspend manual |
| PLAN P08 | Suspensión por inactividad; event bus completo; conditions |
| PLAN P12 | Revisar el modelo de plugins de Herdr antes de fijar el manifiesto |

Relacionado: [uso-v0.1.md](uso-v0.1.md) (observaciones de uso y propuesta de chat agéntico con failover entre proveedores).
