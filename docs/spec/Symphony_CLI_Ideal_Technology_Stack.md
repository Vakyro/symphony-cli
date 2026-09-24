# Symphony CLI — Stack tecnológico ideal

> Especificación recomendada del stack completo para construir Symphony CLI, derivada de los documentos funcionales, de flujo de usuario, modelo de datos y diagrama entidad-relación del proyecto.

**Estado:** Recomendación técnica v1  
**Fecha de referencia:** 2026-09-23  
**Objetivo dominante:** fiabilidad → velocidad → bajo consumo de RAM/CPU → concurrencia multiagente → continuidad entre proveedores  
**Plataformas objetivo:** Windows, Linux y macOS  
**Modelo de producto:** local-first, CLI/TUI-first, sin telemetría, sin nube propia  
**Autenticación:** cuentas/suscripciones mediante los CLIs oficiales de cada proveedor  
**Licencia propuesta:** MIT

---

## 1. Decisión ejecutiva

El stack ideal para Symphony debe asumir que **Symphony no es el agente de IA**. Es el runtime que coordina agentes externos.

Eso obliga a optimizar principalmente:

1. procesos y subprocesos;
2. IPC local;
3. estado persistente;
4. worktrees Git;
5. scheduling de CPU/RAM;
6. checkpoints y handoffs;
7. observación de hooks/eventos;
8. búsqueda y compresión determinística de contexto;
9. TUI reactiva;
10. recuperación tras fallos.

El stack recomendado queda así:

```text
┌────────────────────────────────────────────────────────────────────┐
│                         SYMPHONY CLI                               │
├────────────────────────────────────────────────────────────────────┤
│ Lenguaje                         Rust 2024                          │
│ Async/runtime                    Tokio                              │
│ CLI                              clap                               │
│ TUI                              Ratatui + Crossterm                │
│ IPC local                        interprocess + JSON versionado      │
│ Procesos / PTY / límites         ProcessKit + APIs OS               │
│ Métricas del sistema             sysinfo                            │
│ Estado                           SQLite + rusqlite + WAL             │
│ Migraciones                      rusqlite_migration                  │
│ IDs                              ULID                               │
│ Configuración                    serde + toml_edit                  │
│ Object store                     BLAKE3 + zstd + tempfile           │
│ Git                              Git CLI nativo                     │
│ Parsing de código                tree-sitter                        │
│ Búsqueda contexto                SQLite FTS5/BM25 + ignore          │
│ MCP                              rmcp                               │
│ Logs                             tracing + tracing-subscriber        │
│ Event bus interno                Tokio channels bounded             │
│ Routing                          motor propio determinístico         │
│ Scheduler                        motor propio + Semaphore/queues      │
│ Plugins                          procesos externos + protocolo JSON  │
│ Providers                        CLIs oficiales + adapters propios   │
│ Tests                            cargo-nextest + trycmd + insta       │
│ Property testing                 proptest                           │
│ Benchmarks                       Criterion + harness E2E propio      │
│ Coverage                         cargo-llvm-cov                      │
│ Fuzzing                          cargo-fuzz                          │
│ Seguridad deps                   cargo-deny (+ cargo-vet pre-1.0)    │
│ CI                               GitHub Actions multi-OS             │
│ Releases                         cargo-dist / dist                   │
│ Documentación                    mdBook + Mermaid + cargo doc        │
│ GUI futura                       Tauri 2 + SolidJS + TS + Vite       │
└────────────────────────────────────────────────────────────────────┘
```

La regla arquitectónica que guía todas las elecciones es:

> **Ninguna dependencia o función inteligente puede empeorar de forma material el camino básico de ejecución.**

---

# 2. Cómo se derivó este stack de los requisitos del proyecto

Los documentos de Symphony fijan varias restricciones que afectan directamente el stack:

- `AGENT ≠ MODEL`: el estado del agente debe sobrevivir al executor.
- `STATE ≠ MODEL MEMORY`: Symphony necesita persistencia propia.
- `CHECKPOINT BEFORE FAILURE`: el sistema necesita captura incremental barata.
- `WORKSPACES MUST BE ISOLATED`: Git worktrees son parte estructural.
- `HEAVY WORK IS SCHEDULED`: hace falta observación y enforcement de procesos.
- `SMART FEATURES ARE OPTIONAL`: no se debe depender de ML para operar.
- `RULES BEFORE ML`: el router y dispatcher iniciales son determinísticos.
- CLI-first, local-first, sin telemetría.
- El objetivo real es correr varios agentes sin volver inutilizable la máquina.
- Los CLIs oficiales siguen siendo dueños de sus credenciales.
- Los hooks no escriben directamente a la base; envían eventos al daemon.
- Logs y outputs grandes viven fuera de SQLite.
- El Context Engine v1 usa búsqueda léxica, AST y compresión estructural, no embeddings.

Por eso el stack prioriza sistemas, procesos, almacenamiento local y observabilidad, no frameworks web o infraestructura cloud.

---

# 3. Lenguaje y runtime principal

## 3.1 Lenguaje: Rust

| Campo | Decisión |
|---|---|
| **Elección** | Rust, Edition 2024 |
| **Se usará para** | Core, daemon, CLI, TUI, adapters, scheduler, context engine, IPC, persistencia, plugins internos y utilidades |
| **Dónde** | Todo `crates/` y los binarios `symphony` / `symphonyd` |
| **Por qué se necesita** | Symphony mantiene muchos procesos, streams, sockets, watchers, estados y tareas concurrentes durante horas |
| **Por qué se escogió** | Binario nativo, bajo overhead, memoria segura, concurrencia fuerte, control fino del OS y excelente distribución cross-platform |
| **Alternativa** | Go |
| **Por qué no la alternativa** | Go sería más rápido de desarrollar inicialmente y tiene buena concurrencia, pero Rust ofrece mayor control de memoria, FFI/OS y mejor encaje con el objetivo de minimizar overhead y mantener un daemon pequeño |

### Política de versión

- Usar Rust estable.
- Edition 2024.
- Fijar el toolchain del repositorio mediante `rust-toolchain.toml`.
- Definir un MSRV explícito y testearlo en CI.
- Recomendación inicial: **MSRV 1.95** o superior, sujeto a las dependencias finalmente fijadas.

### Por qué no TypeScript/Node como core

Los propios CLIs externos ya pueden consumir cientos de MB. Symphony no debería añadir otro runtime Node permanente.

Node sí puede aparecer en:

- proyectos que los agentes estén modificando;
- herramientas externas;
- GUI web interna futura.

Pero no como runtime central.

### Por qué no Python

Python es excelente para prototipos, pero no es ideal como daemon final para:

- control fino de procesos;
- consumo consistente;
- distribución en binario;
- concurrencia I/O + subprocess robusta;
- integración profunda con APIs del OS.

---

## 3.2 Runtime asíncrono: Tokio

| Campo | Decisión |
|---|---|
| **Elección** | Tokio |
| **Se usará para** | IPC, subprocess streams, timers, heartbeats, event bus, watchers, scheduler y tareas concurrentes |
| **Dónde** | `core`, `daemon`, `process`, `scheduler`, `protocol`, adapters |
| **Por qué se necesita** | Symphony debe observar varios CLIs simultáneamente sin crear un hilo por cada operación |
| **Por qué se escogió** | Es el runtime async de facto del ecosistema Rust y tiene primitives maduras para I/O, timers, procesos, canales y cancelación |
| **Alternativa** | async-std / smol |
| **Por qué no la alternativa** | Menor ecosistema para las librerías concretas que Symphony necesita |

### Configuración recomendada

No usar `tokio = { features = ["full"] }` sin necesidad.

Habilitar sólo lo requerido, por ejemplo:

- `rt-multi-thread`
- `macros`
- `process`
- `io-util`
- `net`
- `sync`
- `time`
- `signal`

El daemon no necesita explotar todos los cores. Un número pequeño de worker threads es suficiente porque la mayor parte del trabajo pesado se ejecuta fuera del daemon.

---

## 3.3 Cancelación y utilidades async: tokio-util

| Campo | Decisión |
|---|---|
| **Elección** | `tokio-util` |
| **Se usará para** | `CancellationToken`, cancelación jerárquica y lifecycle de agentes/runs |
| **Dónde** | Scheduler, Agent Runtime, process supervisor |
| **Por qué se escogió** | Permite cancelar árboles de tareas de forma estructurada sin inventar flags manuales |
| **Alternativa** | canales `watch` personalizados |
| **Por qué no la alternativa** | Más boilerplate y mayor riesgo de estados incompletos |

---

# 4. Estructura del repositorio

## 4.1 Cargo Workspace

Usar un único workspace Rust:

```text
symphony/
├── Cargo.toml
├── rust-toolchain.toml
├── crates/
│   ├── protocol/
│   ├── core/
│   ├── daemon/
│   ├── cli/
│   ├── tui/
│   ├── store/
│   ├── object-store/
│   ├── process/
│   ├── scheduler/
│   ├── git/
│   ├── context/
│   ├── mcp/
│   ├── skills/
│   ├── plugins/
│   ├── validation/
│   ├── adapters/
│   │   ├── common/
│   │   ├── claude/
│   │   ├── codex/
│   │   ├── antigravity/
│   │   ├── kimi/
│   │   └── copilot/
│   └── testkit/
├── apps/
│   └── gui/                 # futuro
├── docs/
├── migrations/
└── fixtures/
```

