# STATUS — Symphony CLI

**Actualizado:** 2026-10-29 · por codex/gpt-6.1
**Fase actual:** P17 · TUI al día (rama `phase/p17-tui`). P13 **cerrada y mergeada** (`p13-done`); P16 en `phase/p16-validacion`, rebasada sobre `main` y a la espera de Leo (veredicto de uso) y de P17–P20 para los docs as-built.
**Paso actual:** P17.S5 · pestaña TUI de procesos terminada. Queda integrar P17.S6 con sus vistas y cerrar S7. Plan completo de lo pendiente con las decisiones de Leo del 2026-10-29 en **`docs/progress/ROADMAP-2026-10-29.md`**. P16: S0–S1 ✅, S2/S3 medidos, S3 con guard de disco ✅, S4 esperando estar en `main`.
**Estado del paso:** terminado y verificado en este worktree; pendiente de integración por el orquestador.
**En curso por:** codex/gpt-5.x.
**Siguiente:** integrar IPC global y ramas con vistas TUI de P17.S6; continuar S5 y cerrar S7 (merge y tag `p17-done`) → P18 → P19 → P20 → P16 (v0.9.0 → v1.0.0; tags `v*` solo con OK escrito de Leo).

**Handoff 2026-10-29 · P17.S5 (cerrado):** integrado el trabajo del agente 065 (cherry-pick) en el worktree `agent-068`; corregidos clippy (`let-else` → `?` en `recorder.rs`) y el contract suite del fake_adapter (`ToolOutput`). Verdes: clippy `-D warnings` de protocol/daemon/testkit/adapter-common; `cargo nextest run` de los cuatro crates (excluido el binario `daemon`, flaky en Windows) y el L2 `shell_tool_output_is_listed_and_streamed_for_its_agent` por filtro de nombre. Aviso: al correr tests dentro del harness hay que quitar `SYMPHONY_USER_HOME` del ambiente (si no, los tests de plugins fallan; ver `docs/progress/LEARNINGS.md`). Quedan como seguimiento: test de socket IPC end-to-end con fake-agent como proveedor configurado, y revisar parsers de proveedores para emitir `ToolOutput` fuera de hooks. Decisión registrada en `docs/phases/P17-tui.md`. Siguiente: P17.S6.

**Hotfix del 2026-10-28 (después del merge de P13):** el CI de `main` quedó en rojo en `check (ubuntu/macos/windows)` y `coverage` por el mismo test, `agent_create_rejects_an_invalid_depends_on`: sin `model`/`profile` el daemon usa `DecideLater` y comprueba primero que haya proveedores, así que en un entorno sin CLIs de proveedor devolvía `no_eligible_provider` donde el test esperaba `not_a_repo`. El test ya acepta los dos fallos posteriores a la validación de parámetros (`9af3668`, gate 733 ✅, pusheado).

**Cierre de la jornada (detalle en `docs/progress/CIERRE-2026-10-27.md`):** qué salió bien, qué falló, qué se arregló y qué falta. Resumen: integrados #48 (guardia de aislamiento), #49 (freno por disco), #50 (permisos, `depends_on`, `usage.get`). **2026-10-28:** integrados #51 y #52, arreglado el test inestable del watcher y los `LEAK` del CLI, verificado ADR-0021, gate y `deny` en verde, P13 mergeada a `main`.

**Lo que falta para v1.0 (2026-10-28):** (1) Rebasar `phase/p16-validacion` sobre `main`. (2) P16.S2 E2E con fake-agents, S3 benchmark, S4 `install-check.yml`, S5 docs as-built, S7 retrospectiva. (3) ⛔ S6 release v1.0.0 y v0.9.0 (tu OK para pushear tags `v*`). Antes de v1.0.0, P17–P20 (decidido por Leo; ver abajo).

**Plan hasta v1.0 (ADR-0014, aceptado por delegación de Leo):** orden **P11.5 → P12 → P14 → P15 → P15.5 → P13 → P16** (P15.5 añadida el 2026-10-23). El agente corre de forma continua: al cerrar un paso sigue con el siguiente y al cerrar una fase (PLAN §4.6) sigue con la siguiente del orden. Lo que necesita a Leo (⛔) se anota abajo en «Para Leo» y se sigue con lo siguiente que no dependa de eso. **Nunca esperar.**

