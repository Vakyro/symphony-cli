# P07 · TUI y release v0.1

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p07-tui |
| Inicio / cierre | 2026-09-25 / — |
| Agentes que trabajaron | claude-code/opus-5.5 (S1–S7) |
| Tag | — |
| Docs usados | FLOW §1–§8, §12, §13, §16, §18, §20 (Journey A), §21; PLAN P07; DB §3 (providers, models, recovery_items, executor_changes, tool_calls) |

## Pasos

### P07.S1 · Arquitectura TUI — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-25
- **Qué se hizo:**
  - **Crate `symphony-tui`** con arquitectura tipo Elm:
    - `app.rs`: estado y lógica, sin E/S. `App::update(Msg) -> Vec<Call>`. `Msg` = tecla, respuesta IPC, evento del bus, tick o desconexión; `Call` = método IPC + params.
    - `ui.rs`: render puro de `&App`.
    - `io.rs`: dos conexiones IPC. Una hace los requests en orden; la otra está suscrita al bus.
    - `lib.rs`: loop de terminal (`ratatui::init/restore`), un hilo que lee el teclado y un tick de 1 s. Redibuja solo cuando llega un mensaje y junta las ráfagas en un solo dibujo.
  - **La TUI nunca abre la base.** El crate no depende de `symphony-store` ni de `rusqlite`, así que no puede hacerlo (CONSTRAINTS A10).
  - **Suscripción en el daemon** (antes respondía `unsupported`): con `Subscribe{topics:["agent.event"]}` la conexión pasa a recibir solo eventos del bus. Cada evento lleva `project_id`, `agent_id`, `run_id`, `type`, `source` y `occurred_at`; el contenido no se manda. Si el suscriptor se atrasa, llega `bus.lagged`. Un tópico desconocido devuelve el error `unknown_topic`.
  - **Refresco:** un evento marca la vista como sucia y el siguiente tick la relee. Mientras haya respuestas pendientes no se piden más.
  - **Sondeo de respaldo cada 3 s**, marcado con `ponytail:`: los cambios de estado de un agente no pasan por el bus, porque se escriben en 15 sitios del runtime. Si el sondeo pesa, la solución es que el runtime publique `agent.changed`.
  - **Métodos IPC nuevos para las vistas** (`crates/daemon/src/views.rs` + `server.rs`): `project.status`, `project.init`, `models.list`, `provider.set_enabled`, `agent.activity`, `agent.history`, `recovery.list` y `recovery.act`.
  - **CLI:** `symphony` sin argumentos abre la TUI con dos conexiones y autoarranca el daemon. Sin terminal interactiva, explica por qué no la abre (trycmd `no-args`).
- **Archivos clave:** `crates/tui/src/{lib,app,ui,io}.rs`, `crates/daemon/src/{server,views}.rs`, `crates/cli/src/main.rs`
- **Cómo se verificó:** `crates/tui/tests/daemon.rs::tui_drives_a_real_daemon_over_ipc_only`: daemon real + fake-agent como `claude`, la TUI solo por IPC. Recorre Launch → First-run → Home → `spawn` → vista del agente → evento del bus recibido → Conversación → Home con el agente, y además el tópico desconocido y un enum inválido. Pasa en 10.7 s.

### P07.S2 · Launch, First-run y Provider Setup — ✅
- **Qué se hizo:** vistas 01–03 con las ramas de FLOW §4:
  - **Proyecto reconocido** (`project.toml` existe) → Home.
  - **Sin Symphony** → `[i]` inicializar, `[o]` abrir sin configurar, `[q]` salir.
  - **No es un repo** → muestra la ruta y cómo seguir.
  - **Estado inconsistente** (items abiertos en Recovery) → Recovery antes del Home.
  - **First-run** pide nombre, perfil de rendimiento y failover, y los guarda en `project.toml` (`symphony_core::init_project`, que reemplaza a `create_project`). Si algún proveedor necesita atención, pasa por Provider Setup; si no, va directo al Home.
  - **Provider Setup** muestra un mensaje y las acciones de cada estado (FLOW §4.3): `r` reintenta la detección y `d` activa o desactiva el proveedor. Desactivarlo sobrevive a `providers.refresh`, porque el upsert no toca `enabled`. Se puede seguir con un solo proveedor.