### Por qué

Evita un monolito de decenas de miles de líneas y permite que cada subsistema tenga un contrato claro.

### Alternativa

Un solo crate.

### Por qué no

Complica tests, límites de dependencia, feature flags y evolución independiente de adapters.

---

## 4.2 Automatización del repositorio: `cargo xtask`

| Campo | Decisión |
|---|---|
| **Elección** | Patrón `cargo xtask` |
| **Uso** | tareas de desarrollo, generación de fixtures, comprobaciones, packaging auxiliar |
| **Dónde** | crate `xtask/` |
| **Por qué** | Cross-platform, versionado junto al código y sin obligar a instalar Make/Just |
| **Alternativa** | `just` |
| **Por qué no la alternativa** | `just` es excelente, pero sería otra dependencia que el contributor debe instalar |

---

# 5. CLI y TUI

## 5.1 Parser CLI: clap

| Campo | Decisión |
|---|---|
| **Elección** | `clap` con derive |
| **Uso** | Parsear `symphony`, subcomandos, flags y ayuda |
| **Dónde** | binario `symphony` |
| **Por qué** | Muy maduro, rápido y genera ayuda consistente |
| **Alternativa** | `argh` |
| **Por qué no** | Menos completo para una CLI que terminará teniendo muchos subcomandos |

Ejemplos:

```text
symphony
symphony status
symphony spawn
symphony providers
symphony context
symphony hook emit
symphony mcp serve
```

---

## 5.2 TUI: Ratatui

| Campo | Decisión |
|---|---|
| **Elección** | Ratatui |
| **Uso** | Home, Agent View, Providers, Tasks, System, Recovery, Context, Diff, Settings |
| **Dónde** | crate `tui` |
| **Por qué** | Es el toolkit Rust más sólido para interfaces terminales complejas |
| **Alternativa** | Ink/React en Node |
| **Por qué no** | Introduciría Node dentro del frontend principal y aumentaría overhead |

---

## 5.3 Backend de terminal: Crossterm

| Campo | Decisión |
|---|---|
| **Elección** | Crossterm |
| **Uso** | input de teclado, resize, mouse opcional, raw mode y rendering portable |
| **Dónde** | TUI |
| **Por qué** | Windows/Linux/macOS con una API uniforme; integración estándar con Ratatui |
| **Alternativa** | termion |
| **Por qué no** | Crossterm tiene mejor soporte cross-platform para el target de Symphony |

---

## 5.4 Diagnósticos CLI: miette + thiserror

| Campo | Decisión |
|---|---|
| **Elección** | `thiserror` para errores tipados + `miette` en la frontera de usuario |
| **Uso** | Errores internos y mensajes ricos en CLI |
| **Dónde** | Todas las crates; `miette` principalmente en `cli` |
| **Por qué** | Separa errores programáticos de diagnósticos humanos |
| **Alternativa** | `anyhow` para todo |
| **Por qué no** | `anyhow` es útil en aplicaciones pequeñas, pero para un sistema con recovery y clasificación de fallos conviene conservar tipos |

Regla:

- crates internas → errores con `thiserror`;
- binarios → convertirlos a `miette::Report`;
- no usar `unwrap()` en paths de runtime.

---

# 6. Protocolo e IPC

## 6.1 Transporte IPC: interprocess

| Campo | Decisión |
|---|---|
| **Elección** | crate `interprocess` |
| **Uso** | comunicación entre `symphony`, `symphonyd`, hooks y brokers locales |
| **Dónde** | `protocol` / `daemon` |
| **Por qué** | Abstrae Named Pipes en Windows y Unix Domain Sockets en Unix con soporte async |
| **Alternativa** | HTTP en `127.0.0.1` con Axum |
| **Por qué no** | HTTP añade servidor, puertos, parsing y superficie de ataque que el core no necesita |

### Transporte esperado

```text
Windows → Named Pipe
Linux   → Unix Domain Socket
macOS   → Unix Domain Socket
```

---

## 6.2 Wire format: JSON tipado y versionado

| Campo | Decisión |
|---|---|
| **Elección** | Mensajes JSON con `serde_json`, framing por longitud y versión de protocolo |
| **Uso** | requests, responses, events y subscriptions |
| **Dónde** | crate `protocol` |
| **Por qué** | Fácil de inspeccionar y depurar, muy suficiente para tráfico local |
| **Alternativa** | Protobuf/gRPC |
| **Por qué no** | Generación de código y stack mucho más pesado para un protocolo exclusivamente local |

Ejemplo conceptual:

```json
{
  "protocol": 1,
  "id": "01...",
  "method": "agent.pause",
  "params": {
    "agent_id": "01..."
  }
}
```

### No usar JSON sin framing

Un socket recibe bytes, no “mensajes”. Cada frame debe llevar longitud o utilizar un codec inequívoco.

---

## 6.3 Event bus interno: Tokio channels

Usar:

- `mpsc` bounded para comandos/eventos con backpressure;
- `broadcast` para consumidores efímeros como TUI;
- `watch` para estado actual;
- `oneshot` para request/response internos.

| Alternativa | Por qué no |
|---|---|
| Redis Streams | Requiere servidor externo |
| NATS | Infraestructura innecesaria |
| Kafka | Absolutamente desproporcionado |

El event bus de Symphony vive en memoria y su historial se persiste aparte.

---

# 7. Control de procesos y PTY

## 7.1 Process supervisor: ProcessKit

| Campo | Decisión |
|---|---|
| **Elección** | ProcessKit 3.x, detrás de una abstracción propia |
| **Uso** | Spawn, streaming stdout/stderr, PTY cuando haga falta, kill del árbol, límites y stats |
| **Dónde** | crate `process` |
| **Por qué se necesita** | Los executors reales son CLIs externos y pueden crear shells, Node workers, tests y builds |
| **Por qué se escogió** | Reúne async process management, whole-tree termination, PTY y resource limits con soporte multiplataforma |
| **Alternativa** | `tokio::process` + `windows-sys` + `nix` + PTY propia |
| **Por qué no directamente** | Mucho código crítico específico por plataforma; ProcessKit reduce esa superficie |

### Regla de seguridad arquitectónica

ProcessKit **no debe contaminar el resto del core**.

Crear una interfaz propia:

```rust
trait ProcessSupervisor {
    async fn spawn(&self, spec: ProcessSpec) -> Result<ProcessHandle>;
    async fn terminate_tree(&self, id: ProcessId) -> Result<()>;
    async fn suspend(&self, id: ProcessId) -> Result<()>;
    async fn stats(&self, id: ProcessId) -> Result<ProcessStats>;
}
```

Así, si Phase 0 descubre un problema, puede sustituirse sin rediseñar Symphony.

---

## 7.2 Fallback OS-level

### Windows: windows-sys

| Campo | Decisión |
|---|---|
| **Elección** | `windows-sys` sólo en módulos `cfg(windows)` |
| **Uso** | Job Objects, process handles y funciones que ProcessKit no cubra |
| **Alternativa** | `windows` crate |
| **Por qué se escoge windows-sys** | Bindings más directos y de menor nivel para el pequeño conjunto de APIs requerido |

### Unix: nix

| Campo | Decisión |
|---|---|
| **Elección** | `nix` |
| **Uso** | process groups, signals y utilidades Unix específicas |
| **Alternativa** | libc directo |
| **Por qué no libc directo** | `nix` ofrece wrappers más seguros |

### Linux

Usar cgroups v2 cuando estén disponibles para límites duros.

### macOS

Usar process groups y medición; no prometer la misma semántica de hard limits que Linux/Windows si el OS no la ofrece.

---

## 7.3 Métricas de procesos y sistema: sysinfo

| Campo | Decisión |
|---|---|
| **Elección** | `sysinfo` |
| **Uso** | CPU, RAM, procesos y muestras del sistema |
| **Dónde** | Resource Manager y benchmark harness |
| **Por qué** | API portable para Windows/Linux/macOS |
| **Alternativa** | bindings nativos por plataforma |
| **Por qué no** | Multiplica el código y riesgo de bugs para información que `sysinfo` ya normaliza |

---

# 8. Scheduler

## 8.1 Implementación

No usar un framework externo de jobs.

Construir el scheduler como lógica propia sobre:

- `tokio::sync::Semaphore`;
- `BinaryHeap`/priority queue;
- canales bounded;
- Process Supervisor;
- métricas `sysinfo`.

| Campo | Decisión |
|---|---|
| **Elección** | Scheduler propio determinístico |
| **Uso** | Controlar classes 0–4, prioridades, slots pesados, CPU/RAM y fairness |
| **Dónde** | crate `scheduler` |
| **Por qué** | Las reglas son específicas de Symphony y simples comparadas con un cluster scheduler |
| **Alternativa** | Tokio task spawning sin scheduler propio |
| **Por qué no** | No resolvería el problema central de que varios agentes saturen la máquina |

### Componentes internos

```text
OperationClassifier
ResourceBudget
PriorityQueue
HeavySemaphore
AgentFairness
ProcessEnforcer
QueueExplainer
```

### No usar ML

La clasificación de:

```text
npm test
cargo build
pnpm install
grep
git diff
```

debe empezar con reglas y metadatos de tool calls.

---

# 9. Persistencia

## 9.1 Base de datos: SQLite

| Campo | Decisión |
|---|---|
| **Elección** | SQLite embebido |
| **Uso** | Proyectos, sesiones, tasks, agents, runs, providers, routing, events, checkpoints, validation, recovery |
| **Dónde** | `~/.symphony/state.db` |
| **Por qué** | Local, transaccional, crash-safe, sin servidor y con FTS5 |
| **Alternativa** | PostgreSQL |
| **Por qué no** | Exigiría un servidor y contradice el objetivo local/ligero |

### Modo

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous = NORMAL;
```

`NORMAL` en WAL ofrece un buen equilibrio para metadata operativa; operaciones verdaderamente críticas pueden forzar checkpoints/transactions explícitas.

---

## 9.2 Driver: rusqlite

| Campo | Decisión |
|---|---|
| **Elección** | `rusqlite` con SQLite bundled |
| **Uso** | Toda la capa de persistencia |
| **Dónde** | crate `store` |
| **Por qué** | API directa, pequeña y predecible; Symphony ya define un solo writer |
| **Alternativa** | SQLx |
| **Por qué no** | Pool async y abstracción multi-DB son complejidad sin beneficio para una SQLite local de un solo escritor |

### Muy importante

No crear un pool de muchas conexiones de escritura.

Arquitectura:

```text
hooks / adapters / scheduler
            │
            ▼
     Store command channel
            │
            ▼
   Dedicated DB worker
            │
            ▼
         SQLite
```

Una sola autoridad de escritura reduce lock contention y simplifica crash recovery.

---

## 9.3 Migraciones: rusqlite_migration

| Campo | Decisión |
|---|---|
| **Elección** | `rusqlite_migration` |
| **Uso** | Versionado del esquema |
| **Dónde** | startup de `symphonyd` |
| **Por qué** | Ligero y construido específicamente alrededor de rusqlite |
| **Alternativa** | refinery |
| **Por qué no** | Refinery es bueno, pero rusqlite_migration mantiene el stack más directo |

---

## 9.4 IDs: ULID

| Campo | Decisión |
|---|---|
| **Elección** | crate `ulid` |
| **Uso** | IDs de projects, tasks, agents, runs, checkpoints, objects |
| **Por qué** | Ordenables temporalmente y aptos para generación local sin coordinación |
| **Alternativa** | UUIDv7 |
| **Por qué no** | UUIDv7 también sería excelente; ULID se alinea con el modelo ya especificado y es legible en logs |

Excepción:

- `events.id` y tablas de altísimo volumen pueden usar `INTEGER PRIMARY KEY`.

---

# 10. Object store

## 10.1 Hash: BLAKE3

| Campo | Decisión |
|---|---|
| **Elección** | `blake3` |
| **Uso** | Content addressing, deduplicación e integridad |
| **Dónde** | `~/.symphony/objects/` |
| **Por qué** | Muy rápido y adecuado para hashing local de logs, diffs y outputs |
| **Alternativa** | SHA-256 |
| **Por qué no** | SHA-256 es más universal, pero BLAKE3 es mejor para el objetivo de throughput local; no se usa aquí como firma criptográfica |

---

## 10.2 Compresión: zstd

| Campo | Decisión |
|---|---|
| **Elección** | `zstd` |
| **Uso** | Logs, outputs, transcripts grandes y objetos originales |
| **Por qué** | Muy buen ratio/velocidad y niveles configurables |
| **Alternativa** | gzip |
| **Por qué no** | Peor equilibrio entre velocidad y compresión para este tipo de almacenamiento |

Nivel recomendado:

- bajo/moderado para el path interactivo;
- compresión más agresiva sólo en mantenimiento idle si un benchmark demuestra valor.

---

## 10.3 Escrituras atómicas: tempfile

| Campo | Decisión |
|---|---|
| **Elección** | `tempfile` + persist/rename |
| **Uso** | Crear blobs sin dejar archivos parciales tras crash |
| **Dónde** | object store |
| **Alternativa** | escribir directamente al destino |
| **Por qué no** | Un crash podría producir blobs truncados con hashes inválidos |

---

# 11. Configuración

## 11.1 Formato: TOML

| Campo | Decisión |
|---|---|
| **Elección** | TOML |
| **Uso** | `config.toml`, `project.toml`, registry MCP, plugins |
| **Por qué** | Legible, editable y apropiado para configuración humana |
| **Alternativa** | YAML |
| **Por qué no** | YAML tiene más ambigüedades y edge cases de parsing |

---

## 11.2 Parsing/serialización: serde

| Campo | Decisión |
|---|---|
| **Elección** | `serde` |
| **Uso** | Config, DB JSON pequeño, protocolo IPC, manifests |
| **Por qué** | Estándar del ecosistema Rust |
| **Alternativa** | parsing manual |
| **Por qué no** | Más código y menos seguridad de tipos |

---

## 11.3 Edición preservando comentarios: toml_edit

| Campo | Decisión |
|---|---|
| **Elección** | `toml_edit` |
| **Uso** | Cuando Symphony modifica archivos del usuario |
| **Por qué** | Conserva estructura/comentarios mejor que reserializar todo |
| **Alternativa** | `toml` + serde |
| **Por qué no** | Reescribiría el archivo perdiendo formato y comentarios |

---

# 12. Git y worktrees

## 12.1 Git: CLI oficial del sistema

| Campo | Decisión |
|---|---|
| **Elección** | Ejecutar `git` |
| **Uso** | worktrees, branches, diff, merge, status, commits, conflict preflight |
| **Dónde** | crate `git` |
| **Por qué** | Symphony debe comportarse exactamente como el Git que el usuario ya utiliza |
| **Alternativa** | libgit2 / `git2` crate |
| **Por qué no** | No implementa siempre todas las semánticas/workflows de la CLI de Git con la misma fidelidad y añade una capa más |

### Usar formatos machine-readable

Preferir:

```text
git status --porcelain=v2 -z
git diff --no-ext-diff
git worktree list --porcelain
```

Evitar parsear output “bonito” pensado para humanos.

---

## 12.2 Estrategia de dependencias por worktree

Symphony no debe imponer un package manager universal.

### Detección

```text
pnpm-lock.yaml       → pnpm
package-lock.json    → npm
yarn.lock            → yarn
bun.lock/bun.lockb   → bun
Cargo.lock           → cargo
uv.lock              → uv
poetry.lock          → poetry
```

### Preferencia cuando el proyecto usa pnpm

Usar el store content-addressable compartido de pnpm.

### Alternativa

Links/hardlinks seguros cuando lockfiles coinciden.

### Regla

Nunca modificar el package manager del proyecto sólo para beneficiar a Symphony.

---

# 13. File watching y traversal

## 13.1 Watcher: notify

| Campo | Decisión |
|---|---|
| **Elección** | `notify` |
| **Uso** | Detectar cambios del proyecto/worktrees |
| **Dónde** | Project State / Context Engine |
| **Por qué** | Abstracción madura sobre mecanismos nativos de los tres OS |
| **Alternativa** | polling periódico |
| **Por qué no** | Más I/O y latencia |

### Diseño

Un watcher lógico por proyecto, no uno por agente cuando pueda evitarse.

Debounce y coalescing son obligatorios.

---

## 13.2 Traversal: ignore

| Campo | Decisión |
|---|---|
| **Elección** | crate `ignore` |
| **Uso** | Recorrer archivos respetando `.gitignore` y exclusiones |
| **Por qué** | Es rápido y evita indexar `node_modules`, `target`, builds y basura |
| **Alternativa** | `walkdir` |
| **Por qué no** | `ignore` entiende reglas Git directamente |

---

# 14. Context Engine

## 14.1 Parsing estructural: tree-sitter

| Campo | Decisión |
|---|---|
| **Elección** | tree-sitter |
| **Uso** | L0–L5, símbolos, firmas, funciones relevantes y reducción estructural de código |
| **Dónde** | crate `context` |
| **Por qué** | Parsing incremental, rápido y multilenguaje |
| **Alternativa** | Language Server Protocol |
| **Por qué no** | Levantar language servers por proyecto/agente sería mucho más pesado |

### Política

No cargar decenas de gramáticas por defecto.

Compilar/activar sólo un conjunto inicial importante y permitir plugins de lenguaje más adelante.

Fallback:

```text
tree-sitter disponible → estructura
no grammar → búsqueda textual / chunks
```

---

## 14.2 Búsqueda textual: SQLite FTS5 + BM25

| Campo | Decisión |
|---|---|
| **Elección** | FTS5 embebido |
| **Uso** | `context.search`, historial, decisiones, logs/chunks |
| **Dónde** | misma SQLite |
| **Por qué** | Cero servidor y suficiente para v1 |
| **Alternativa** | Qdrant/vector DB |
| **Por qué no** | Embeddings + proceso/DB adicional contradicen las prioridades sin evidencia de beneficio |

### No vector DB en v1

No usar:

- Qdrant;
- Chroma;
- Weaviate;
- Milvus;
- Pinecone.

Si los benchmarks demuestran que BM25 falla en retrieval semántico relevante, se reevalúa después.

---

## 14.3 Compresores determinísticos: implementación propia

Construir módulos pequeños:

```text
LogCollapser
TestSummaryCompressor
JsonStructuralCompressor
Deduplicator
AstReducer
GitDiffReducer
```

| Alternativa | Por qué no |
|---|---|
| Headroom como dependencia | Duplica runtime/complejidad y el proyecto necesita integración profunda con Git/agents |
| LLM de resumen | Añade latencia, consumo y posibilidad de pérdida semántica |
| Embeddings | No necesarios para colapsar estructuras determinísticas |

La compresión siempre debe preservar el original en el object store.

---

## 14.4 Consolidación de memoria: motor propio por hechos

Usar registros estructurados:

```text
fact
status = CURRENT | SUPERSEDED
source
created_at
superseded_by
```

No usar un “memory LLM” en v1.

---

# 15. MCP

## 15.1 SDK: rmcp

| Campo | Decisión |
|---|---|
| **Elección** | `rmcp`, SDK oficial Rust para MCP |
| **Uso** | Context MCP y futuras herramientas compartidas |
| **Dónde** | crate `mcp` |
| **Por qué** | Evita implementar JSON-RPC/MCP a mano y mantiene compatibilidad con el protocolo |
| **Alternativa** | servidor MCP manual |
| **Por qué no** | Riesgo innecesario de incompatibilidades |

### Arquitectura recomendada

Los CLIs que necesiten stdio pueden lanzar:

```text
symphony mcp serve
```

Ese proceso debe ser un **broker mínimo**, no otro Context Engine.

```text
CLI
  │ stdio MCP
  ▼
