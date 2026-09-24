# CONSTRAINTS — Symphony CLI

Contrato de calidad y rendimiento. Cada regla dice cómo se comprueba. Una regla sin chequeo automático se revisa a mano al cerrar cada fase (PLAN §4.6).

**Bajar un umbral, silenciar un lint o saltarse un test requiere un ADR aprobado por Leo.** Nunca se borra ni se debilita un test para que pase.

## 1. Reglas de arquitectura (PLAN §2, IDEA §2)

| # | Regla | Cómo se comprueba |
|---|---|---|
| A1 | **AGENT ≠ MODEL.** `agents` nunca guarda el modelo actual; vive en el `agent_run` abierto (DB §3.C) | Revisión de migraciones; test de esquema desde P03 |
| A2 | La verdad vive en Symphony (SQLite + Git + object store), no en el chat del modelo | Revisión |
| A3 | Checkpoints incrementales, nunca generados "al fallar" | Test E2E de kill sin cleanup desde P06 |
| A4 | Un worktree por agente | Test de integración desde P04 |
| A5 | El trabajo pesado pasa por el scheduler | Test desde P08 |
| A6 | Router, dispatcher, scheduler y parsers de error son deterministas. Sin LLM en el camino crítico | Revisión + `cargo deny` (sin SDKs de LLM en el core) |
| A7 | Todo lo "inteligente" va detrás de feature flag o config y tiene fallback | Revisión |
| A8 | Sin telemetría ni tráfico de red. El core no hace llamadas HTTP (STACK §50) | `cargo deny` prohíbe clientes HTTP en el core |
| A9 | Nunca leer, copiar ni extraer credenciales o tokens OAuth de proveedores | Revisión; grep de rutas de credenciales en code review |
| A10 | Un solo escritor en SQLite. Hooks y procesos externos nunca abren la DB | Revisión; solo `crates/store` depende de `rusqlite` |

## 2. Prohibido en el core (STACK §36)

Node/Python como runtime · Electron · PostgreSQL · Redis · NATS/Kafka · Docker obligatorio · Kubernetes · ORM · gRPC · servidor HTTP en v1 (Axum/Actix) · vector DB · embeddings en v1 · orquestador LLM · Headroom como dependencia · plugins nativos in-process · libgit2 como backend principal (se usa el `git` del sistema) · auto-updater residente o polling de red en background.

**Cómo se comprueba:** `[bans]` en `deny.toml` (desde P00.S6) + `cargo deny check` en CI.

## 3. Código

| # | Regla | Cómo se comprueba |
|---|---|---|
| C1 | `cargo fmt --check` limpio | `cargo xtask check`, CI |
| C2 | `cargo clippy --workspace --all-targets -- -D warnings` limpio | `cargo xtask check`, CI |
| C3 | Sin `unwrap()`, `expect()`, `todo!()`, `unimplemented!()`, `dbg!()` en rutas de runtime. En tests se permite `unwrap`/`expect` | `[workspace.lints.clippy]`: `unwrap_used`, `expect_used`, `todo`, `unimplemented`, `dbg_macro` = deny; `clippy.toml` con `allow-unwrap-in-tests`/`allow-expect-in-tests` |
| C4 | Sin placeholders ni stubs commiteados como "terminado". Lo pendiente va a la bitácora y a un issue | Revisión al cerrar el paso |
| C5 | Errores: `thiserror` en crates de librería, `miette` en binarios | Revisión |
| C6 | `unsafe` prohibido por defecto. Solo en módulos de OS aislados que lo exijan, con comentario `// SAFETY:` obligatorio (STACK §27) | `[workspace.lints.rust] unsafe_code = "deny"`; el módulo que lo necesite hace `#[allow(unsafe_code)]` local y `clippy::undocumented_unsafe_blocks = "deny"` |
| C7 | Nunca `panic = "abort"` en release (STACK §38) | `Cargo.toml` `[profile.release]` |

## 4. Dependencias (PLAN §2.13, STACK §28, §58)

| # | Regla | Cómo se comprueba |
|---|---|---|
| D1 | Una crate fuera de STACK §58 se justifica en la bitácora de fase ("Dependencias agregadas") | Revisión al cerrar fase |
| D2 | Una crate de peso (runtime, red, ML) necesita ADR | Revisión |
| D3 | Licencias permitidas, sin advisories abiertos, sin fuentes fuera de crates.io | `cargo deny check` |
| D4 | Toda dependencia se declara en `[workspace.dependencies]` y se crea el crate solo en la fase que lo usa | Revisión |

## 5. Presupuestos de rendimiento (IDEA §3, §6; STACK §39)

Son objetivos medibles, no garantías. Se miden con el harness E2E (STACK §25.2) desde P08.

| # | Presupuesto | Umbral | Se mide desde |
|---|---|---|---|
| R1 | **Métrica principal:** 3 agentes, 3 worktrees, 2–3 proveedores y la máquina sigue usable | Consumo total con Symphony ≤ los mismos agentes corriendo sueltos; UI del sistema responde | P08, P10, P13, P16 |
| R2 | RAM del daemon en idle | < 100 MB | P02 |
| R3 | Decisión de routing | milisegundos (objetivo < 10 ms p99) | P10 |
| R4 | Latencia de evento (hook → bus → TUI) | milisegundos (objetivo < 50 ms p99) | P05 |
| R5 | Threads: Tokio con pocos workers, 1 worker de DB, compresión/índice con workers acotados y prioridad baja. Sin pools grandes "por si acaso" | Revisión | P02 |

Los objetivos p99 de R3 y R4 son una concreción de "milisegundos" (IDEA §6). Si una medición real muestra que no aplican, se ajustan con ADR.

## 6. Pruebas (PLAN §6, STACK §42)

- CI nunca gasta suscripciones reales: usa fixtures y `fake-agent`.
- Tests live solo con `SYMPHONY_LIVE=1` y permiso de Leo.
- Cada paso con lógica deja tests; cada comando CLI nuevo, un `trycmd`; cada vista TUI, un snapshot `insta`.
- Una afirmación de "funciona" sin comando de verificación en la bitácora no cuenta.