## Salud del repo
- `cargo xtask check`: ✅ **733 tests, 0 fallos, 0 leaky** (2026-10-28, `main` con el merge de P13 y el hotfix del test de `depends_on`).
- `cargo deny check`: ✅ `advisories, bans, licenses, sources ok` (rama de P13 y `main`).
- CI en `main`: 🟡 el run del merge de P13 salió en rojo por un test dependiente de la máquina; el run del hotfix va en verde en `docs`, `deny`, `coverage`, `msrv`, `fuzz` y `gui`, con los tres `check` en curso al escribir esto.
- Tests conocidos en rojo: ninguno.

## Cola de trabajo (en orden)
Detalle de cada paso (qué y **Verifica**) en el PLAN. Bitácora de la fase en `docs/phases/`.

**P11.5 · Pendientes y pulido** (`phase/p115-pendientes`, bitácora `docs/phases/P11.5-pendientes.md`)
- [x] S1 `ctx://` con `@` y caracteres reservados en la ruta
- [x] S2 Permisos de proveedores en `config.toml` (defaults iguales)
- [x] S3 Respuesta construyéndose en el chat (deltas de Claude)
- [x] S4 Router: `needed_context` y aviso de checkpoint obsoleto
- [x] S5 Lote: el tope de simultáneos sigue a la cuota
- [x] S6 Merge chat → `main` con review agent (ADR-0006)
- [x] S7 Pulido de la TUI (milestones, «Enviar al agente», avisos que caducan)
- [x] S8 Medir `resume` y `/compact` con OpenCode gratuito
- [x] S9 Live (S9.2 inconcluso: 429 del plan gratuito) gratuito con OpenCode (broker MCP, handoff, plan por fases, review)
- [x] S10 Deuda anotada
- [x] S11 Cierre (`p115-done`)

**P12 · Plugins** (`phase/p12-plugins`, `docs/phases/P12-plugins.md`): ✅ S1 · ✅ S2 · ✅ S3 · ✅ S4 cierre (`p12-done`)

**P14 · Sugerencias de modelo** (`phase/p14-sugerencias`, `docs/phases/P14-sugerencias.md`): ✅ S1 · ✅ S2 · ✅ S3 · ✅ S4 (ML rechazado, ADR-0017) · ✅ S5 cierre (`p14-done`)

**P15 · GUI de escritorio** (`phase/p15-gui`, `docs/phases/P15-gui.md`): ✅ S1 · ✅ S2 · ✅ S3 · 🟡 S4 (falta revisión visual) · ✅ S5 · ✅ S6 cierre (`p15-done`)

**P15.5 · Cambios de rumbo** (`phase/p155-cambios-de-rumbo`, `docs/phases/P15.5-cambios-de-rumbo.md`; pedido de Leo 2026-10-23, va antes de P13): ✅ S0 ADRs 0018–0020 (aceptados 2026-10-23) · ✅ S1 modelos de las 6 suscripciones (`docs/research/modelos-2026-10.md`) · ✅ S2 Codex/Copilot/agy sin bloqueos de sandbox · ✅ S3 estado y daemon por proyecto (ADR-0018; `symphony migrate split`) · ✅ S4 harness de Symphony (ADR-0019; `[harness] mode`, 5 de 6 proveedores) · ✅ S5 más allá del código (ADR-0020; `symphony init --kind`, perfiles `@write/@media/@research`, migración 008) · ✅ S6 cierre (`p155-done`)