symphony mcp serve
  │ IPC local
  ▼
symphonyd
  │
  ▼
Context Engine
```

Así pueden existir varios procesos broker baratos sin duplicar memoria/índices.

---

# 16. Skills

## 16.1 Formato canónico propio

Una skill vive en:

```text
~/.symphony/skills/<name>/
project/.symphony/skills/<name>/
```

Archivos:

```text
SKILL.md
skill.toml       # metadata opcional
resources/
```

| Campo | Decisión |
|---|---|
| **Elección** | Markdown + TOML |
| **Uso** | Fuente canónica que adapters traducen al formato del CLI |
| **Alternativa** | Guardar skills sólo en SQLite |
| **Por qué no** | Archivos son más fáciles de versionar, revisar y editar |

SQLite puede indexar metadata/estado, pero el filesystem debe seguir siendo la fuente de verdad de la skill.

---

# 17. Arquitectura de plugins

## 17.1 Plugins de terceros: procesos externos

No cargar `.dll/.so/.dylib` arbitrarios dentro de `symphonyd`.

Usar:

```text
plugin.toml
executable
versioned JSON protocol
stdin/stdout o local socket
```

| Campo | Decisión |
|---|---|
| **Elección** | Process-isolated plugins |
| **Uso** | Provider adapters externos, validators, context transformers, tools |
| **Por qué** | Un plugin que crashea no tumba el daemon y puede limitarse con el mismo supervisor |
| **Alternativa** | dynamic libraries |
| **Por qué no** | ABI, seguridad y crash isolation peores |

### Alternativa futura

WASM + Wasmtime.

No usarlo en v1 porque añade un runtime y complejidad que todavía no se justifica.

---

# 18. Provider adapters

Los cinco adapters iniciales deben ser crates de primera parte.

## 18.1 Patrón común

```rust
trait ProviderAdapter {
    async fn detect(...);
    async fn auth_status(...);
    async fn list_models(...);
    async fn spawn(...);
    async fn resume(...);
    async fn stop(...);
    fn parse_event(...);
    fn parse_error(...);
    async fn health(...);
    async fn install_hooks(...);
}
```

El core nunca contiene `if provider == "claude"`.

---

## 18.2 Claude Code Adapter

| Campo | Decisión |
|---|---|
| **Dependencia externa** | Claude Code CLI oficial |
| **Uso** | Executor Anthropic |
| **Integración** | Headless/CLI + hooks + transcript cuando esté disponible |
| **Auth** | Propiedad del CLI oficial |
| **Por qué** | Mantiene Symphony fuera de credenciales y endpoints privados |
| **Alternativa** | Anthropic API directa |
| **Por qué no inicialmente** | Cambiaría el modelo de autenticación/costo y no reutiliza necesariamente la suscripción del usuario |

---

## 18.3 Codex Adapter

| Campo | Decisión |
|---|---|
| **Dependencia externa** | Codex CLI oficial |
| **Uso** | Executor OpenAI |
| **Integración** | ejecución no interactiva/event stream estructurado cuando esté disponible |
| **Auth** | Cuenta manejada por el CLI |
| **Alternativa** | OpenAI API directa |
| **Por qué no inicialmente** | Mismo principio: CLI-bridge primero; direct sólo como modo oficial futuro |

---

## 18.4 Antigravity Adapter

| Campo | Decisión |
|---|---|
| **Dependencia externa** | Antigravity CLI oficial, según el contrato validado en Phase 5 |
| **Uso** | Executor Google |
| **Integración** | hooks/stream/output disponible |
| **Auth** | CLI oficial |
| **Alternativa** | integración directa con Gemini API |
| **Por qué no inicialmente** | Symphony debe trabajar con la suscripción/configuración oficial del usuario |

---

## 18.5 Kimi Adapter

| Campo | Decisión |
|---|---|
| **Dependencia externa** | Kimi Code CLI oficial |
| **Uso** | Executor Kimi |
| **Integración** | CLI + hooks/events disponibles |
| **Auth** | CLI oficial |
| **Alternativa** | API Moonshot/Kimi |
| **Por qué no inicialmente** | Mantener el mismo modelo de seguridad y suscripción |

---

## 18.6 Copilot Adapter

| Campo | Decisión |
|---|---|
| **Dependencia externa** | GitHub Copilot CLI oficial |
| **Uso** | Executor GitHub |
| **Integración** | CLI + hooks |
| **Auth** | Login manejado por GitHub/Copilot |
| **Alternativa** | APIs individuales de modelos |
| **Por qué no inicialmente** | Rompería el concepto de usar la suscripción existente |

---

## 18.7 Direct mode futuro

Sólo si un proveedor documenta explícitamente una ruta compatible con el uso previsto.

Tecnología recomendada para ese módulo futuro:

- `reqwest`;
- `rustls`;
- SSE/WebSocket si el proveedor lo requiere.

Debe quedar detrás de una feature flag y **fuera del core básico**.

---

# 19. Routing

## 19.1 Motor: implementación propia determinística

No usar un LLM como router en el camino crítico.

### Inputs

```text
capability fit
context fit
provider health
quota certainty/headroom
reserve
recent failures
current load
latency class
user policy
```

### Outputs

```text
eligible
reject_reason
score
factors
selected
explanation
```

### Por qué

Con pocos candidatos, reglas transparentes son:

- más rápidas;
- auditables;
- reproducibles;
- fáciles de testear;
- prácticamente gratis.

### Alternativa

Jev/Laya/NanoJev u otro decision model.

### Cuándo reevaluarla

Sólo después de tener un benchmark con dataset real de decisiones donde el modelo supere claramente las reglas sin perjudicar latencia/RAM.

---

# 20. Provider health y error parsing

## 20.1 Clasificación por adapter

Cada adapter implementa parsers específicos para:

```text
RPM
TPM
TEMP_RATE_LIMIT
DAILY_QUOTA
WEEKLY_QUOTA
MODEL_LIMIT
ACCOUNT_LIMIT
AUTH
NETWORK
PROVIDER_ERROR
MODEL_UNAVAILABLE
UNKNOWN
```

### Herramientas

- parsing estructurado si el CLI emite JSON;
- patrones propios sólo como fallback;
- `regex` para mensajes no estructurados.

### Alternativa

Clasificador LLM de errores.

### Por qué no

Un error de cuota/rate limit es un problema de protocolo, no de razonamiento.

---

# 21. Logs y observabilidad local

## 21.1 tracing

| Campo | Decisión |
|---|---|
| **Elección** | `tracing` + `tracing-subscriber` + `tracing-appender` |
| **Uso** | Logs estructurados y spans |
| **Dónde** | Todo el daemon |
| **Por qué** | Excelente para sistemas concurrentes async |
| **Alternativa** | `log` + env_logger |
| **Por qué no** | Menos contexto estructurado entre tasks/spans |

### Contexto de span recomendado

```text
project_id
session_id
agent_id
run_id
provider
model
tool_call_id
```

### Persistencia

- consola/TUI: sólo mensajes útiles;
- archivos rotativos locales: debug/audit;
- DB: sólo eventos del dominio, no cada línea de log técnico.

### Prohibido

- OpenTelemetry remoto por defecto;
- Sentry;
- analytics;
- beacon.

Symphony no envía telemetría.

---

# 22. Seguridad y redacción

## 22.1 Secret redaction

Implementar un redactor central antes de:

- persistir stdout/stderr;
- guardar errors;
- mostrar audit logs;
- escribir tracing.

Redactar:

```text
Authorization
Bearer ...
API keys
cookies
known credential env vars
```

### No usar un secrets vault

Symphony no debería almacenar los secretos de los proveedores.

### Alternativa

OS Keychain.

Sólo sería necesario si en el futuro Symphony tiene **secretos propios**, no para copiar los de los CLIs.

---

# 23. Validación de proyectos

Symphony debe orquestar validadores, no imponer un ecosistema.

## 23.1 Detector de proyecto

Reconocer:

```text
package.json
Cargo.toml
pyproject.toml
go.mod
pom.xml
build.gradle
...
```

## 23.2 Validation commands

Guardar comandos por proyecto:

```text
tier1
tier2
tier3
```

Ejemplo:

```toml
[validation]
tier1 = ["npm run lint"]
tier2 = ["npm run typecheck", "npm test -- --changed"]
tier3 = ["npm test", "npm run build"]
```

### Por qué no integrar ESLint/Vitest/Cargo directamente

Symphony debe poder trabajar con cualquier stack de proyecto.

Su responsabilidad es:

- schedule;
- ejecutar;
- capturar;
- resumir;
- reportar.

---

# 24. Testing de Symphony

## 24.1 Test runner: cargo-nextest

| Campo | Decisión |
|---|---|
| **Elección** | `cargo-nextest` |
| **Uso** | Ejecutar suite Rust en desarrollo/CI |
| **Por qué** | Mejor aislamiento, reporting y paralelismo que el runner estándar en suites grandes |
| **Alternativa** | `cargo test` |
| **Por qué no como principal CI** | Sigue siendo fallback, pero nextest dará mejor experiencia al crecer la suite |

---

## 24.2 CLI tests: trycmd

| Campo | Decisión |
|---|---|
| **Elección** | `trycmd` |
| **Uso** | Probar comandos, exit codes, stdout/stderr |
| **Dónde** | `cli` |
| **Alternativa** | assert_cmd |
| **Por qué se escoge** | Muy cómodo para fixtures/snapshots de CLIs |

`assert_cmd` sigue siendo buena alternativa si algunos casos requieren control imperativo.

---

## 24.3 Snapshot tests: insta

| Campo | Decisión |
|---|---|
| **Elección** | `insta` |
| **Uso** | TUI states, diagnostics, route explanations y serialized events |
| **Alternativa** | archivos golden manuales |
| **Por qué no** | Insta mejora review y actualización controlada de snapshots |

---

## 24.4 Property-based tests: proptest

Usar especialmente para:

- state machines;
- scheduler invariants;
- DAG;
- parsing de eventos;
- routing scoring;
- IPC framing.

Ejemplos de invariantes:

```text
un agente nunca tiene dos runs activos
un task DAG nunca acepta un ciclo
un modelo ineligible nunca puede ganar routing
un checkpoint no referencia un objeto inexistente
```

---

## 24.5 Fuzzing: cargo-fuzz / libFuzzer

Fuzz targets prioritarios:

1. parsers de stdout/CLI;
2. IPC frames;
3. JSON/TOML importado;
4. provider error parsers;
5. ANSI/terminal output;
6. context URI parser.

### Alternativa

AFL.

`cargo-fuzz` tiene integración más directa con el ecosistema Rust.

---

# 25. Benchmarks

## 25.1 Microbenchmarks: Criterion

| Campo | Decisión |
|---|---|
| **Elección** | Criterion |
| **Uso** | Routing, compression, FTS, event serialization, checkpoint building |
| **Alternativa** | `cargo bench` nightly |
| **Por qué** | Estadísticas y regresiones más útiles |

---

## 25.2 Benchmark E2E propio

Más importante que Criterion.

Debe medir escenarios reales:

```text
1 agent / 1 provider
3 agents / 3 worktrees
3 agents + one heavy test
forced failover
daemon restart
large logs
10k/100k events
```

Métricas:

```text
RAM daemon
RAM total process tree
CPU average / p95
process count
startup latency
time to first token
event latency
scheduler queue latency
checkpoint latency
handoff build latency
failover latency
context raw/sent
```

Este harness decide si una optimización entra al producto.

---

# 26. Coverage

## 26.1 cargo-llvm-cov

| Campo | Decisión |
|---|---|
| **Elección** | `cargo-llvm-cov` |
| **Uso** | Coverage local y CI |
| **Alternativa** | tarpaulin |
| **Por qué se escoge** | Integración LLVM y buen soporte de workspaces/nextest |

No fijar un porcentaje absurdo global.

Pedir cobertura alta especialmente en:

- scheduler;
- state transitions;
- persistence;
- routing;
- recovery;
- parsers.

---

# 27. Lint, formato y calidad

## Obligatorios

```text
rustfmt
clippy
```

CI:

```text
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

