# ADR-0008 · Adelantar los proveedores restantes (P11) antes del multiagente (P08–P10)

- **Estado:** ACEPTADO
- **Fecha:** 2026-10-01
- **Autor:** claude-code/sonnet-5.5 · **Aprobado por:** Leo (2026-10-01)
- **Fase/paso:** P07.S10 (revisión de P08–P16)

## Contexto
P07.5 cerró con el chat funcionando entre Claude Code y Codex (ADR-0006). La propuesta clave de Leo es un chat que cambia de proveedor sin perder contexto; cuantos más proveedores, más valor. El PLAN pone los proveedores restantes en P11, después de P08 (scheduler multiagente), P09 (Context Engine) y P10 (router/profiles). ADR-0006 ya señaló que P09 y P10 construyen cosas que el chat no usa.

## Evidencia (2026-10-01)
- **El core es genérico.** Fuera de los dos adapters, los ids `anthropic`/`openai` solo aparecen en el registro (`crates/daemon/src/providers.rs`, 2 líneas) y en el catálogo y prefijo de skills (`executor.rs:105`, `skills.rs:155`, `tui/app.rs:596,1044`). El failover elige al siguiente con `repo::next_executor`, sin lista fija.
- **El contrato ya existe y se prueba:** trait `ProviderAdapter` y suite de contrato en `crates/adapters/common`; P11 dice «sin cambios en el core».
- **Los tres CLIs están instalados** y exponen turno headless, salida estructurada y resume (`docs/research/cli-p11.md`).
- P08 (scheduler, DAG, merge, benchmark) y P10 (router con puntuación, reserva de cuota) no se usan en el chat; ADR-0007 pide gate de tiempo para P08.

## Opciones
1. **Mantener el orden P08 → P09 → P10 → P11.** Respeta el PLAN, pero los proveedores llegan al final, tras construir lo que el chat no necesita.
2. **Adelantar P11 justo tras P07.S10 (elegida).** Los adapters reutilizan lo que ya hay.
3. **Un solo proveedor nuevo primero, el resto después.** Más prudente, pero la suite de contrato ya cubre el riesgo común, así que no ahorra apenas.

## Decisión propuesta
1. **Orden nuevo:** P07.S10 → **P11** → (P10 recortada) → (P09 recortada) → P08. P08 sigue bajo el gate de tiempo de ADR-0007 §3.
2. **P11 se acota al chat:** un adapter por CLI (Kimi, Antigravity, Copilot), sus fixtures L1, la suite de contrato, registro en `providers.rs` y skills por proveedor (prefijo y catálogo). Test C / `hooks_can_hold` solo se investiga; no bloquea (queda `None`).
3. **Pasos y bitácora:** P11.S1 (investigación, ya empezada en `cli-p11.md`) → S2 Kimi → S3 Antigravity → S4 Copilot → S5 matriz de handoff → S6 cierre. Se mantiene el orden y las reglas del PLAN. Cada adapter nuevo con live es L3: **con permiso de Leo**.
4. **Matriz de handoff (S5):** con 5 proveedores son 20 pares dirigidos. Todos con `fake-agent`; live para Claude→Kimi, Codex→Copilot y Antigravity→Claude (como ya dice el PLAN), no los 20.
5. **P10 recortada:** solo lo que necesita el chat con N proveedores (parsers de error por adapter, estado de salud, `quota_certainty`). Router con puntuación, profiles y reserva de cuota se reevalúan después.
6. **P09 recortada:** solo medir el coste real del handoff con una conversación larga (pendiente de P07.5.S9). AST, watcher, consolidación de hechos y broker MCP se difieren.
7. **OpenCode** (instalado, fuera del PLAN): no se incluye salvo que Leo lo pida.

## Consecuencias
- Si se acepta: PLAN §8, P08–P11 y los avisos de prerrequisitos se actualizan citando este ADR; STATUS apunta a P11.S1.
- **Riesgo:** los parsers de cuota/error de cada CLI se escriben sin el router de P10; el failover básico de P06 ya basta para el chat, pero una política `any` con 5 proveedores no se ha probado.
- **Riesgo:** `agy` ofrece modelos de Anthropic y OpenAI; hay que revisar ToS y cuota antes de exponerlo (tos.md).
- Antes de S2, hay que probar en vivo el esquema de eventos de cada CLI (cuesta cuota).

## Respuestas de Leo (2026-10-01)
Aprobó el orden nuevo y el recorte de P09/P10, y dio permiso para las pruebas en vivo mínimas de P11.S1. Sobre OpenCode no respondió: queda fuera.

## Preguntas (resueltas)
1. ¿Apruebas el orden nuevo (P11 tras P07.S10) y el recorte de P09/P10?
2. ¿Permiso para las pruebas en vivo mínimas (un turno corto por CLI) en P11.S1?
3. ¿Incluyo OpenCode como cuarto proveedor o lo dejo fuera?
