# STATUS — Symphony CLI

**Actualizado:** 2026-10-12 · por claude-code/opus-5.5
**Fase actual:** P11.5 · Pendientes y pulido (rama `phase/p115-pendientes`, por crear desde `main`)
**Paso actual:** P11.5.S1 · `ctx://` con `@` y caracteres reservados en la ruta
**Estado del paso:** sin empezar
**En curso por:** nadie

**Plan hasta v1.0 (ADR-0014, aceptado por delegación de Leo):** orden **P11.5 → P12 → P14 → P15 → P13 → P16**. El agente corre de forma continua: al cerrar un paso sigue con el siguiente y al cerrar una fase (PLAN §4.6) sigue con la siguiente del orden. Lo que necesita a Leo (⛔) se anota abajo en «Para Leo» y se sigue con lo siguiente que no dependa de eso. **Nunca esperar.**

## Salud del repo
- `cargo xtask check`: ✅ en `main` al cierre de P08 (558 tests; los live se omiten sin `SYMPHONY_LIVE=1`).
- `cargo deny check`: ✅ (solo avisos de duplicados).
- CI en `main`: ✅ (ubuntu, windows, macos, msrv, deny).
- Tests conocidos en rojo: ninguno.

## Cola de trabajo (en orden)
Detalle de cada paso (qué y **Verifica**) en el PLAN. Bitácora de la fase en `docs/phases/`.

**P11.5 · Pendientes y pulido** (`phase/p115-pendientes`, bitácora `docs/phases/P11.5-pendientes.md`)
- [ ] S1 `ctx://` con `@` y caracteres reservados en la ruta
- [ ] S2 Permisos de proveedores en `config.toml` (defaults iguales)
- [ ] S3 Respuesta construyéndose en el chat (deltas de Claude)
- [ ] S4 Router: `needed_context` y aviso de checkpoint obsoleto
- [ ] S5 Lote: el tope de simultáneos sigue a la cuota
- [ ] S6 Merge chat → `main` con review agent (ADR-0006)
- [ ] S7 Pulido de la TUI (milestones, «Enviar al agente», avisos que caducan)
- [ ] S8 Medir `resume` y `/compact` con OpenCode gratuito
- [ ] S9 Live gratuito con OpenCode (broker MCP, handoff, plan por fases, review)
- [ ] S10 Deuda anotada
- [ ] S11 Cierre (`p115-done`)

**P12 · Plugins** (`phase/p12-plugins`): S1 protocolo y manifiesto · S2 plugin host · S3 plugin de ejemplo · S4 cierre (`p12-done`)

**P14 · Sugerencias de modelo** (`phase/p14-sugerencias`): S1 migración **006** · S2 sugerencias por reglas · S3 desempeño aprendido (umbral en ADR) · S4 experimentos ML (ADR; rechazar es válido) · S5 cierre (`p14-done`)

**P15 · GUI de escritorio** (`phase/p15-gui`): S1 puente · S2 scaffold · S3 vistas · S4 diseño y QA · S5 bundles en CI · S6 cierre (`p15-done`)

**P13 · Endurecimiento y pre-release** (`phase/p13-endurecimiento`): S1 seguridad · S2 fuzzing · S3 recuperación y 4 h · S4 rendimiento y retención · S5 calidad del repo · S6 mdBook (+ guía SSH) · S7 pipeline de release en dry run · S8 cierre (`p13-done`; `v0.9.0` local)

**P16 · Validación final** (`phase/p16-validacion`): S1 checklist · S2 E2E (fake-agents) · S3 benchmark · S4 instalación limpia en CI · S5 docs as-built · S6 release ⛔ · S7 retrospectiva

## Permisos vigentes (sin preguntar)
- Merge `--no-ff` de la rama de fase a `main`, tag `pNN-done`, push de `main` y del tag (memoria «merge a main OK»).
- Live (`SYMPHONY_LIVE=1`) **solo con modelos gratuitos de OpenCode** (`opencode/…-free`, `opencode/big-pickle`), 2026-10-10.
- Requisito permanente (CONSTRAINTS A12): instalación única y global; nada se instala ni se escribe por proyecto.

## Para Leo (⛔ — el agente anota aquí y sigue)
Formato: paso · qué hace falta · comando exacto si aplica.
- **Siempre ⛔:** live que gaste cuota (Claude, Codex, Kimi, Antigravity, Copilot); push de ramas que no sean `main`; publicar releases o pushear tags `v*`; cambiar defaults de permisos de proveedores; ToS.
- P08 · Live de plan por fases y bloque ```review con Claude/Codex (si S9 con OpenCode no basta).
- P08 · Revisión de fluidez de las vistas 15–19 y 26–28 en una terminal real.
- P09 · Que Claude y Codex llamen a `context_search` en vivo y que Codex `exec` apruebe herramientas MCP sin preguntar.
- P11 · Re-validación de Kimi, Antigravity y Copilot (criterios en `docs/research/opencode-y-calidad-de-proveedores.md` § 4); ToS de los tres y de usar la credencial de Copilot dentro de OpenCode; skills de los tres; sondear los modelos habilitados de Copilot; Kimi sin sandbox.
- P11 · Los 17 pares live de handoff restantes (n = 1 hoy).
- ADR-0013 D2 · Medir `/compact` con Claude/Codex (si S8 no lo resuelve con OpenCode).

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
| P11.5 | ⏳ siguiente | |
| P12, P14, P15, P13, P16 | ⏳ en ese orden (ADR-0014) | |
