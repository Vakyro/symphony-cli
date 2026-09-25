# P07 · TUI y release v0.1

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p07-tui |
| Inicio / cierre | 2026-09-25 / — |
| Agentes que trabajaron | claude-code/opus-5.5 (S1–S8) |
| Tag | v0.1.0 (S8); p07-done al cerrar la fase |
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
- **«Abrir en el CLI»** (attach de ADR-0005; agregado 2026-09-25, a pedido de Leo):
  - Tecla `o` en la vista del agente, o `symphony attach <agente> [--print]`.
  - Abre una terminal nueva con la sesión del agente en el CLI oficial, interactivo y en su worktree:
    - Claude: `claude --resume <id> --model <m> --permission-mode …`.
    - Codex: `codex resume <id> -m <m> -c sandbox_mode=…`.
    - La terminal se abre con `cmd /c start` en Windows, Terminal.app (`osascript`) en macOS y `x-terminal-emulator` en Linux.
  - **Nunca con el executor vivo** (serían dos procesos sobre la misma sesión) y nunca antes de que el agente haya corrido.
  - Siempre devuelve el comando para copiarlo, por si no se puede abrir una terminal.
  - Deja una nota `SYSTEM` en la conversación.
  - **Verificación:**
    - `attach_opens_the_last_cli_session_only_when_idle` (runtime), `open_in_the_cli_reports_the_terminal_or_the_command` (TUI), tests de quoting en `crates/daemon/src/attach.rs`.
    - **Manual en Windows con Claude Code real:** un agente `claude/haiku` terminó y `symphony attach 1` abrió una consola nueva con `claude.exe --resume 2035eb18-… --model haiku`. Nota `SYSTEM` en `symphony logs 1`.
  - El regreso («al volver, Symphony retoma con `resume`», ADR-0005) llega con el pendiente de mensajes después del turno.

### P07.S6 · Providers y Recovery Center — ✅
- **Qué se hizo:**
  - Vista 22 (Providers).
  - Vista 24 (Failover Event): anterior → nuevo executor, motivo con el tipo de falla y edad del checkpoint, dentro de la pestaña Historial y con el separador en la Conversación.
  - Vista 30 (Recovery Center) con acciones `restart`, `reclaim`, `stop` y `dismiss` sobre `recovery.act`. `stop` cierra el item como `ARCHIVE`; `restart` y `reclaim` lo resuelven con la lógica del runtime (P06.S6).
- **Verificación:** snapshots `view_22_providers` y `view_30_recovery_center`; `recovery_actions_and_disconnection`. En el daemon, `recovery_center_and_tui_views_over_ipc` cubre `recovery.list` filtrado por proyecto, `restart` sin agente con error claro, acción inválida, `dismiss`, doble `dismiss`, desactivar un proveedor que sobrevive al refresh, sus modelos que pasan a no disponibles, `project.status` fuera de un repo y `agent.history` de un agente inexistente.

### P07.S7 · Snapshots y E2E — ✅
- **Hecho:**
  - 16 snapshots `insta` (una o más por vista) revisados a mano, más tests de render de la vista de agente sin executor y terminado.
  - E2E con fake-agent: abrir, inicializar, crear agente, verlo trabajar y volver al Home (`tui_drives_a_real_daemon_over_ipc_only`).
  - **Terminal real (Leo, 2026-09-25):** recorrió la TUI en su terminal. Resultado: «está de maravilla»; no reportó fallas.
  - **Journey A live con Claude Code real** (permiso de Leo, 2026-09-25): `crates/tui/tests/live.rs::live_journey_a_with_claude_code` maneja la misma lógica de la TUI (`App` + IPC) contra un daemon real con `claude/haiku`. Recorre abrir → inicializar → formulario New Agent → Model Picker (`claude/haiku`) → vista del agente → Conversación → `COMPLETED` → Actividad → Cambios (diff con `## Usage`) → Historial → Home.
    - **Resultado:** ✅ en 17 s, con 17 eventos del bus. Claude editó el README; el run quedó `EXITED · COMPLETED`.
    - Repetible con `SYMPHONY_LIVE=1 cargo nextest run -p symphony-tui --no-capture live_`.
  - La primera corrida live falló y destapó 4 bugs, corregidos en este paso (ver «Bugs encontrados»).
- **Fuera de v0.1:** el resto de Journey A (validación, merge preflight, integrar y archivar) es P08.

### P07.S8 · Release v0.1.0 — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-09-25 · con permiso de Leo para el tag y el push.
- **Qué se hizo:**
  - Versión del workspace `0.0.1` → `0.1.0` (`symphony --version` → `symphony 0.1.0`).
  - `CHANGELOG.md` con git-cliff 2.14.2, vía `npx git-cliff@latest`: binario precompilado, sin `cargo install`. `cliff.toml` agrupa por tipo de Conventional Commit en español y solo toma tags `v*`, no los `pNN-done`. Excluye bitácoras, `wip` y merges de fase. Resultado: 58 entradas en `0.1.0`.
  - Build release local para Windows: `cargo build --release -p symphony-cli -p symphony-daemon` en 6 min 28 s. `symphony.exe` 2.9 MB, `symphonyd.exe` 9.5 MB. Los instaladores llegan en P13.
  - Tag `v0.1.0` en la rama de fase y push de la rama y el tag. La CI corre en la rama; los tags no disparan workflows. El merge a `main` y `p07-done` quedan para el cierre de la fase (P07.S9, después de la replanificación P07.S10).
- **Prueba de humo de los binarios release** (home temporal):
  - `symphony status` en frío (autoarranca el daemon) en 0.33 s.
  - `providers` detecta Claude 2.1.282 y Codex 0.157.0.
  - **RAM de `symphonyd` en reposo: 16.5 MB** (presupuesto R2 < 100 MB).