- **Verificación:** snapshots `view_01_*`, `view_02_first_run` y `view_03_provider_setup`, más `inconsistent_state_opens_recovery_before_home` y `open_without_setup_goes_home_without_listing_other_projects`.

### P07.S3 · Home — ✅
- **Qué se hizo:** vista 04 con la lista de agentes (activos primero y después un grupo «Completados»), la franja de proveedores, la franja de sistema y la barra de comandos (`:`).
  - Estado vacío con una acción dominante: «Pulsa n para crear el primero».
  - Banner no intrusivo cuando hay problemas por recuperar o proveedores que necesitan atención.
  - Los agentes que esperan o están bloqueados muestran su razón humana (`state_reason` o la frase de FLOW §7).
  - La franja de sistema es un placeholder hasta P08, como pide el plan.
  - **Barra de comandos:** `spawn [proveedor/modelo] <tarea>`, `agent <n>`, `new`, `providers`, `recovery` y `quit`.
- **Verificación:** snapshots `view_04_home_empty` y `view_04_home_with_agents_and_a_problem`; `created_agent_opens_its_view`.

### P07.S4 · New Agent y Model Picker básico — ✅
- **Qué se hizo:** vistas 05 y 13, con modelo exacto o «decidir después». Los profiles aparecen como «llegan en v0.5».
  - Un modelo no disponible **nunca se sustituye**: se avisa y se ofrece elegir otro, esperar o cancelar (FLOW §8.2).
  - `no_eligible_provider` → Provider Setup sin perder la tarea escrita; al continuar se vuelve al formulario.
  - `exact_model_unavailable` → aviso con las opciones.
- **Verificación:** snapshots `view_05_new_agent` y `view_13_model_picker`; `an_unavailable_exact_model_is_never_substituted` y `no_eligible_provider_keeps_the_task_and_comes_back`.

### P07.S5 · Vista de agente — ✅
- **Qué se hizo:** vistas 06–09 y 12 como pestañas de una sola vista (Resumen, Conversación, Actividad, Cambios, Historial).
  - Encabezado con estado y razón, tarea, executor, failover, workspace y edad del checkpoint.
  - Acciones: `m` manda un mensaje, `p` pausa o reanuda según el estado, `s` cambia el modelo (Model Picker), `d` abre el diff y `x x` detiene el agente (pide confirmación).
  - La Conversación muestra el separador `EXECUTOR_CHANGE` (FLOW §13.4) y arranca desde lo más nuevo.
- **Verificación:** snapshots `view_06`–`view_09` y `view_12_24`; `stop_needs_confirmation_and_switch_uses_the_picker`, `pause_toggles_with_the_state` y `tabs_fetch_their_data`.

### P07.S6 · Providers y Recovery Center — ✅
- **Qué se hizo:**
  - Vista 22 (Providers).
  - Vista 24 (Failover Event): anterior → nuevo executor, motivo con el tipo de falla y edad del checkpoint, dentro de la pestaña Historial y con el separador en la Conversación.
  - Vista 30 (Recovery Center) con acciones `restart`, `reclaim`, `stop` y `dismiss` sobre `recovery.act`. `stop` cierra el item como `ARCHIVE`; `restart` y `reclaim` lo resuelven con la lógica del runtime (P06.S6).
- **Verificación:** snapshots `view_22_providers` y `view_30_recovery_center`; `recovery_actions_and_disconnection`. En el daemon, `recovery_center_and_tui_views_over_ipc` cubre `recovery.list` filtrado por proyecto, `restart` sin agente con error claro, acción inválida, `dismiss`, doble `dismiss`, desactivar un proveedor que sobrevive al refresh, sus modelos que pasan a no disponibles, `project.status` fuera de un repo y `agent.history` de un agente inexistente.

### P07.S7 · Snapshots y E2E — 🟡
- **Hecho:**
  - 16 snapshots `insta` (una o más por vista) revisados a mano.
  - E2E con fake-agent, que es la parte de Journey A que existe en v0.1: abrir, inicializar, crear agente, verlo trabajar y volver al Home.
