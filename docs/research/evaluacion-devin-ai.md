# Evaluación de Symphony CLI y Comparativa Externa (Devin AI)

Registro del análisis y evaluación técnica realizada por Devin AI sobre el proyecto Symphony CLI (Octubre 2026), contrastando arquitectura, calidad de código, documentación, proyectos análogos a nivel mundial y roadmap de mejoras pendientes.

---

## 1. Veredicto General

Symphony CLI se posiciona como un proyecto con un **rigor de ingeniería y documentación excepcional para su estado (v0.1.0+)**, donde la principal virtud no radica únicamente en el código actual, sino en la disciplina de proceso, diseño prevalidado y arquitectura defensiva.

### Fortalezas Destacadas
* **Arquitectura modular desacoplada:** Workspace con 14+ crates con responsabilidades delimitadas (`protocol`, `core`, `daemon`, `store`, `adapters/*`, `tui`, `process`, `testkit`), IPC versionado y bus de eventos interno (`crates/core/src/events.rs`).
* **Lints de máxima exigencia en Rust:** Restricciones duras en workspace (`Cargo.toml`): `unsafe_code = "forbid"`, `unwrap_used = "deny"`, `expect_used = "deny"`, `todo = "deny"`, `unimplemented = "deny"`. Supera el estándar de la mayoría de proyectos open-source en Rust.
* **Stack tecnológico sensato:** Tokio (runtime async), Rusqlite con WAL y migraciones versionadas, object store direccionado por contenido (BLAKE3 + zstd), Ratatui (TUI moderna), Miette/Thiserror para diagnósticos enriquecidos.
* **Cultura de testing real:** Más de 200–400 tests unitarios y de integración, tests CLI con `trycmd`, snapshots de UI con `insta` para 15+ vistas TUI, suite de contrato para adaptadores y un `testkit` aislado sin depender de credenciales reales (`fake-agent`).
* **Documentación de estándar industrial:** Registros de decisiones de arquitectura (ADRs versionados), bitácoras cronológicas por fase (`docs/phases/`), seguimiento vivo de estado y handoffs entre agentes (`docs/progress/STATUS.md`, `LEARNINGS.md`), especificación exhaustiva de 42 tablas y prevalidación empírica antes de escribir la primera línea de código (`docs/research/prevalidacion.md`).
* **Honestidad técnica:** Gates de uso real antes de dar por cerrado un hito, replanificación guiada por evidencia empírica (ADR-0006, ADR-0010) y visibilidad explícita de deuda técnica pendiente.

---

## 2. Mapa Comparativo de Ecosistemas

### A. Dominio: Orquestación de CLIs de IA en Worktrees
* **Proyectos similares:** *Claude Squad*, *Vibe Kanban*, *Conductor*.
* **Diferencial de Symphony:** A diferencia de proyectos TypeScript creados rápidamente sin aislamiento de procesos o contratos estrictos, Symphony implementa:
  1. Handoff contextual y preservación de historial entre proveedores heterogéneos.
  2. Límites y aislamiento de recursos a nivel de sistema operativo (Job Objects en Windows, cgroups en Linux).
  3. Soporte nativo de primer nivel en Windows (escapado de batch, shims de npm, pipes, control de handles).
  4. Core desacoplado de dependencias de red o SDKs cerrados.

### B. Arquitectura y Disciplina de Código
* **Proyectos análogos:** *rust-analyzer*, *typst*.
* **Puntos en común:** Separación daemon/servidor e IPC tipado, `xtask` como automatizador de desarrollo, lints en deny, diseño orientado a datos sin estado global compartido.
* **TUI:** Inspirada en la ergonomía y reactividad de *lazygit*, *k9s* y *helix* usando Ratatui.

### C. Espacio de Agentes Autónomos
* **Proyectos análogos:** *OpenHands*, *Aider*.
* **Diferencia conceptual:** Aider y OpenHands construyen o integran el loop del modelo directamente en su proceso. Symphony es un **runtime/meta-orquestador**: `AGENT ≠ PROCESS ≠ MODEL ≠ CLI`. Coordina los CLIs oficiales/externos del usuario (Claude Code, Codex, Antigravity, Kimi, Copilot) respetando sus contratos y entornos de ejecución.

---

## 3. Comparativa con Repositorios Consolidados