- **Para regenerar el CHANGELOG:** `npx --yes git-cliff@latest --tag vX.Y.Z -o CHANGELOG.md`.

## Qué funciona (verificado)
| Funcionalidad | Cómo se verificó | Resultado |
|---|---|---|
| TUI solo por IPC contra un daemon real, con eventos del bus | `tui_drives_a_real_daemon_over_ipc_only` | ✅ |
| Las 13 vistas de P07 renderizan (01–09, 12, 13, 22, 24, 30) | 16 snapshots `insta` + 2 tests de render | ✅ |
| Ramas de FLOW §4, §6, §8.2, §16 | 11 tests de lógica en `crates/tui/tests/views.rs` | ✅ |
| Recovery Center y vistas por IPC | `recovery_center_and_tui_views_over_ipc` | ✅ |
| TUI en una terminal real de Windows | Prueba manual de Leo | ✅ |
| Journey A con Claude Code real | `live_journey_a_with_claude_code` (L3) | ✅ 17 s |

## Qué está roto o incompleto
| Problema | Impacto | Cómo reproducir | Plan / issue |
|---|---|---|---|
| Mensajes a Claude **después** de su turno | Con la adenda de ADR-0005 el proceso sale al terminar el turno; `m` devuelve «el agente no tiene un executor corriendo» | mandar un mensaje a un agente `COMPLETED` | `resume` (`claude --resume <id>`) con el mensaje: siguiente paso natural |
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
- **Los agentes de Claude nunca terminaban** (visto en la primera corrida live): el agente quedaba `RUNNING` para siempre después de que Claude respondía. Con `--input-format stream-json` el CLI espera otro mensaje y no sale. Ahora el `result` produce `TurnFinished` y el executor cierra stdin (adenda de ADR-0005). `fake-agent --stdin stream` imita a Claude para que los tests cubran este camino.
- **Claude negaba hasta los `Read` si el worktree tenía una ruta 8.3** (`C:\Users\LATITU~1\…`, lo que devuelve `%TEMP%` en esta máquina). Reproducido a mano con `claude -p`: *«the permission system is flagging the Windows path»*. Ahora el daemon arranca con la forma larga de su home (`transport::long_path`), y de ahí salen todas las rutas de worktree. El nombre del pipe también usa la forma larga: antes, la misma carpeta escrita de dos formas daba dos pipes distintos.
- **Cada proceso que lanzaba el daemon tardaba ~3 s en Windows:** `git` durante `agent.create`, que llegaba al timeout de 10 s, y `claude --version`/`codex --version` al arrancar. El daemon se lanzaba con `DETACHED_PROCESS`, que anula `CREATE_NO_WINDOW`; sin consola, Windows le creaba una a cada hijo de consola. Ahora se lanza con una consola propia oculta (`CREATE_NO_WINDOW` sin `DETACHED_PROCESS`), y `symphony spawn` con autoarranque pasó de timeout (16.8 s) a 0.6 s. Lo reveló un test de la CLI que pasaba en la mañana: el costo depende del estado de Windows (probablemente la terminal por defecto).
- **Un `agent.create` que llegaba al timeout dejaba el worktree y la rama huérfanos**, sin agente en la base. El timeout del request cortaba el future a mitad de camino. Ahora la creación corre en su propia tarea: termina (o hace su rollback) aunque el cliente deje de esperar.
- **Vista de agente:** una tarea larga empujaba fuera la línea de failover y checkpoint (ahora el encabezado no hace wrap). Un agente terminado mostraba «sin executor»; ahora muestra el último. A un agente «decidir después» le falta pista de cómo arrancarlo: ahora el Resumen dice «pulsa s».
- **El watchdog mataba como `NO_HEARTBEAT` a un CLI lento en arrancar** (era el «flake» de `forced_kill_test_d_acceptance_test` y `hung_executor_…` con la suite cargada). Un run sin actividad registrada contaba como silencioso desde el primer tick (`is_none_or`): 100 ms en los tests, 5 s en producción, y Claude por el shim de npm puede tardar más en su primera línea. Ahora el run marca actividad al lanzarse. Test de regresión: `slow_starting_executor_is_not_taken_for_hung` (con `startup_delay_ms` en el guion del fake-agent), que falla sin la corrección (`NO_HEARTBEAT`) y pasa con ella. Los umbrales de los tests no cambiaron.

## Dependencias agregadas
| Crate | Versión | Para qué | ¿Estaba en STACK §58? |
|---|---|---|---|
| ratatui | 0.30.2 | TUI (se usa su re-export de crossterm 0.29) | Sí, y en ADR-0001 |

## Pruebas
- Comandos: `cargo nextest run -p symphony-tui`, `cargo nextest run -p symphony-daemon --test daemon`, `cargo xtask check`.
- Totales: `cargo xtask check` → 199 passed, 1 skipped (live de la CLI; el live de la TUI pasa como omitido sin `SYMPHONY_LIVE`). Dos corridas completas seguidas en verde. `cargo deny check` → ok (avisos de duplicados: `unicode-width` 0.1/0.2 por ratatui, `hashbrown`, `syn`).
- Live L3: `live_journey_a_with_claude_code` ✅ (17 s, 17 eventos).

## Notas para el siguiente agente
- **Snapshots:** `INSTA_UPDATE=always cargo nextest run -p symphony-tui` los regenera. Revísalos en `crates/tui/tests/snapshots/` antes de commitear.
- **El `Display` de un `serde_json::Value` ignora el ancho de `format!`** (`{:<3}` no rellena). Usa `num()`/`text()` de `ui.rs`.
- **Los ids de proveedor son `anthropic` y `openai`**, no `claude`/`codex`. Los modelos sí son `claude/sonnet` y `codex/…`.
