# STATUS — Symphony CLI

**Actualizado:** 2026-10-24 · por claude-code/opus-5.5
**Fase actual:** P13 · Endurecimiento y pre-release (rama `phase/p13-endurecimiento`, bitácora `docs/phases/P13-endurecimiento.md`)
**Paso actual:** S1 ✅ · S2 🟡 (targets y proptest; falta correr con nightly) · S3 🟡 (soak corto; falta el de 4 h) · S4 disco 🟡 · S5 🟡 (typos/lychee en CI) · S6 🟡 (libro, ER, QUICKSTART) · S7 ⏳ · S8 ⏳. Delegado con Symphony: `docs/progress/DOGFOOD.md`
**Estado del paso:** pausado al cierre de la jornada (2026-10-24); rama pusheada
**En curso por:** nadie
**Siguiente:** watchdog que cuente la CPU del árbol como latido (DOGFOOD), luego S2 con nightly (WSL o CI), S3 de 4 h, S7 release en dry run

**Plan hasta v1.0 (ADR-0014, aceptado por delegación de Leo):** orden **P11.5 → P12 → P14 → P15 → P15.5 → P13 → P16** (P15.5 añadida el 2026-10-23). El agente corre de forma continua: al cerrar un paso sigue con el siguiente y al cerrar una fase (PLAN §4.6) sigue con la siguiente del orden. Lo que necesita a Leo (⛔) se anota abajo en «Para Leo» y se sigue con lo siguiente que no dependa de eso. **Nunca esperar.**

## Salud del repo
- `cargo xtask check`: ✅ 666 tests (2026-10-24, rama de P13; el gate quita `SYMPHONY_LIVE`, nunca corre live).
- `cargo deny check`: ✅ en la rama `phase/p13-endurecimiento` (tokio 1.54.1 por RUSTSEC-2026-0386); en `main` el job `deny` sigue en rojo hasta que esa rama se integre.
- CI en `main`: 🔴 solo el job `deny` (aviso nuevo contra tokio 1.53.1, publicado después de P14); los demás ✅. Arreglo ya hecho en la rama de P13 (P13.S1), pendiente de integrar.
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

**P13 · Endurecimiento y pre-release** (`phase/p13-endurecimiento`): S1 seguridad · S2 fuzzing (◐ targets `fuzz/` + proptest hechos; falta correr 30 min nightly y corpus en CI) · S3 recuperación y 4 h · S4 rendimiento y retención · S5 calidad del repo · S6 mdBook (+ guía SSH) · S7 pipeline de release en dry run · S8 cierre (`p13-done`; `v0.9.0` local)

**P16 · Validación final** (`phase/p16-validacion`): S1 checklist · S2 E2E (fake-agents) · S3 benchmark · S4 instalación limpia en CI · S5 docs as-built · S6 release ⛔ · S7 retrospectiva

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