**P13 · Endurecimiento y pre-release** (`phase/p13-endurecimiento`): ✅ S1 seguridad · ✅ S2 fuzzing (30 min por target en WSL, 2 crashes de `redact` arreglados, corpus y job corto en CI) · ✅ S3 checkpoint corrupto, writer a media transacción y soak de 4 h (963 tandas, 470 reinicios, RSS 25 → 27 MB, 2026-10-27) · ✅ S4 retención y harness E2E · ✅ S5 auditorías de 10 áreas con sus arreglos (incluidos #51 y #52 de `server.md`), ponytail-audit, typos/lychee/mdbook bloqueantes, cobertura · ✅ S6 mdBook · ✅ S7 cargo-dist en dry run · ✅ S8 cierre (`p13-done`, 2026-10-28: gate 733 ✅, `deny` ✅, merge a `main`; `v0.9.0` local)

**P17 · TUI al día** (`phase/p17-tui`, `docs/phases/P17-tui.md`): ✅ S1 tema y barra de estado · ✅ S2 chat estilo Claude Code · ✅ S3 arranque (arte, «para empezar», atajos con `?`, recuadro de versión local) · ✅ S4 mascota · ✅ S6 IPC daemon (uso global y ramas) · pendientes S5, vistas TUI de S6 y S7 · backlog documentado: S8 renderizado rico del chat (colores por importancia) y política configurable de Symphony/harness (fuera de P17, nueva fase).

**P16 · adelantado** (`phase/p16-validacion`, sale de P13; rebasada sobre `main` tras el merge): ✅ S0 check `files` (ADR-0021) · ✅ S1 checklist · ✅ S3 benchmark medido con guard de disco · 🟡 S4 `install-check.yml` (espera estar en `main`; el trigger ya no se dispara desde `main`) · 🟡 S5 anexo de desviaciones

**P16 · Validación final** (`phase/p16-validacion`): S1 checklist · S2 E2E (fake-agents) · S3 benchmark · S4 instalación limpia en CI · S5 docs as-built · S6 release ⛔ · S7 retrospectiva

**Antes de publicar v1.0.0 (decidido por Leo, 2026-10-27; P16.S6 queda después de P20; detalle en `docs/research/backlog-2026-10-27.md` y PLAN § P17–P20):** P17 TUI al día (tema morado apagado, barra de estado con usos/modelo/rama/modo, streaming y diffs, arranque con arte ASCII, mascota, shell en curso, vistas de plan/ramas/status global con heatmap) → P18 contexto y ahorro de tokens (auditar router, flintstone por defecto, sesiones cortas, graphify al iniciar, `/init` de Symphony, segundo cerebro) → P19 orquestación automática (Jev, perfiles `@best-*`, planeador, delegación con effort, `/orchester`, chat principal con paridad Claude Code) → P20 editor integrado, grafo, vista Habbo y móvil. Ideas futuras: cualquier CLI de agente y API keys de cualquier proveedor.

## Para usar lo nuevo (Leo)
`symphony daemon stop` y luego `cargo install --path crates/cli --locked --force` y `cargo install --path crates/daemon --locked --force` (no lo hice yo: puede haber un daemon tuyo en uso). Novedades: `symphony chat merge`, `symphony review`, `[providers.permissions]`, texto del chat en vivo con Claude, `Ctrl+G`/`Ctrl+R` en el chat.

Novedades de P14: `symphony suggestion list|switch|keep|mute`, banner de sugerencia de modelo en el chat (`Ctrl+T/K/N`) y en la vista del agente (`S/K/N`); el router aprende de los runs (ADR-0016).

Novedades de P12: `symphony plugin list|check`, `[plugins]` en `project.toml` y el ejemplo `examples/plugins/validator-todo-check` (QUICKSTART § Plugins).

## Permisos vigentes (sin preguntar)
- Merge `--no-ff` de la rama de fase a `main`, tag `pNN-done`, push de `main` y del tag (memoria «merge a main OK»).
- Live (`SYMPHONY_LIVE=1`) **solo con modelos gratuitos de OpenCode** (`opencode/…-free`, `opencode/big-pickle`), 2026-10-10.
- Live con los modelos de pago de las 6 suscripciones (Claude Code, Codex, Antigravity, Kimi Code, OpenCode, Copilot), **tope de 2 mensajes por corrida**, 2026-10-23.
- Acceso local total por defecto para todos los proveedores (P15.5.S2), 2026-10-23.
- Requisito permanente (CONSTRAINTS A12): instalación única y global; nada se instala ni se escribe por proyecto.

## Para Leo (⛔ — el agente anota aquí y sigue)
- **2026-10-29 · Las 53 preguntas de producto: respondidas por Leo.** Todas las decisiones quedan en docs/progress/ROADMAP-2026-10-29.md (orden P17->P18->P19->P20->P16, merge a main sin OK, tags pNN-done pusheados, mascota = mini piano, status global, 40 % de reduccion de tokens, Flintstone por defecto, 3 tareas por sesion con /compact automatico, Laya como backend de decision con personalizacion y umbrales 0.9/0.6, perfiles elegidos por el modelo de decision, /orchester con plan aprobado, editor con guardado y nano tal cual, Habbo fuera del plan). Tags v* solo con OK escrito; homebrew autorizado (Vakyro/homebrew-symphony).
- **2026-10-28 · P16.S4 no puede correr hasta que el workflow esté en `main`.** GitHub devuelve 404 al lanzar `install-check.yml` desde una rama de fase (`gh workflow run ... --ref phase/p16-validacion`): un workflow que no existe en la rama por defecto no se puede despachar. Está en `phase/p16-validacion` (ya pusheada) y se lanzará con `run_id` de un CI que sí tenga los artefactos `dist-*`. De paso se corrigió que el `workflow_run` se dispararía también en `main`, donde `release-dry-run` no corre y no hay artefactos: el primer CI verde de `main` habría salido en rojo.
- **2026-10-28 · P20.S4 (móvil) ya está cubierto.** La guía `docs/book/src/uso-remoto-ssh.md` (P13.S6) describe Tailscale + SSH + tmux y el trabajo por comandos en pantallas pequeñas, que es exactamente lo que pide P20.S4; no hace falta código nuevo. Solo queda releerla cuando se cierre P20.
- **2026-10-28 · Cuota de opencode-go agotada.** El run del primer delegado de P17 devolvió `provider.quota: Go usage limit exceeded status=429`; el proveedor queda `EXHAUSTED` y el reinicio estimado fue 23:28 local. Mientras tanto se delegó a los modelos **free** de opencode-go (`beam-free`), que no gastan la cuota Go y funcionan. Dos cosas que conviene decidir: (1) los modelos free son más flojos, así que el trabajo se revisa más; (2) Symphony bloquea **todo** el proveedor cuando la cuota Go se agota, aunque los modelos free tengan su propia cuota — si quieres, se afina para que la salud sea por modelo en vez de por proveedor.
- **2026-10-28 · `WAITING_PROVIDER` no se reanuda solo.** Cuando un agente se queda sin proveedor, Symphony abre un ítem de recuperación y espera; el estado `EXHAUSTED` sin hora de reinicio pasa a `PROBING` a los 30 min (`EXHAUSTED_PROBE_MS`), pero el agente no vuelve a intentarlo: hay que `symphony resume` o `switch`. `providers refresh` no limpia la salud. Si prefieres que se reintente solo, es un cambio pequeño en el scheduler.
- **2026-10-27 · Backlog nuevo (P17–P20): respondido por Leo.** P17–P20 van antes de v1.0.0. Jev no se usa: se evaluará una versión ligera de código abierto que Leo describirá después. Móvil por Tailscale + SSH + tmux, sin app ni web (P20.S4).
- **2026-10-29 · P19.S1: investigación documentada.** Se añadió a PLAN.md (S1 de P19) el detalle de Jev (TypeSafe AI, System One, clasificador con Choice/Score/Noul, probabilidades calibradas) y 7 alternativas open-source (Laya, Kev, Von, NanoJev, Nimble, SemIf, Rizzo Flow) con licencia, tamaño, runtime y latencia. Pendiente de Leo: (1) elegir el backend por defecto, (2) el umbral de confianza para la cascada (decisión barata → código determinista → modelo de frontera), (3) confirmar el requisito de **personalización**: el usuario puede usar el modelo que tenga instalado en su máquina (Jev, una alternativa open-source o uno propio), no necesariamente Jev ni una versión open-source fija. Symphony solo define el contrato `estado + preguntas tipadas → decisiones tipadas + probabilidades` y un adaptador por backend. ADR pendiente.
- **2026-10-29 · Ideas futuras de Leo documentadas.** Se registraron en PLAN.md (Ideas futuras) y en la bitacora de P17 cuatro ideas: (1) version cooperativa en linea de Symphony (invitar companeros/amigos, cada uno suma proveedores/cuentas/modelos/tokens; requiere ADR de transporte, auth, aislamiento y cuotas), (2) context compaction con Jev o equivalente (preservando mas contenido importante: decisiones, constraints, archivos tocados), (3) documentar cuando usar git merge vs git rebase, (4) ingenieria de grafos en los agentes (linea de produccion con nodos/etapas y aristas/dependencias, reintentos, paralelismo, checkpoint por nodo). Ninguna implementada; quedan como backlog.
- **2026-10-29 · Idea futura de Leo (adicional):** Symphony con su propia linea de agentes basados en LLMs ya existentes - un agente para codigo, uno para tareas sencillas, uno para investigacion, etc. - cada uno con system prompt, herramientas y perfil optimizados para su rol. Requiere ADR de roles, prompts y seleccion automatica por tipo de tarea; complementa los perfiles @best-coding/@best-reasoning de P19.S2 y el planeador P19.S3. Sin codigo por ahora.
- **2026-10-28 · #51, #52 y ADR-0021: revisados e integrados** (Leo autorizó hacerlo en vez de dejarlo en documento). #51 (`5e46112`, `fe4ec5d`) y #52 (`1840654`) están en `main`. ADR-0021 coincide con su implementación; única desviación menor y no bloqueante: sin archivos que revisar el check devuelve `PASS` con resumen, no `SKIPPED` como decía el ADR. Si prefieres `SKIPPED`, es un cambio de dos líneas en `files_check.rs`.
- **2026-10-27 · Noche de dogfooding:** reporte completo en `docs/progress/REPORTE-2026-10-27.md`.
- **Publicar v0.9.0 / v1.0.0:** el pipeline de cargo-dist está listo en dry run; pushear un tag `v*` publica (y para Homebrew hace falta crear el repo `Vakyro/homebrew-symphony`).
- **Aislamiento:** dos modelos escribieron en el repo principal (DOGFOOD § Hallazgos). Antes de delegar sin supervisión hace falta una guardia en Symphony; mientras tanto, no dejar cambios sin commitear en el repo principal con agentes corriendo.
- **Agente #39 (arreglos de `server.md`)** dejó 1154 líneas sin commitear en su worktree; no se integró. Lo que faltaba de `server.md` se cubrió con #51 y #52 (integrados el 2026-10-28); lo que #39 traía de más ya no aplica.
- **Watchdog:** la CPU ya no mantiene vivo un run que lleva 60 min sin escribir nada (tope fijo). Una corrida de Antigravity de más de una hora sin salida caería: si te pasa, se hace configurable.
- **Disco:** `cargo clean` en el repo si vuelve a crecer (`target/` llegó a 104 GB); WSL creció por los clones del fuzzing (`wsl --shutdown` + compactar el vhdx si molesta).
- **Basura en `~` de WSL:** clones `~/symphony-fuzz-*` y `~/symphony-linux-*` (se pueden borrar).
- **2026-10-24 · Dogfooding:** la copia estable de Symphony con la que se delega está en `~/.symphony-stable/bin` (commit en `VERSION`); el estado de los agentes de este repo, en `<repo>/.symphony/`. Detalle y hallazgos en `docs/progress/DOGFOOD.md`.
- **2026-10-24 · Disco:** tu `~/.symphony` vieja (97 MB de base y unos 60 worktrees con `node_modules`/`target/`) se limpia sola cuando corras `symphony migrate split` en cada proyecto y abras su daemon (los worktrees de agentes terminados se borran al arrancar). Lo que no se migre, bórralo a mano.
- **Basura que dejé yo:** por un typo de ruta se creó el archivo `C:\LATITU~1\x` (y su carpeta `C:\LATITU~1`) en la raíz del disco. No pude borrarlo: la comprobación de seguridad bloqueó el `rmdir` de una carpeta en la raíz. Es seguro quitarlo con `rmdir /s /q C:\LATITU~1`.
Formato: paso · qué hace falta · comando exacto si aplica.
- **Siempre ⛔:** live que gaste cuota (Claude, Codex, Kimi, Antigravity, Copilot); push de ramas que no sean `main`; publicar releases o pushear tags `v*`; cambiar defaults de permisos de proveedores; ToS.
- **2026-10-12 · cuota gastada sin permiso (error del agente):** la sesión arrancó con `SYMPHONY_LIVE=1` en el entorno y el `cargo xtask check` de baseline de P11.5 corrió los 6 tests live con CLIs reales (`live_benchmark`, `live_handoff`, `live_cost`, `live_resume`, `live_sessions`, `live` de la TUI; ~9 min, 557 pasaron). **Falló `symphony-tui::live live_journey_a_with_claude_code`** (47 s): no se investigó; no es un cambio de P11.5, pero conviene que Leo lo revise (¿Claude Code, auth o cambio de CLI?). Corrección: `unset SYMPHONY_LIVE` antes de todo comando de cargo. Leo: revisa tu terminal de lanzamiento (`echo $SYMPHONY_LIVE`) para que no vuelva a heredarse.
- P15.5.S5 · Para usar lo nuevo: reinstalar y, en una carpeta de documentos, `symphony init --kind document --git-init`. Los profiles `@write/@media/@research` arrancan con puntajes derivados (editables en `profile_models`); dime si algún modelo te rinde distinto y los ajusto. Quedó fuera (ADR-0020 § Enmienda): check de formato propio, tipo por tarea y binarios grandes con puntero.
- P15.5.S4 · El harness de Symphony es el defecto desde 2026-10-24 (tu decisión). Antigravity no lo admite y sigue con sus herramientas. Pendiente: que `shell` pase por el daemon para la cola de comandos pesados (P08). Para volver a lo de cada CLI: `[harness] mode = "native"`.
- P15.5.S3 · Para usar el estado por proyecto: `symphony daemon stop`, reinstalar (`cargo install --path crates/cli --locked --force` y `crates/daemon`) y, en cada proyecto con historia, `symphony migrate split` una vez (apaga tu daemon global y reparte `~/.symphony/symphony.db`; no borra nada). Los worktrees viejos siguen en `~/.symphony/worktrees` y funcionan; los nuevos nacen en `<proyecto>/.symphony/worktrees`. Pendiente aparte: `symphony daemon stop --all` y la GUI con selector de proyecto probado en una ventana real.
- P15.5.S2 · Para aplicar los permisos nuevos: `symphony daemon stop` y reinstalar. Si tu `~/.symphony/config.toml` tiene valores explícitos en `[providers.permissions]`, mandan sobre los nuevos defaults. Verificado en vivo solo con Codex (pnpm, red y lectura de `~/.symphony` OK con `danger-full-access`); Claude, Copilot y Antigravity cambian por config sin prueba live.
- P15.5.S1 · Copilot solo ofrece `auto` (Leo, 2026-10-24): nada que hacer. Antigravity: habilitados sus modelos Claude y GPT-OSS, con el proveedor en el nombre.
- P15.S4 · Revisión visual de la GUI en una ventana real (`cd apps/gui && pnpm tauri dev`, con `symphony`/`symphonyd` instalados) y con DevTools del WebView; no pude hacerla (extensión de Chrome desconectada). Los bundles de macOS/Linux solo se construyen en CI.
- P08 · Live de plan por fases y bloque ```review con Claude/Codex (si S9 con OpenCode no basta).
- P11.5.S3 · Capturar un fixture real del stream parcial de Claude (`--include-partial-messages`) y ver el texto avanzar en una terminal real (gasta cuota: ⛔).
- P11.5.S9 · Un modelo gratuito de OpenCode escribió `n1.txt`…`n8.txt` en la raíz del repo de Symphony durante una corrida live (causa sin explicar; ver bitácora P11.5 § Incidentes). Si vuelve a pasar con otro modelo, es un riesgo de aislamiento (CONSTRAINTS A4).
- P08 · Revisión de fluidez de las vistas 15–19 y 26–28 en una terminal real.
- P09 · Que Claude y Codex llamen a `context_search` en vivo y que Codex `exec` apruebe herramientas MCP sin preguntar.
- P11 · Re-validación de Kimi, Antigravity y Copilot (criterios en `docs/research/opencode-y-calidad-de-proveedores.md` § 4); ToS de los tres y de usar la credencial de Copilot dentro de OpenCode; skills de los tres; sondear los modelos habilitados de Copilot; Kimi sin sandbox.
- P11 · Los 17 pares live de handoff restantes (n = 1 hoy).
- ADR-0013 D2 · Medir `/compact` con Claude/Codex: con OpenCode no existe en modo no interactivo (P11.5.S8), así que solo queda Claude Code (`claude -p "/compact"`) y Codex.
- P11.5.S9.2 · Handoff entre modelos a media tarea en los cuatro modos de contexto con proveedores reales (Claude/Codex): con OpenCode gratuito dio 429 en 3 corridas. Comando: `SYMPHONY_LIVE=1 cargo nextest run -p symphony-cli --test live_handoff` (ya existe para otros pares).

## Handoff para el siguiente agente
- **Antes de tocar nada:** `git status` y `git log --oneline 441dfd6..HEAD` en `phase/p13-endurecimiento`. Hay 8 commits locales sin pushear desde la noche anterior.
- **Agentes vivos:** #51 y #52 corren en Kimi y escriben en sus worktrees (`.symphony/worktrees/…/agent-05N`). No integrar a ciegas: revisar `git log` y el diff de cada uno.
- **Integrar con cherry-pick** (no con merge del worktree): el worktree de #51 salió de una base anterior a #50.
- **Gate:** `unset SYMPHONY_LIVE` y luego `cargo xtask check`. Con varios cargo en paralelo usar `CARGO_TARGET_DIR` aparte (si no, `LNK1104` al enlazar `soak.exe`).
- **Disco:** C: al 97 %. `target/` ~66 GB. `cargo clean` cuando no haya agentes compilando.
- **Test inestable:** `project_files_stay_searchable_as_they_change` (daemon). Pasa solo, falla en corridas completas.
- Empieza por PLAN § P11.5 y ADR-0014. Corre `cargo xtask check` en `main` antes de crear la rama.
- La TUI está en `crates/tui`: `app.rs` estado y lógica pura, `ui.rs` render, `io.rs` IPC, `lib.rs` loop. Snapshots: `INSTA_UPDATE=always cargo nextest run -p symphony-tui` y revisar el diff.
- Si un test deja un `symphonyd.exe` huérfano, el build falla con «Acceso denegado»: mátalo antes.
- Reinstalar para que Leo use lo nuevo: `symphony daemon stop` y los dos `cargo install --path … --locked --force` (QUICKSTART §1).
- Mapa del código: `graphify-smart-out/GRAPH_REPORT.md` (rebuild con `python graphify-smart-out/build.py <trigger>`).

## Bloqueos
- (ninguno)

## Fases
| Fase | Estado | Tag |
|---|---|---|
| P00–P07 | ✅ | p00-done … p07-done, v0.1.0 |
| P07.5 | ✅ | p075-done |
| P11 | ✅ | p11-done |
| P10 | ✅ | p10-done, v0.5.0 |
| P09 | ✅ (live pendientes → P11.5.S9) | p09-done |
| P08 | ✅ | p08-done |
| P11.5 | ✅ (live de pago pendientes → Para Leo) | p115-done |
| P12 | ✅ | p12-done |
| P14 | ✅ | p14-done |
| P15 | ✅ (revisión visual → Para Leo) | p15-done |
| P15.5 | ✅ (ADR-0018, 0019, 0020) | p155-done |
| P13, P16 | ⏳ en ese orden (ADR-0014) | |

## Ola 0 — decisiones del 2026-10-29

- P17.S4 queda definido como un mini piano de una octava; no incluye fuegos artificiales.
- P20.S3, la vista Habbo, sale del plan y queda registrada como idea futura.
- P19 usa Laya por defecto, con backend enchufable; Symphony no trae Python.
- Antigravity está sin cuota durante aproximadamente 48 horas desde el 2026-10-29.
