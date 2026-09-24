# Symphony

> Un solo terminal para dirigir a todos tus agentes de código.

[![CI](https://github.com/Vakyro/symphony-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/Vakyro/symphony-cli/actions/workflows/ci.yml)
![Estado: en construcción](https://img.shields.io/badge/estado-en%20construcci%C3%B3n-orange)
![Rust 1.95+](https://img.shields.io/badge/rust-1.95%2B-blue)
![Licencia MIT](https://img.shields.io/badge/licencia-MIT-green)

Symphony es un runtime local, escrito en Rust, que coordina los CLIs oficiales de IA para código (**Claude Code, Codex, Antigravity, Kimi Code y Copilot**) desde una sola terminal.

**El problema.** Si usas varios de estos CLIs, los tienes en terminales separadas. Cuando a uno se le acaba la cuota, abres otro y le vuelves a explicar todo: la tarea, qué archivos tocaste, qué faltaba. Y si corres tres agentes a la vez, cada uno lanza sus builds y tests y la computadora se traba.

**La idea.** Separar al **agente** del **modelo**.

```text
AGENTE  = tarea + worktree + rama + checkpoints   → lo guarda Symphony
MODELO  = quien lo ejecuta ahora mismo            → se puede cambiar
```

Si Claude se queda sin cuota a media tarea, Symphony le pasa el mismo agente a Codex con un checkpoint que se fue construyendo mientras trabajaba. Codex continúa desde ahí sin que le vuelvas a explicar nada.

> [!WARNING]
> **Todavía no hay nada usable.** El repositorio tiene el esqueleto del workspace y la especificación completa. El avance está en [`docs/progress/STATUS.md`](docs/progress/STATUS.md).

---

## ¿Funciona la idea?

Antes de escribir código hicimos la prueba a mano ([prevalidación](docs/research/prevalidacion.md)): matamos a un agente a media tarea, sin dejarlo limpiar nada, y le pasamos al otro proveedor un prompt armado **solo** con el objetivo, `git diff`, el último comando y el último mensaje del agente.

| Corrida | Muere | Continúa | ¿Reexplicar? | ¿Rehace trabajo? | Tests al final |
|---|---|---|---|---|---|
| Claude → Codex | Claude a los 48 s | Codex | No | No | ✅ 32/32 |
| Codex → Claude | Codex a los 1 min 53 s | Claude | No | No | ✅ 28/28 |

En la misma investigación revisamos las herramientas que ya existen (Claude Squad, Vibe Kanban, Conductor, AgentBridge y otras). Worktrees y agentes en paralelo ya están resueltos. Ninguna hace **handoff a otro proveedor**, ninguna **limita los recursos** de builds y tests, y casi ninguna corre de forma nativa en **Windows**. Symphony se construye para cubrir eso.

## Cómo funciona

```text
 symphony (CLI / TUI)
        │  IPC local: named pipe / unix socket, JSON tipado y versionado
        ▼
 symphonyd (daemon)
 ├── Estado ........ SQLite + Git + object store (BLAKE3 + zstd)
 ├── Event bus ..... hooks de cada CLI normalizados a un solo stream
 ├── Scheduler ..... clases de operación 0–4 + límites del SO (Job Objects / cgroups)
 ├── Checkpoints ... incrementales, escritos antes de fallar, no después
 ├── Context engine  arma el handoff para el siguiente modelo
 └── Adapters ...... Claude Code · Codex · Antigravity · Kimi · Copilot
        │
        ▼
 CLIs oficiales, cada uno en su propio worktree y con su propio login
```

### Principios

- **AGENTE ≠ MODELO.** El agente persiste y el modelo se puede reemplazar.
- **Checkpoint antes de fallar.** El handoff se construye mientras el agente trabaja.
- **Un worktree por agente.** Nunca dos agentes en la misma carpeta.
- **El trabajo pesado se agenda.** Las llamadas de red corren en paralelo; los builds y tests se limitan.
- **Reglas antes que ML.** El router, el scheduler y los parsers son deterministas. No hay LLM en el camino crítico.
- **Local y privado.** Sin telemetría, sin servidor y sin tocar tus credenciales: cada CLI oficial maneja su propio login.

**Qué no es:** no reemplaza a Claude Code ni a Codex, no es un proxy ni un SaaS, no extrae tokens OAuth y no es un enjambre autónomo sin control.

**Métrica de éxito:** 3 agentes, 3 worktrees y 2–3 proveedores a la vez, y la computadora sigue usable.

## Roadmap

| Etapa | Qué trae | Estado |
|---|---|---|
| P00 · Arranque | Repo, workspace, CI, reglas de calidad | ✅ |
| P01 · Spike | Evidencia de hooks, handoff y consumo con Claude Code + Codex | 🟡 en curso |
| P02–P07 · Core → **v0.1** | Daemon, SQLite, worktrees, adapters, checkpoints, handoff, TUI | ⏳ |
| P08 · Multiagente | Scheduler de recursos, DAG, validación | ⏳ |
| P09 · Context engine | Handoff comprimido, MCP de contexto | ⏳ |
| P10 · Failover → **v0.5** | Salud de proveedores, failover automático, profiles | ⏳ |
| P11 · Más proveedores | Antigravity, Kimi, Copilot | ⏳ |
| P12–P13 → **v0.9** | Plugins, endurecimiento | ⏳ |
| P14–P16 → **v1.0** | Sugerencias de modelo, GUI (Tauri), validación final | ⏳ |

El detalle, paso por paso, está en [`PLAN.md`](PLAN.md).

## Compilar

Requisitos: Rust estable (MSRV 1.95) y Git. En Windows también hace falta el workload **"Desarrollo para el escritorio con C++"** de Visual Studio (linker MSVC).

```bash
git clone https://github.com/Vakyro/symphony-cli.git
cd symphony-cli
cargo run -p symphony-cli -- --version

# verificación completa (fmt + clippy + tests)
cargo install --locked cargo-nextest
cargo xtask check
```

## Documentación

| Documento | Contenido |
|---|---|
| [`docs/spec/idea.md`](docs/spec/idea.md) | Visión, principios y arquitectura |
| [`docs/spec/Symphony_CLI_Ideal_Technology_Stack.md`](docs/spec/Symphony_CLI_Ideal_Technology_Stack.md) | Stack tecnológico y qué está prohibido |
| [`docs/spec/symphony_database.md`](docs/spec/symphony_database.md) | Esquema: 42 tablas |
| [`docs/spec/Symphony_CLI_User_Flow_and_Views.html`](docs/spec/Symphony_CLI_User_Flow_and_Views.html) | Flujo de usuario y 35 vistas |
| [`PLAN.md`](PLAN.md) | Plan de implementación y protocolo de trabajo |
| [`CONSTRAINTS.md`](CONSTRAINTS.md) | Reglas de calidad y presupuestos de rendimiento |
| [`docs/adr/`](docs/adr/) | Decisiones de arquitectura |

## Contribuir

El proyecto lo construyen agentes de código siguiendo un protocolo que permite que uno retome el trabajo de otro, la misma idea que Symphony aplica al código. Si quieres contribuir, humano o agente, empieza por [`AGENTS.md`](AGENTS.md).

## Licencia

[MIT](LICENSE)