| Proyecto | Ventajas / Madurez frente a Symphony | Posición de Symphony |
|---|---|---|
| **lazygit / gitui** | Años de edge cases resueltos en terminales exóticas, releases empaquetados multiplataforma, base amplia de usuarios. | Symphony comparte el estándar de calidad TUI e IPC, pero se encuentra en fase temprana de producto. |
| **helix / typst** | Fuzzing continuo, property-based testing exhaustivo, comunidad y ecosistema de plugins. | Symphony iguala el rigor de lints, arquitectura de crates y testing automatizado en CI. |
| **Vibe Kanban / Claude Squad** | Mayor rapidez inicial al basarse en stacks web/TS. | Symphony supera ampliamente en solidez de ingeniería, seguridad de procesos, rendimiento de memoria (daemon ~16 MB) y ausencia de crashes no controlados. |

---

## 4. Oportunidades y Mejoras Pendientes

1. **Failover Automático (Killer Feature):**
   * *Estado:* El handoff existe de forma determinista y manual/con reattach, pero la transición transparente y desatendida ante agotamiento de cuota o rate limit (`KNOWN` / `UNKNOWN`) es la funcionalidad clave que consolidará la propuesta de valor.
   * *Acción:* Implementar el pipeline de failover reactivo planificado en P10 / P08.

2. **Scheduler de Recursos del Sistema:**
   * *Estado:* Diseñado conceptualmente en el spec (Clases 04, asignación de prioridades, límites de CPU/RAM mediante Job Objects en Windows y cgroups en Linux).
   * *Acción:* Completar la implementación en la fase P08.

3. **Event Bus Reactivo en TUI (Eliminación de Polling):**
   * *Estado:* Ciertas vistas de la TUI realizan sondeo periódico (ej. polling cada 3 s) para refrescar datos.
   * *Acción:* Migrar todas las vistas a suscripciones directas de eventos push a través del bus interno (`AgentStateChanged`, `TaskUpdated`, `MetricsEmitted`).

4. **Canales de Distribución y Empaquetado:**
   * *Estado:* Requiere clonar y compilar localmente con `cargo`.
   * *Acción:* Añadir flujos de empaquetado binario precompilado en GitHub Releases, soporte para `cargo-binstall`, y manifiestos de instalación para gestores de paquetes (`scoop`, `winget`, `homebrew`).

5. **Completitud de Tablas y Repositorios en Store:**
   * *Estado:* Tablas como `executor_changes`, `messages`, `tool_calls`, `handoffs` y `recovery_items` requieren persistencia y consumo integral.
   * *Acción:* Culminar la integración de esquemas y queries durante las fases P09 y P08.

6. **Diversificación de Proveedores y Hardening de Entorno:**
   * *Estado:* Adaptadores de Claude Code y Codex completamente operativos; Kimi, Antigravity y Copilot integrados en P11 con permisos y ToS pendientes de configuración por el usuario.
   * *Acción:* Parametrizar flags de sandbox y permisos en `config.toml`.

7. **Mitigación del Bus Factor:**
   * *Estado:* Proyecto desarrollado por un solo mantenedor humano (Leo) en colaboración con agentes de IA.
   * *Acción:* Mantener la exhaustividad en ADRs y bitácoras para que cualquier colaborador o agente pueda retomar el contexto sin pérdida de diseño.

8. **Control y Supervisión Remota vía SSH (Móvil):**
   * *Estado:* Arquitectura desacoplada (`symphonyd` + TUI Ratatui).
   * *Acción:* Validar y documentar flujos de supervisión y respuesta a agentes desde smartphones vía SSH (Termius/Termux/Prompt sobre Tailscale/SSH en Windows), con miras a posibles notificaciones push/webhooks.

9. **Estrategia de Sesiones Pequeñas (Bajo Impacto en Cuota/Usage):**
   * *Estado:* En desarrollo con las políticas de compresión contextual (P09).
   * *Acción:* Fomentar un modo de ejecución en "micro-sesiones" con objetivos atómicos y entrega condensada, evitando arrastrar historiales crudos excesivos que agoten las cuotas de 5 horas o diarias de los proveedores.


---

## 5. Nivel y Calibre Relativo

* **En proceso, rigor de diseño y documentación:** Equivalente al estado inicial de herramientas de infraestructura modernas como **`jj` (Jujutsu VCS)** o **`zellij`** en sus etapas de 1-2 core developers (diseño previo riguroso, ADRs fundamentados, cero `unwrap` en runtime, decisiones basadas en benchmarks reales).
* **En madurez de producto:** Nivel **"v0.1 honesto"** (pre-producto sólido). No pretende inflar métricas ni vender estabilidad prematura; reconoce sus pendientes y somete cada hito a validación práctica.