### Política

`unsafe`:

- prohibido por defecto;
- permitido sólo en módulos OS muy aislados si una API lo exige;
- comentario de safety obligatorio.

---

# 28. Seguridad de dependencias

## 28.1 cargo-deny

| Campo | Decisión |
|---|---|
| **Elección** | `cargo-deny` |
| **Uso** | Advisories, licencias, dependencias duplicadas/bans y fuentes |
| **Dónde** | CI |
| **Alternativa** | sólo `cargo audit` |
| **Por qué** | Cubre más políticas en una sola herramienta |

---

## 28.2 cargo-vet antes de 1.0

Recomendado cuando Symphony empiece a distribuir binarios ampliamente.

Sirve para revisar confianza/supply chain en dependencias críticas.

No es necesario bloquear Phase 0 por esto.

---

## 28.3 GitHub Dependabot

Activarlo para:

- Cargo;
- GitHub Actions;
- GUI futura npm/pnpm.

Alternativa: Renovate.

Renovate es más configurable; Dependabot es suficiente y nativo para empezar.

---

# 29. CI

## 29.1 GitHub Actions

| Campo | Decisión |
|---|---|
| **Elección** | GitHub Actions |
| **Uso** | Build/test/lint/security/release multi-OS |
| **Por qué** | Integración natural con repositorio GitHub y runners Windows/Linux/macOS |
| **Alternativa** | Buildkite |
| **Por qué no** | Más infraestructura para un proyecto open source local-first |

### Matriz mínima

```text
ubuntu-latest
windows-latest
macos-latest
```

### Por qué NO ejecutar sólo Linux

Los componentes críticos son precisamente:

- PTY;
- process trees;
- named pipes;
- signals;
- Job Objects;
- worktrees;
- paths.

Mockearlos no sustituye runners reales.

---

## 29.2 Cache CI

Usar `Swatinem/rust-cache` o equivalente bien mantenido.

En workflows de release/security, pinnear actions por SHA.

---

# 30. Releases y distribución

## 30.1 dist / cargo-dist

| Campo | Decisión |
|---|---|
| **Elección** | `dist` (cargo-dist) |
| **Uso** | Builds reproducibles de releases, archives e installers |
| **Dónde** | release pipeline |
| **Por qué** | Automatiza gran parte del packaging cross-platform de binarios Rust |
| **Alternativa** | scripts manuales |
| **Por qué no** | Es fácil que diverjan y se rompan entre OS |

### Canales iniciales recomendados

- `.tar.xz` / `.zip`;
- shell installer;
- PowerShell installer;
- Homebrew;
- MSI/Windows installer cuando esté maduro.

### No usar npm como canal principal

Symphony es un binario nativo y no debería exigir Node para instalarse.

---

# 31. Firma y provenance

Para releases públicas:

- GitHub artifact attestations;
- checksums SHA-256 de artefactos;
- signing de Windows cuando el proyecto tenga certificado;
- notarización macOS cuando sea necesario.

BLAKE3 sigue siendo el hash interno del object store; SHA-256 se usa en release artifacts por compatibilidad del ecosistema.

---

# 32. Documentación

## 32.1 mdBook

| Campo | Decisión |
|---|---|
| **Elección** | mdBook |
| **Uso** | Documentación de arquitectura, adapters, plugins, protocol, troubleshooting |
| **Por qué** | Rust-native, simple, Markdown |
| **Alternativa** | Docusaurus |
| **Por qué no** | Node, React y más infraestructura para documentación que puede ser estática |

---

## 32.2 Diagramas: Mermaid

Usar Mermaid fuente en Markdown para:

- flujo de agentes;
- state machines;
- IPC;
- failover;
- ER simplificado;
- scheduler.

### Alternativa

PlantUML.

Mermaid es más fácil de renderizar directamente en GitHub y documentación web.

---

## 32.3 API docs: rustdoc

Cada crate pública/interna importante debe documentar sus invariantes.

Especialmente:

```text
protocol
scheduler
store
context
process
ProviderAdapter
```

---

# 33. IDE y entorno de desarrollo

## 33.1 IDE recomendado: VS Code

| Campo | Decisión |
|---|---|
| **Elección** | VS Code |
| **Uso** | Desarrollo principal |
| **Por qué** | Gratuito, cross-platform, buen ecosistema Rust y fácil onboarding de contribuidores |
| **Alternativa** | RustRover |
| **Por qué no como default** | Excelente IDE, pero propietario/comercial para algunos usuarios |

### Extensiones recomendadas

1. **rust-analyzer** — análisis, completado y navegación Rust.
2. **CodeLLDB** — debugging.
3. **Taplo** — TOML.
4. **Even Better TOML** sólo si Taplo no cubre una preferencia específica; no instalar ambos sin razón.
5. **Error Lens** — opcional, feedback inline.
6. **GitLens** — opcional; no requerido por el proyecto.