- **Pendiente:**
  - Probar la TUI en una **terminal real**. Los snapshots y el E2E no ejercitan el modo raw, el redimensionado ni las teclas de Windows; no se pudo hacer desde la herramienta del agente.
  - Prueba manual con **Claude Code real** (necesita permiso de Leo).
  - Documentar Journey A con capturas.
  - El resto de Journey A (validación, merge preflight, integrar y archivar) es P08.

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|
| TUI solo por IPC contra un daemon real, con eventos del bus | `tui_drives_a_real_daemon_over_ipc_only` | ✅ |
| Las 13 vistas de P07 renderizan (01–09, 12, 13, 22, 24, 30) | 16 snapshots `insta` | ✅ |
| Ramas de FLOW §4, §6, §8.2, §16 | 11 tests de lógica en `crates/tui/tests/views.rs` | ✅ |
| Recovery Center y vistas por IPC | `recovery_center_and_tui_views_over_ipc` | ✅ |

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|
| TUI no probada en una terminal real | Posibles detalles de teclado o redimensionado en Windows | `symphony` en una terminal | Leo la prueba (P07.S7) |
| Los cambios de estado no pasan por el bus | La TUI los ve con hasta 3 s de retraso | pausar un agente desde la CLI con la TUI abierta | Sondeo marcado `ponytail:`; `agent.changed` si hace falta |
| «Archive» de completados | Home agrupa los completados pero no los archiva | — | Llega con merge (P08) |

## Decisiones tomadas
- **Elm en vez de widgets con estado:** `update` puro devuelve `Call`s. Toda la lógica se prueba sin terminal ni daemon, y la misma lógica serviría para la GUI (FLOW regla 9).
- **Dos conexiones IPC:** la de suscripción queda dedicada (el daemon ya no atiende requests en ella). Así el protocolo sigue siendo request/response simple.
- **El evento no lleva contenido:** la TUI relee por request. El bus no expone texto del agente a otros clientes, y los requests ya pasan por el redactor.
- **Vista 24 dentro de Historial + Conversación**, no como modal: el failover automático no espera al usuario, y un modal lo interrumpiría.

## Desviaciones del spec
| Documento y sección | Qué dice | Qué se hizo | Por qué |
|---|---|---|---|
| FLOW §4.1 «No parece repo» | Ofrecer seleccionar otra carpeta | Se muestra la ruta y cómo seguir (abrir symphony en el proyecto o `git init`) | Un selector de carpetas en la TUI es trabajo sin uso probado; abrir `symphony` en otra carpeta es igual de rápido |
| FLOW §5 System Strip | CPU, RAM, slots | Placeholder | El plan lo difiere a P08 (scheduler) |

## Bugs encontrados y corregidos de paso
- **`symphony spawn --failover none|same-provider` y `--context-mode` se ignoraban:** la CLI mandaba `none`, el enum solo acepta `NONE`, y el daemon caía en silencio a `ANY`. Ahora el daemon rechaza valores inválidos (`enum_param`) y la CLI los normaliza (`db_value`). Tests: `flags_map_to_db_values` y el enum inválido del E2E de la TUI.
- **`symphony logs --limit N` devolvía los primeros N mensajes, no los últimos.** `repo::agent_messages` ahora devuelve los últimos N en orden cronológico.

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|
| ratatui | 0.30.2 | TUI (se usa su re-export de crossterm 0.29) | Sí, y en ADR-0001 |

## Pruebas
- Comandos: `cargo nextest run -p symphony-tui`, `cargo nextest run -p symphony-daemon --test daemon`, `cargo xtask check`.
- Totales: `cargo xtask check` → 191 passed, 1 skipped (live). `cargo deny check` → ok (avisos de duplicados: `unicode-width` 0.1/0.2 por ratatui, `hashbrown`, `syn`).
- Flake visto una vez: `forced_kill_test_d_acceptance_test` (P06) falló con la suite compilando binarios en paralelo (watchdog de 1.5 s en el test). Aislado 4/4 y dos corridas completas en verde. Si reaparece: subir el umbral del watchdog **del test** con justificación, no la aserción.

## Notas para el siguiente agente
- **Snapshots:** `INSTA_UPDATE=always cargo nextest run -p symphony-tui` los regenera. Revísalos en `crates/tui/tests/snapshots/` antes de commitear.
- **El `Display` de un `serde_json::Value` ignora el ancho de `format!`** (`{:<3}` no rellena). Usa `num()`/`text()` de `ui.rs`.
- **Los ids de proveedor son `anthropic` y `openai`**, no `claude`/`codex`. Los modelos sí son `claude/sonnet` y `codex/…`.
