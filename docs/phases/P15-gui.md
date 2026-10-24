# P15 · GUI de escritorio

| Campo | Valor |
|---|---|
| Estado | CERRADA (revisión visual en una ventana real pendiente → Para Leo) |
| Rama | phase/p15-gui |
| Inicio / cierre | 2026-10-12 / 2026-10-12 |
| Agentes que trabajaron | claude-code/sonnet-5.5 |
| Tag | p15-done |
| Docs usados | STACK §35, §55; FLOW §18 y regla 9 |

**Diseño.** La GUI es *otra representación del mismo estado* (FLOW §18): no tiene lógica de negocio ni abre la base. Pide al daemon por IPC lo mismo que la TUI y pinta lo que vuelve; las acciones llaman al mismo método IPC. Vive en `apps/gui` con **workspace de Rust propio** (Tauri trae cientos de dependencias que no deben entrar al gate ni a `cargo deny` del repo principal) y su CI es un job aparte.

## Pasos

### P15.S1 · Puente — ✅
- `crates/gui-bridge` (`symphony-gui-bridge`, en el workspace principal y en el gate): `Bridge::call` (un request por conexión, timeout 15 s, errores `Unavailable | Timeout | Remote{code,message}`), `subscribe` (bus `agent.event`) y `ensure_daemon` (ejecuta `symphony daemon start`, que ya sabe desacoplar el proceso). Sin Tauri, así se prueba contra un daemon real: `tests/bridge.rs`.
- `apps/gui/src-tauri/src/main.rs`: comandos `ipc_call` y `daemon_ensure`, y reemisión del bus como `symphony://event` / `symphony://disconnected` (reintenta cada 2 s).
- **Verifica:** `cargo nextest run -p symphony-gui-bridge`.

### P15.S2 · Scaffold — ✅
- Tauri 2 + SolidJS + TypeScript + Vite. Tema claro/oscuro con tokens (sigue al sistema o se fija a mano). Versiones fijadas a lo que acepta Node 22.11 (Vite 6, Vitest 3, TS 5.9); subir a Vite 8 exige Node ≥ 22.12.

### P15.S3 · Vistas — ✅
- Las 36 vistas 00–35 de FLOW §18 en el registro `views.tsx` (un test comprueba que están todas). Composición propia en Home, Chat, New Agent, Providers/Provider Setup/Detail, System, Recovery (con `recovery.act`), Tasks/Task Detail, Context Search, Usage y Model Picker; el resto usa el renderizador genérico (`Auto`: tabla, pares o texto) sobre el método que usa la TUI.
- **Límite conocido:** las vistas de agente en genérico (06–12, 24–29, 34) muestran la respuesta del daemon sin la jerarquía fina de la TUI (p. ej. diff coloreado, tiers de validación); se refinan con uso real.

### P15.S4 · Diseño y QA — 🟡
- Vitest + `@solidjs/testing-library` con un transporte falso (7 tests: registro, `Auto`, Home pide al daemon y navega al agente, error visible, New Agent envía lo elegido, vistas de agente sin selección). Modo demo (`pnpm dev` fuera de Tauri) con datos de ejemplo para revisar el diseño en un navegador; no entra al bundle de producción.
- Accesibilidad básica: etiquetas en formularios, `aria-current` en la navegación, foco visible, contraste con tokens por tema.
- **No hecho:** revisión visual en pantalla (la extensión de Chrome no estaba conectada) y DevTools del WebView → Para Leo.

### P15.S5 · Empaquetado — ✅
- `pnpm tauri build` local en Windows produce MSI (2,3 MiB) y NSIS (1,5 MiB). CI: job `gui` (typecheck, test, build) y `gui-bundle` (Linux/Windows/macOS, artefactos del workflow). La publicación entra en P13.S7.

## Estado final
GUI funcional sobre IPC, con las 36 vistas, tests de componentes, bundles locales y CI en job aparte. Pendiente de una persona: revisión visual y uso real.

## Notas para el siguiente agente
- Para probar con un daemon real: `pnpm tauri dev` en `apps/gui` (necesita `symphony` y `symphonyd` instalados). La raíz del proyecto se elige en el campo «Proyecto» de la cabecera.
- Cambiar un método IPC del daemon exige revisar `views.tsx`; los parámetros de proyecto son `project_root`.