### Workspace settings

No obligar a extensiones cosméticas.

Recomendar únicamente las necesarias en `.vscode/extensions.json`.

---

# 34. Git hooks del proyecto Symphony

No depender de hooks locales para garantizar calidad.

Opcionalmente usar `pre-commit` o scripts de `cargo xtask`, pero **CI es la autoridad**.

### Alternativa recomendada

`cargo xtask check`:

```text
fmt
clippy
unit tests
schema validation
docs links
cargo-deny
```

---

# 35. GUI futura

La GUI sólo entra después de que CLI/TUI sea excelente.

## 35.1 Desktop shell: Tauri 2

| Campo | Decisión |
|---|---|
| **Elección** | Tauri 2 |
| **Uso** | GUI opcional |
| **Dónde** | `apps/gui` |
| **Por qué** | Usa el WebView del sistema y mantiene un frontend mucho más ligero que Electron |
| **Alternativa** | Electron |
| **Por qué no** | Contradice de forma directa la prioridad de bajo consumo |

La GUI no reimplementa el core.

```text
GUI
 │
 ▼
IPC/API local
 │
 ▼
symphonyd
```

---

## 35.2 Frontend GUI: SolidJS + TypeScript

| Campo | Decisión |
|---|---|
| **Elección** | SolidJS |
| **Uso** | Render de dashboard, agents, resources, context y history |
| **Por qué** | Reactividad granular y runtime pequeño |
| **Alternativa** | React |
| **Por qué no como elección ideal** | React tiene un ecosistema mayor, pero Symphony no necesita ese tamaño; Solid encaja mejor con una GUI local altamente reactiva |

Si el equipo futuro domina claramente React, cambiar a React es aceptable. La GUI no debe condicionar el core.

---

## 35.3 Bundler GUI: Vite

| Campo | Decisión |
|---|---|
| **Elección** | Vite |
| **Uso** | Desarrollo/build frontend |
| **Alternativa** | webpack |
| **Por qué** | Tooling moderno, rápido y simple |

### CSS

Vanilla CSS + design tokens.

No introducir Tailwind como requisito si la interfaz es un dashboard pequeño.

Alternativa: Tailwind si el equipo concluye que acelera la implementación sin inflar la complejidad.

---

# 36. Dependencias que NO deberían existir en el core

## 36.1 No HTTP server

No Axum/Actix en v1.

IPC local cubre CLI/TUI/hooks.

Añadir un servidor HTTP sólo si la GUI o integración externa realmente lo necesita.

---

## 36.2 No Redis

Estado y colas son locales.

SQLite + memoria cubren los requisitos.

---

## 36.3 No PostgreSQL

No hay servidor multiusuario.

---

## 36.4 No Docker como requisito

Symphony debe ejecutar las herramientas reales del usuario.

Docker puede ser una integración opcional para proyectos que lo usen, no una dependencia del runtime.

---

## 36.5 No Kubernetes

No existe un cluster.

---

## 36.6 No vector DB

FTS5/BM25 primero.

---

## 36.7 No embeddings en v1

Sólo si benchmarks reales demuestran una mejora necesaria.

---

## 36.8 No orquestador LLM

El dispatcher y router son reglas.

---

## 36.9 No Headroom como dependencia

Se reutilizan las ideas de:

- compresión por tipo;
- reversibilidad;
- retrieval.

La implementación es propia y consciente de Git/agents.

---

## 36.10 No OAuth extraction

Cada CLI mantiene su autenticación.

---

## 36.11 No Electron

Tauri si algún día hay GUI.

---

## 36.12 No gRPC

JSON local versionado es suficiente.

---

## 36.13 No libgit2 como fuente primaria

Git CLI primero.

---

## 36.14 No native plugins en el proceso del daemon

Plugins externos aislados.

---

## 36.15 No auto-updater residente

Nada debe hacer polling de red en background.

Puede existir:

```text
symphony update --check
```

o aprovechar package managers.

---

# 37. Feature flags recomendadas

No compilar todo dentro de todo.

Ejemplo conceptual:

```toml
[features]
default = ["cli-providers"]

cli-providers = []
pty = []
process-limits = []
mcp = []
gui-protocol = []

direct-provider = ["dep:reqwest", "dep:rustls"]
experimental-ml = []
```

El objetivo no es obsesionarse con el tamaño binario, sino impedir que una función futura convierta una dependencia opcional en costo obligatorio.

---

# 38. Perfiles de build

## Development

```text
debug symbols completos
incremental compilation
logs debug habilitables
```

## Release

Recomendado:

```toml
[profile.release]
opt-level = 3
lto = "thin"
```

No usar `panic = "abort"` inicialmente.

Symphony depende de cleanup y diagnóstico robusto; ahorrar unos KB no justifica reducir capacidad de recuperación.

Los símbolos de depuración pueden distribuirse aparte para releases.

---

# 39. Arquitectura de threads recomendada

Para evitar que el daemon compita con los CLIs:

```text
Tokio runtime:
  pequeño número de workers

DB:
  1 worker dedicado

Context compression/index:
  bounded workers
  prioridad baja

Resource sampling:
  timer ligero

External executors:
  procesos separados y limitados por scheduler
```

No crear pools enormes “por si acaso”.

---

# 40. Stack de desarrollo por fase

## Phase 0 — Spike

Usar solamente lo imprescindible:

```text
Rust
Tokio
clap
serde / serde_json
Git CLI
ProcessKit
sysinfo
interprocess
tempfile

events.jsonl
checkpoint.json
```

No SQLite todavía si el spike no lo necesita.

Objetivo:

- procesos;
- hooks;
- resource control;
- forced handoff.

---

## Phase 1 — Core

Agregar:

```text
Ratatui
Crossterm
SQLite
rusqlite
rusqlite_migration
ULID
tracing
TOML/toml_edit
BLAKE3
zstd
```

---

## Phase 2 — Multi-agent runtime

Completar:

```text
scheduler
OS enforcement
notify
validation engine
DAG
heartbeat/reclaim
```

---

## Phase 3 — Context Engine

Agregar:

```text
tree-sitter
FTS5/BM25
ignore
rmcp
deterministic compressors
```

---

## Phase 4 — Failover/profiles

No necesita una gran dependencia nueva.

Construir:

```text
provider health
error parsers
routing rules
profile scoring
quota reserve
```

---

## Phase 5 — Providers restantes

Agregar adapters:

```text
Kimi
Antigravity
Copilot
```

sin rediseñar core.

---

## Phase 6 — Experimentos

Sólo aquí evaluar:

```text
Decision model
ML compression
embeddings
semantic index
performance learning
```

Cada uno detrás de benchmark y feature flag.

---

## Phase 7 — GUI

```text
Tauri 2
SolidJS
TypeScript
Vite
```

---

# 41. Stack de testing por tipo de componente

| Componente | Herramienta principal |
|---|---|
| Core/state | Rust unit tests + proptest |
| CLI commands | trycmd |
| TUI rendering | insta snapshots |
| IPC | integration tests + proptest framing |
| DB | tempfile + real SQLite |
| Scheduler | deterministic simulation + proptest |
| Process control | real subprocess integration tests |
| Windows limits | Windows CI runner |
| Linux limits | Linux CI runner |
| macOS process handling | macOS CI runner |
| Provider adapters | recorded sanitized fixtures + opt-in live tests |
| Failover | fake provider + real forced-kill E2E |
| Context compression | golden corpus + Criterion |
| MCP | protocol integration tests |
| Security parsers | cargo-fuzz |
| Release | smoke test installed binary on each OS |

---

# 42. Provider adapter testing strategy

Nunca hacer que CI normal consuma suscripciones reales.

Tres niveles:

### L1 — Fixture

Outputs/hook payloads sanitizados.

```text
fast
deterministic
always in CI
```

### L2 — Fake CLI

Ejecutable fixture que simula:

```text
stream
tool request
429
quota exhausted
crash
hang
auth error
```

Permite probar Symphony end-to-end.

### L3 — Live contract

Opt-in/manual/nightly cuando haya credenciales del maintainer.

Sirve para detectar cambios de CLI upstream.

---

# 43. Database tooling

No usar un ORM.

El esquema ya está diseñado explícitamente y contiene:

- CHECKs;
- índices parciales;
- FKs;
- tablas FTS;
- queries operativas específicas.

Usar SQL escrito a mano y estructuras Rust tipadas.

### Alternativa

SeaORM/Diesel.

### Por qué no

La abstracción no aporta lo suficiente y puede dificultar SQLite-specific features.

---

# 44. ER diagram y esquema

Mantener:

```text
migrations/*.sql          → fuente ejecutable
docs/database.md          → explicación humana
docs/er/*.mmd             → diagrama Mermaid generado/actualizado
```

El HTML del ER puede generarse como artefacto de documentación, pero no debe ser fuente de verdad.

---

# 45. State machines

Implementar estados como enums Rust fuertes:

```rust
enum AgentState {
    Created,
    Ready,
    Running,
    WaitingProvider,
    WaitingResource,
    WaitingDependency,
    Testing,
    Blocked,
    Paused,
    Completed,
    Failed,
    Cancelled,
}
```

No manejar estados como strings arbitrarios dentro del core.

