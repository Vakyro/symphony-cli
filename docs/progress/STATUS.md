# STATUS — Symphony CLI

**Actualizado:** 2026-10-12 · por claude-code/sonnet-5.5
**Fase actual:** P14 · Sugerencias de modelo (rama `phase/p14-sugerencias`, por crear desde `main`). P12 cerrada (`p12-done`)
**Paso actual:** P14.S1 · Migración 007 (`model_suggestions`)
**Estado del paso:** sin empezar
**En curso por:** nadie

**Plan hasta v1.0 (ADR-0014, aceptado por delegación de Leo):** orden **P11.5 → P12 → P14 → P15 → P13 → P16**. El agente corre de forma continua: al cerrar un paso sigue con el siguiente y al cerrar una fase (PLAN §4.6) sigue con la siguiente del orden. Lo que necesita a Leo (⛔) se anota abajo en «Para Leo» y se sigue con lo siguiente que no dependa de eso. **Nunca esperar.**

## Salud del repo
- `cargo xtask check`: ✅ al cierre de P11.5 (588 tests; el gate quita `SYMPHONY_LIVE`, nunca corre live).
- `cargo deny check`: ✅ (solo avisos de duplicados).
- CI en `main`: ✅ (ubuntu, windows, macos, msrv, deny).
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

**P14 · Sugerencias de modelo** (`phase/p14-sugerencias`): S1 migración **007** · S2 sugerencias por reglas · S3 desempeño aprendido (umbral en ADR) · S4 experimentos ML (ADR; rechazar es válido) · S5 cierre (`p14-done`)

**P15 · GUI de escritorio** (`phase/p15-gui`): S1 puente · S2 scaffold · S3 vistas · S4 diseño y QA · S5 bundles en CI · S6 cierre (`p15-done`)

**P13 · Endurecimiento y pre-release** (`phase/p13-endurecimiento`): S1 seguridad · S2 fuzzing · S3 recuperación y 4 h · S4 rendimiento y retención · S5 calidad del repo · S6 mdBook (+ guía SSH) · S7 pipeline de release en dry run · S8 cierre (`p13-done`; `v0.9.0` local)

**P16 · Validación final** (`phase/p16-validacion`): S1 checklist · S2 E2E (fake-agents) · S3 benchmark · S4 instalación limpia en CI · S5 docs as-built · S6 release ⛔ · S7 retrospectiva

## Para usar lo nuevo (Leo)
`symphony daemon stop` y luego `cargo install --path crates/cli --locked --force` y `cargo install --path crates/daemon --locked --force` (no lo hice yo: puede haber un daemon tuyo en uso). Novedades: `symphony chat merge`, `symphony review`, `[providers.permissions]`, texto del chat en vivo con Claude, `Ctrl+G`/`Ctrl+R` en el chat.

Novedades de P12: `symphony plugin list|check`, `[plugins]` en `project.toml` y el ejemplo `examples/plugins/validator-todo-check` (QUICKSTART § Plugins).

## Permisos vigentes (sin preguntar)
- Merge `--no-ff` de la rama de fase a `main`, tag `pNN-done`, push de `main` y del tag (memoria «merge a main OK»).
- Live (`SYMPHONY_LIVE=1`) **solo con modelos gratuitos de OpenCode** (`opencode/…-free`, `opencode/big-pickle`), 2026-10-10.
- Requisito permanente (CONSTRAINTS A12): instalación única y global; nada se instala ni se escribe por proyecto.

## Para Leo (⛔ — el agente anota aquí y sigue)
- **Basura que dejé yo:** por un typo de ruta se creó el archivo `C:\LATITU~1\x` (y su carpeta `C:\LATITU~1`) en la raíz del disco. No pude borrarlo: la comprobación de seguridad bloqueó el `rmdir` de una carpeta en la raíz. Es seguro quitarlo con `rmdir /s /q C:\LATITU~1`.
Formato: paso · qué hace falta · comando exacto si aplica.
- **Siempre ⛔:** live que gaste cuota (Claude, Codex, Kimi, Antigravity, Copilot); push de ramas que no sean `main`; publicar releases o pushear tags `v*`; cambiar defaults de permisos de proveedores; ToS.
- **2026-10-12 · cuota gastada sin permiso (error del agente):** la sesión arrancó con `SYMPHONY_LIVE=1` en el entorno y el `cargo xtask check` de baseline de P11.5 corrió los 6 tests live con CLIs reales (`live_benchmark`, `live_handoff`, `live_cost`, `live_resume`, `live_sessions`, `live` de la TUI; ~9 min, 557 pasaron). **Falló `symphony-tui::live live_journey_a_with_claude_code`** (47 s): no se investigó; no es un cambio de P11.5, pero conviene que Leo lo revise (¿Claude Code, auth o cambio de CLI?). Corrección: `unset SYMPHONY_LIVE` antes de todo comando de cargo. Leo: revisa tu terminal de lanzamiento (`echo $SYMPHONY_LIVE`) para que no vuelva a heredarse.
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
| P14, P15, P13, P16 | ⏳ en ese orden (ADR-0014) | |
