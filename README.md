# Symphony CLI

> Un solo terminal para dirigir a todos tus agentes de código.

[![CI](https://github.com/Vakyro/symphony-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/Vakyro/symphony-cli/actions/workflows/ci.yml)
![Estado: v0.1.0 (usable)](https://img.shields.io/badge/estado-v0.1.0%20usable-brightgreen)
![Rust 1.95+](https://img.shields.io/badge/rust-1.95%2B-blue)
![Licencia MIT](https://img.shields.io/badge/licencia-MIT-green)

**Hoy en `main`: hasta P15.5** (multiagente, failover, context engine, plugins, sugerencias, GUI y daemon por proyecto); P13 · endurecimiento está en curso. Abres `symphony` en cualquier carpeta, conversas con Claude Code, Codex, Kimi Code, Antigravity, Copilot u OpenCode y puedes pasar de uno a otro a mitad de conversación sin repetir nada. Con el harness de Symphony (por defecto) todos los modelos trabajan con las mismas herramientas.

Symphony es un runtime local, escrito en Rust, que coordina los CLIs oficiales de IA para código desde una sola terminal. Hoy funcionan **Claude Code, Codex, Kimi Code, Antigravity (`agy`), Copilot y OpenCode**. Los últimos cuatro llegaron en P11: el cambio de proveedor con handoff está probado con CLIs reales, pero sus skills nativas aún no funcionan en el chat y cada uno tiene sus límites de permisos (ver el [QUICKSTART](QUICKSTART.md#proveedores-y-permisos)).

**El problema.** Si usas varios de estos CLIs, los tienes en terminales separadas. Cuando a uno se le acaba la cuota, abres otro y le vuelves a explicar todo: la tarea, qué archivos tocaste, qué faltaba. Y si corres tres agentes a la vez, cada uno lanza sus builds y tests y la computadora se traba.

**La idea.** Separar al **agente** del **modelo**.

```text
AGENTE  = tarea + worktree + rama + checkpoints   → lo guarda Symphony
MODELO  = quien lo ejecuta ahora mismo            → se puede cambiar
```

Si Claude se queda sin cuota a media tarea, Symphony le pasa el mismo agente a Codex con un checkpoint que se fue construyendo mientras trabajaba. Codex continúa desde ahí sin que le vuelvas a explicar nada.

> [!NOTE]
> **Usable hoy, camino a v0.9.** Compila o instala, abre `symphony` y conversa. El roadmap está en [`docs/progress/STATUS.md`](docs/progress/STATUS.md) y el detalle completo en [`PLAN.md`](PLAN.md).

---

## El chat

La vista inicial de `symphony` es un chat agéntico, como el de Claude Code o Codex, con la ventaja de que el contexto no se queda atado a un proveedor.

- **Cambia de proveedor a mitad de conversación.** Elige otro modelo al enviar un mensaje y el nuevo proveedor recibe la conversación, el `git status` y el diff del worktree. Si un proveedor se queda sin cuota, el chat pasa solo al siguiente. También hay una política opcional por umbral de tokens, apagada por defecto.
- **Tu trabajo queda en git.** El chat vive en su propio worktree, en la rama `symphony/chat`, con un commit por turno cuando hubo cambios.
- **Skills nativas.** `/skill` en Claude y `$skill` en Codex funcionan dentro del chat, con autocompletado al escribir `/` o `$`. Una skill escrita para un proveedor no se traduce al otro, y lo que una skill deja en el contexto de la sesión no viaja en un cambio.
- **Copiar y exportar.** `Ctrl+Y` copia la respuesta, `F2` activa el modo de selección y `Ctrl+E` exporta la conversación completa a Markdown en `.symphony/exports/`.
- **Se abre desde cualquier carpeta** una vez instalado con `cargo install` (ver [`QUICKSTART.md`](QUICKSTART.md)).

**Qué cuesta cambiar de proveedor.** En una prueba real (una sola corrida, modelos baratos, conversación corta), el proveedor de destino leyó unos 20,7k tokens al pasar de Claude a Codex y unos 31,1k al volver a Claude. El handoff de Symphony aportó solo 2,4k–2,9k; el resto es el arranque propio de cada CLI. Son cifras de tamaño de contexto, no de dinero ni de cuota, y falta medir conversaciones largas. Detalle en [`docs/phases/P07.5-chat.md`](docs/phases/P07.5-chat.md).

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
 ├── Checkpoints ... incrementales, escritos antes de fallar, no después
 ├── Handoff ....... arma el prompt para el siguiente modelo (con la conversación)
 ├── Adapters ...... Claude Code · Codex · Kimi · Antigravity · Copilot · OpenCode
 ├── Scheduler ..... clases de operación 0–4, cola explicada y límites del SO (P08)
 ├── Multiagente ... DAG de dependencias, validación por niveles e integración a `symphony/chat` (P08)
 └── Context engine  handoff comprimido y MCP de contexto (P09)
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
| P00 · Arranque | Repo, workspace, CI, reglas de calidad | ✅ p00-done |
| P01 · Spike | Evidencia de hooks, handoff y consumo con Claude Code + Codex | ✅ p01-done |
| P02–P07 · Core → **v0.1** | Daemon, SQLite, worktrees, adapters, checkpoints, handoff, TUI | ✅ **v0.1.0** |
| P07.5 · Chat general | Chat como vista inicial, cambio de proveedor sin perder contexto, skills, exportar | ✅ p075-done |
| P07.S10 | Replanificación de P08–P16 con lo aprendido en el uso (ADR-0007, ADR-0008, ADR-0010: P10 → P09 → P08) | ✅ cerrada |
| P08 · Multiagente | Scheduler de recursos, DAG, plan por fases, validación, integración y revisión | ✅ p08-done (benchmark con 6 proveedores en paralelo aprobado: 202 s, pico de 1,7 GB de RAM, sistema usable) |
| P09 · Context engine | Handoff comprimido, MCP de contexto | ✅ p09-done |
| P10 · Failover → **v0.5** | Salud de proveedores, failover con router, profiles y `explain-route` | ✅ **v0.5.0** |
| P11 · Más proveedores | Kimi, Antigravity, Copilot y OpenCode con el mismo contrato; matriz de handoff entre los proveedores | ✅ p11-done |
| P11.5 · Pendientes | `ctx://` robusto, permisos en config, deltas en el chat, merge chat → `main` con revisión | ✅ p115-done |
| P12 · Plugins | Validators y context transformers como procesos aparte | ✅ p12-done |
| P14 · Sugerencias | Sugerencias de cambio de modelo (nunca se aplican solas), aprendizaje determinista | ✅ p14-done |
| P15 · GUI | App de escritorio (Tauri) sobre el mismo daemon | ✅ p15-done |
| P15.5 · Cambios de rumbo | Daemon y estado **por proyecto**, acceso local total, modelos al día, **harness de Symphony** (mismas herramientas para todos los modelos), proyectos que no son código | ✅ p155-done |
| P13 · Endurecimiento → **v0.9** | Seguridad, fuzzing, recuperación, rendimiento y disco, docs (mdBook), release | 🟡 en curso |
| P16 · Validación final → **v1.0** | Checklist de aceptación, E2E, benchmark, validación por tipo de proyecto | ⏳ |

El detalle, paso por paso, está en [`PLAN.md`](PLAN.md).

## Symphony se construye con Symphony

Desde P13, parte del trabajo lo hacen agentes que Symphony lanza sobre su propio repo: Kimi,
OpenCode (con modelos gratuitos) y Antigravity reciben una tarea con un plan escrito, trabajan en
su worktree y un integrador revisa y mete el resultado. Lo que falla al usarlo se anota y se
arregla en [`docs/progress/DOGFOOD.md`](docs/progress/DOGFOOD.md): así salieron, por ejemplo, una
caída de Kimi en Windows por la codificación de su salida y los worktrees que nunca se borraban.

## Cómo usar

**Guía completa paso a paso:** [`QUICKSTART.md`](QUICKSTART.md) — desde compilar hasta trabajar en proyectos reales.

TL;DR:
```bash
git clone https://github.com/Vakyro/symphony-cli.git
cd symphony-cli
cargo install --path crates/cli --locked
cargo install --path crates/daemon --locked

cd /tu/proyecto
symphony
```

Necesitas al menos uno de los seis CLIs (Claude Code, Codex, Kimi Code, Antigravity, Copilot u OpenCode) instalado y con sesión iniciada; Symphony no toca tus credenciales. Cada proyecto tiene su propio daemon y su estado en `<proyecto>/.symphony/`; si venías de antes, corre `symphony migrate split` una vez.

## Compilar

Requisitos: Rust estable (MSRV 1.95) y Git. En Windows también hace falta el workload **"Desarrollo para el escritorio con C++"** de Visual Studio (linker MSVC).

```bash
git clone https://github.com/Vakyro/symphony-cli.git
cd symphony-cli

# Build release (recomendado para usar)
cargo build -p symphony-cli -p symphony-daemon --release

# O ejecuta directo desde el repo
cargo run -p symphony-cli -- --version

# Verificación completa (fmt + clippy + tests)
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
| [`docs/book/`](docs/book/src/SUMMARY.md) | Libro de uso (mdBook): instalación, configuración, proveedores, uso remoto, problemas y ER |
| [`docs/progress/DOGFOOD.md`](docs/progress/DOGFOOD.md) | Qué pasó al construir Symphony con Symphony |

## Contribuir

El proyecto lo construyen agentes de código siguiendo un protocolo que permite que uno retome el trabajo de otro, la misma idea que Symphony aplica al código. Si quieres contribuir, humano o agente, empieza por [`AGENTS.md`](AGENTS.md).

## Licencia

[MIT](LICENSE)