SQLite mantiene strings por legibilidad, pero la capa Rust valida la conversión.

---

# 46. Protocol/version compatibility

Tanto IPC como plugins/adapters externos deben declarar versión.

Ejemplo:

```text
protocol_version = 1
plugin_api = 1
```

Cambios incompatibles requieren:

- versión nueva;
- mensaje de error claro;
- nunca interpretar silenciosamente payload desconocido.

---

# 47. Seguridad del daemon local

Aunque sea local:

- socket/pipe sólo accesible por el usuario actual;
- validar todas las requests;
- límites de tamaño de frames;
- timeouts;
- nunca ejecutar un comando recibido desde un plugin sin pasar por políticas;
- sanitize paths;
- canonicalize worktree roots;
- impedir path traversal en `ctx://`.

### Alternativa

“Es localhost, confiar en todo”.

No es aceptable.

---

# 48. ANSI y output de CLIs

Los CLIs pueden emitir secuencias ANSI.

Política:

```text
raw stream → parser/sanitizer → TUI/log
```

Nunca reproducir secuencias terminal arbitrarias sin filtrar.

Usar una librería de parsing/stripping ANSI pequeña o un parser VTE.

### Alternativa

Guardar/renderizar stdout tal cual.

### Por qué no

Puede romper la TUI y crea riesgos de terminal escape injection.

---

# 49. Timeouts y retries

Usar políticas explícitas.

No añadir una librería “retry everything”.

Cada dominio decide:

```text
IPC request timeout
provider startup timeout
hook hold timeout
health probe
process graceful-stop timeout
force-kill timeout
DB busy handling
```

Retries deben ser:

- acotados;
- con backoff donde tenga sentido;
- registrados como eventos;
- nunca infinitos.

---

# 50. Networking

En v1 el core **no necesita cliente HTTP genérico**.

La red la usan:

- CLIs oficiales;
- MCPs que el usuario configure.

Esto ayuda a cumplir la promesa “no hidden network calls”.

Si Direct Mode aparece:

```text
provider-direct crate
→ reqwest + rustls
```

separado y auditable.

---

# 51. Herramientas externas requeridas en runtime

## Requeridas

1. `git`
2. al menos uno de:
   - Claude Code
   - Codex CLI
   - Antigravity CLI
   - Kimi Code
   - GitHub Copilot CLI

## Detectadas según proyecto

- npm / pnpm / yarn / bun
- cargo
- Python/uv/poetry
- Go
- Maven/Gradle
- etc.

Symphony no debe empaquetarlas.

---

# 52. Herramientas opcionales de usuario

## ripgrep (`rg`)

Puede aprovecharse como acelerador si está instalado.

Pero **no debe ser requisito** porque Symphony ya puede hacer traversal/search internamente.

### Alternativa

`ignore` + búsqueda propia/FTS.

---

# 53. Changelog y releases

## Recomendado: git-cliff

| Campo | Decisión |
|---|---|
| **Elección** | `git-cliff` |
| **Uso** | CHANGELOG desde commits/tags |
| **Alternativa** | changelog manual |
| **Por qué** | Reduce trabajo repetitivo de release |

No obligar a Conventional Commits perfectos en cada contribución; sí exigir PR titles útiles y squash merge coherente.

---

# 54. Checks adicionales recomendados

## cargo-semver-checks

Añadir cuando exista una API pública estable de plugins/crates.

## typos-cli

Para errores tipográficos en código/docs.

## lychee

Para links rotos de documentación.

Son herramientas de CI/desarrollo; no forman parte del binario distribuido.

---

# 55. Stack de GUI vs stack de runtime

Debe mantenerse una frontera estricta:

```text
                  ┌──────────────┐
                  │  Tauri GUI   │
                  └──────┬───────┘
                         │
┌──────────────┐         │       ┌──────────────┐
│ CLI / TUI    ├─────────┼──────►│  symphonyd   │
└──────────────┘   IPC   │       └──────────────┘
                         │
                  same protocol
```

Ninguna feature crítica vive exclusivamente en la GUI.

---

# 56. Stack tecnológico por componente — resumen maestro

| Componente | Elección ideal | Para qué | Dónde | Razón principal | Alternativa |
|---|---|---|---|---|---|
| Lenguaje | Rust 2024 | Todo el runtime | Core | Bajo overhead + control OS | Go |
| Async | Tokio | I/O/concurrencia | Daemon | Ecosistema/madurez | smol |
| Cancelación | tokio-util | Lifecycle | Runtime | Cancelación estructurada | watch channels |
| CLI | clap | Commands | `symphony` | Maduro/completo | argh |
| TUI | Ratatui | Dashboard | CLI | Rica y ligera | Ink |
| Terminal | Crossterm | Input/render | TUI | Cross-platform | termion |
| Errors | thiserror + miette | Diagnósticos | Todo | Tipos + UX | anyhow |
| IPC | interprocess | Pipe/socket local | Protocol | Win/Unix unificado | Axum localhost |
| Wire | serde_json | Mensajes IPC | Protocol | Inspeccionable | Protobuf |
| Event bus | Tokio channels | Eventos internos | Core | Cero infra | NATS |
| Process manager | ProcessKit | CLIs/árboles | Process | PTY + limits + kill tree | APIs OS propias |
| Windows OS | windows-sys | Job Objects | Process | Bajo nivel | windows |
| Unix OS | nix | signals/groups | Process | Safe wrappers | libc |
| Metrics | sysinfo | CPU/RAM | Resource Manager | Portable | APIs nativas |
| DB | SQLite | Estado | Store | Embedded/crash-safe | PostgreSQL |
| DB binding | rusqlite | SQL | Store | Simple/directo | SQLx |
| Migrations | rusqlite_migration | Schema | Store | Integración directa | refinery |
| IDs | ULID | entidades | Store | Sortable/local | UUIDv7 |
| Config | TOML | user config | Files | Legible | YAML |
| Config edit | toml_edit | preservar comentarios | Config | UX | resave serde |
| Serialization | serde | tipos | Todo | estándar | manual |
| Hash | BLAKE3 | object addressing | Object store | rápido | SHA-256 |
| Compression | zstd | blobs | Object store | ratio/velocidad | gzip |
| Atomic files | tempfile | blobs | Object store | crash-safe | direct write |
| Git | system git CLI | worktrees/diff/merge | Git | fidelidad | libgit2 |
| Watcher | notify | cambios | Project state | nativo/cross-OS | polling |
| Walk | ignore | files | Context | .gitignore-aware | walkdir |
| AST | tree-sitter | contexto código | Context | multi-language | LSP |
| Search | FTS5/BM25 | retrieval | Context/SQLite | sin servidor | vector DB |
| MCP | rmcp | context/tools | MCP | SDK oficial Rust | custom MCP |
| Skills | SKILL.md + TOML | skills compartidas | Files | editable/versionable | DB-only |
| Plugins | subprocess + JSON | extensiones | Plugins | crash isolation | dylib |
| Router | reglas propias | model choice | Routing | microsegundos | decision model |
| Scheduler | propio + semaphores | recursos | Scheduler | requisito específico | no scheduler |
| Logging | tracing stack | observabilidad | Daemon | async spans | log |
| Test runner | cargo-nextest | tests | CI/dev | aislamiento/velocidad | cargo test |
| CLI tests | trycmd | command UX | Tests | fixtures | assert_cmd |
| Snapshots | insta | TUI/outputs | Tests | fácil review | golden files |
| Property tests | proptest | invariants | Tests | state safety | example-only |
| Fuzzing | cargo-fuzz | parsers | Security | coverage-guided | AFL |
| Benchmarks | Criterion | microperf | Bench | estadísticas | cargo bench |
| Coverage | cargo-llvm-cov | coverage | CI | LLVM/nextest | tarpaulin |
| Lint | Clippy | calidad | CI | oficial | manual lint |
| Format | rustfmt | estilo | CI | oficial | manual |
| Dependency security | cargo-deny | advisory/license | CI | amplio | cargo audit |
| CI | GitHub Actions | multi-OS | Cloud CI | integración repo | Buildkite |
| Release | dist/cargo-dist | installers | Release | cross-platform | scripts |
| Docs | mdBook | manual | Docs | Markdown/Rust | Docusaurus |
| Diagrams | Mermaid | arquitectura | Docs | GitHub-friendly | PlantUML |
| IDE | VS Code | desarrollo | local | accesible | RustRover |
| Rust IDE plugin | rust-analyzer | IDE | local | estándar | RustRover engine |
| Debugger | CodeLLDB | IDE | local | Rust/native | platform debugger |
| TOML IDE | Taplo | IDE | local | schema/format | basic TOML |
| GUI shell | Tauri 2 | GUI futura | Desktop | ligera | Electron |
| GUI frontend | SolidJS | dashboard | GUI | reactividad granular | React |
| GUI tooling | Vite | frontend | GUI | rápido/simple | webpack |
| HTTP Direct mode | reqwest + rustls | futuro | Provider-direct | estándar Rust | hyper manual |

---

# 57. Stack explícitamente descartado

| Tecnología/enfoque | Estado | Motivo |
|---|---|---|
| Node.js/TypeScript como core | ❌ | Suma otro runtime pesado permanente |
| Python como daemon final | ❌ | Distribución y control OS menos ideales |
| Electron | ❌ | RAM/CPU incompatible con prioridad |
| PostgreSQL | ❌ | Requiere servidor |
| Redis | ❌ | Cola/estado local no lo necesita |
| NATS/Kafka | ❌ | Infraestructura innecesaria |
| Docker obligatorio | ❌ | Debe trabajar en host real |
| Kubernetes | ❌ | No existe cluster |
| SQLx como primera opción | ❌ | Async DB/pooling no necesario |
| ORM | ❌ | Esquema específico y SQLite features |
| libgit2 como backend primario | ❌ | Git CLI es la referencia real |
| gRPC/Protobuf | ❌ | IPC local simple |
| REST server en v1 | ❌ | Named pipes/sockets son suficientes |
| Vector DB | ❌ v1 | FTS5/BM25 primero |
| Embeddings | ❌ v1 | RAM/latencia sin evidencia |
| LLM router | ❌ critical path | Reglas son mejores inicialmente |
| LLM orchestrator | ❌ | Impredecible y costoso |
| Headroom dependency | ❌ | Implementación propia especializada |
| Modelos locales grandes | ❌ | Compiten con los agentes por RAM |
| OAuth/token extraction | ❌ | Seguridad/ToS |
| Native dynamic plugins | ❌ v1 | Crash/ABI risk |
| Wasmtime plugin host | ⏸ futuro | Sólo si el ecosistema lo justifica |
| OpenTelemetry remoto | ❌ default | No telemetría |
| Sentry | ❌ default | No datos saliendo |
| Background updater | ❌ | No tráfico oculto |
| Custom allocator | ❌ inicialmente | Sólo si benchmark lo justifica |

---

# 58. Stack de dependencias Cargo aproximado

No es un `Cargo.toml` definitivo; muestra la intención del stack.

```toml
[workspace.dependencies]
tokio = { version = "1", default-features = false }
tokio-util = "0.7"

clap = { version = "4", features = ["derive"] }
ratatui = "0.30"
crossterm = "0.29"

serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml_edit = { version = "0.25", features = ["serde"] }

thiserror = "2"
miette = "7"

interprocess = { version = "2", features = ["tokio"] }

rusqlite = { version = "0.40", features = ["bundled"] }
rusqlite_migration = "2"
ulid = "3"

blake3 = "1"
zstd = "0.14"
tempfile = "3"

tracing = "0.1"
tracing-subscriber = "0.3"
tracing-appender = "0.2"

sysinfo = "0.39"
notify = "8"
ignore = "0.4"

tree-sitter = "0.27"

rmcp = "3"

regex = "1"

# platform-specific / process layer:
processkit = "3"

[target.'cfg(windows)'.dependencies]
windows-sys = "0.61"

[target.'cfg(unix)'.dependencies]
nix = "0.31"
```

**Nota:** las versiones concretas deben quedar fijadas en `Cargo.lock` y revisarse antes de iniciar implementación. El proyecto no debe actualizar automáticamente majors.

---

# 59. Plugins / tooling de desarrollo no-runtime

```text
cargo-nextest
cargo-llvm-cov
cargo-deny
cargo-fuzz
cargo-semver-checks        # cuando exista API estable
git-cliff
typos-cli
lychee
dist / cargo-dist
```

IDE:

```text
VS Code
rust-analyzer
CodeLLDB
Taplo
```

CI:

```text
GitHub Actions
rust-cache
native Windows/Linux/macOS runners
```

---

# 60. Decisión sobre ProcessKit

Ésta es probablemente la dependencia que más merece un spike específico.

### Razón

Toca la parte más delicada de Symphony:

```text
process trees
PTY
Job Objects
cgroups
resource limits
kill semantics
stdout/stderr
```

### Recomendación

Usarlo en Phase 0, pero detrás de `ProcessSupervisor`.

### Gate

Si no demuestra:

- kill sin procesos huérfanos;
- streaming estable;
- buen comportamiento Windows/Linux/macOS;
- overhead bajo;
- límites suficientemente predecibles;

entonces reemplazar gradualmente por:

```text
tokio::process
+ windows-sys
+ nix
+ PTY abstraction
```

antes de Phase 1.

No acoplar el producto a ProcessKit hasta superar ese benchmark.

---

# 61. Decisión sobre Rust

También debe confirmarse con el spike, pero la recomendación final sigue siendo Rust.

La razón no es:

> “Rust mágicamente deja correr cuatro agentes”.

La razón es:

> “Si los cinco CLIs externos ya son pesados, el coordinador no debe convertirse en otro consumidor significativo y además necesita excelente control de procesos”.

Lo que realmente permitirá varios agentes será:

```text
scheduler
resource enforcement
shared state
shared watchers
shared context
lazy executor lifecycle
worktree isolation
```

Rust ayuda a que esa capa sea pequeña y predecible.

---

# 62. Arquitectura tecnológica consolidada

```text
                              USER
                               │
                        symphony CLI/TUI
                   clap + Ratatui + Crossterm
                               │
                  interprocess local IPC / JSON
                               │
                               ▼
                         ┌───────────┐
                         │ symphonyd │
                         │ Rust      │
                         │ Tokio     │
                         └─────┬─────┘
                               │
      ┌────────────────────────┼──────────────────────────┐
      │                        │                          │
      ▼                        ▼                          ▼
 Store                   Agent Runtime                Event Bus
 SQLite/rusqlite          ProcessKit                  Tokio channels
 WAL                      Git CLI                     tracing
      │                   sysinfo                         │
      │                        │                          │
      ▼                        ▼                          ▼
 Checkpoints              Scheduler                 Provider adapters
 Object store             semaphores/queues         Claude Code
 BLAKE3+zstd              OS limits                 Codex
      │                                              Antigravity
      ▼                                              Kimi
 Context Engine                                      Copilot
 tree-sitter
 FTS5/BM25
 deterministic
 compressors
      │
      ▼
 Context MCP
 rmcp
```

GUI futura:

```text
Tauri 2
  └─ SolidJS + TypeScript + Vite
       │
       └──────── misma API/IPC local ────────► symphonyd
```

---

# 63. Qué debe instalar un desarrollador para construir Symphony

## Mínimo

```text
Git
Rust stable
LLVM/toolchain necesario por plataforma
```

## Recomendado

```text
VS Code
rust-analyzer
CodeLLDB
Taplo
cargo-nextest
cargo-deny
cargo-llvm-cov
```

## Para integración real de providers

```text
Claude Code CLI
Codex CLI
Kimi Code CLI
Antigravity CLI
GitHub Copilot CLI
```

No hacen falta todos al mismo tiempo para desarrollar el core.

---

# 64. Qué debe instalar un usuario final

```text
Symphony binary
Git
al menos un provider CLI oficial autenticado
```

Nada más debe ser obligatorio.

En particular:

```text
NO Node por Symphony
NO Python por Symphony
NO database server
NO Docker
NO Redis
NO JVM
NO browser runtime propio
```

Si el proyecto del usuario necesita esas tecnologías, eso es independiente de Symphony.

---

# 65. Decisión final

El stack ideal de Symphony CLI debe parecerse más al de una **herramienta de sistemas local** que al de una aplicación web o una plataforma SaaS.

La combinación recomendada es:

> **Rust + Tokio + Ratatui + interprocess + ProcessKit + Git CLI + SQLite/rusqlite + BLAKE3/zstd + tree-sitter + FTS5/BM25 + rmcp + tracing**, con adapters propios sobre los CLIs oficiales y un scheduler determinístico.

La arquitectura deliberadamente evita infraestructura que no sea imprescindible:

> **sin servidor web en el core, sin Postgres, sin Redis, sin Docker obligatorio, sin Electron, sin vector DB, sin embeddings, sin orquestador LLM, sin telemetría y sin manipular las credenciales de los proveedores.**

Esto es coherente con la prioridad fundamental del proyecto:

> Si Symphony no permite trabajar con varios agentes mientras la computadora sigue siendo utilizable, la arquitectura falla independientemente de cuántas funciones tenga.

---

# 66. Fuentes técnicas que conviene mantener como referencia

Documentación/proyectos oficiales a consultar durante implementación:

- Rust: <https://www.rust-lang.org/>
- Tokio: <https://tokio.rs/>
- clap: <https://docs.rs/clap/>
- Ratatui: <https://ratatui.rs/>
- Crossterm: <https://docs.rs/crossterm/>
- SQLite: <https://www.sqlite.org/>
- rusqlite: <https://docs.rs/rusqlite/>
- tree-sitter: <https://tree-sitter.github.io/tree-sitter/>
- MCP Rust SDK (`rmcp`): <https://github.com/modelcontextprotocol/rust-sdk>
- Tauri: <https://tauri.app/>
- Git: <https://git-scm.com/>
- cargo-nextest: <https://nexte.st/>
- cargo-llvm-cov: <https://github.com/taiki-e/cargo-llvm-cov>
- cargo-deny: <https://github.com/EmbarkStudios/cargo-deny>
- dist/cargo-dist: <https://opensource.axo.dev/cargo-dist/>

La documentación de cada provider CLI debe tratarse como fuente autoritativa para sus hooks, autenticación, ejecución headless, sesiones y modelos disponibles, ya que esos contratos pueden cambiar independientemente de Symphony.
