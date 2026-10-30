# Plan de ejecución P17 → P20 → P16 (2026-10-29)

Qué agente hace qué, con qué modelo y en qué orden. Se basa en:
- Las decisiones de Leo (`DECISIONES-2026-10-29.md`).
- Los pasos del PLAN (§ P16, § P17–P20).
- El histórico de calidad por modelo (`model-reviews.csv`, 45 corridas).

**Estado: propuesta. No se ejecuta nada hasta el visto bueno de Leo.**

---

## 1. Proveedores disponibles y rol de cada modelo

Estado de cuotas según Leo (2026-10-29):
- **Disponibles:** Codex, Kimi, Antigravity con Gemini y Copilot.
- **Agotados:** Claude Code y Antigravity con Claude/GPT.
- **OpenCode:** a la mitad semanal.

| Rol | Modelo principal | Por qué (histórico) | Respaldo si falla o se agota |
|---|---|---|---|
| **Código de features** (TUI, daemon) | `moonshot/kimi-for-coding` | 3 corridas de código: 5, 4, 5; respeta FK y escribe tests | `openai/gpt-6-luna` → `google/gemini-4-argon-high` |
| **Código medio / IPC** | `openai/gpt-6-luna` (Leo: luna como base) | sin histórico propio; se mide aquí. `gpt-5.6-luna`: 4 en auditoría | `openai/gpt-6.1-sol` (4–5 en código) |
| **Diseño difícil** (planeador, contratos) | `openai/gpt-6-astra` | 5 en auditoría, 4 en feature; detecta contradicciones | `openai/gpt-6.1-sol` → `moonshot/k3.1-pro` |
| **Tests y E2E** | `google/gemini-4-argon-low` | 5 en el harness E2E, 4 en auditoría | `moonshot/kimi-for-coding` |
| **Docs y daemon de complejidad media** | `google/gemini-4-argon-medium` | 4 en auditoría; corrigió las citas inventadas por flash | `openai/gpt-5.6-luna` |
| **Revisión de diffs (cada ola)** | `moonshot/k3-256k` | 5 en auditoría: sin altas inventadas | `google/gemini-4-argon-medium` |
| **Revisión de cierre de fase** | rota entre `gpt-6-astra`, `k3-256k` y `argon-medium` | los tres con 4–5 en auditoría alta | — |
| **Orquestador / integrador** | esta sesión (Claude Code) | — | `opencode-go/deepseek-v4-pro` (como el 2026-10-28) |

**Fuera del reparto:**
- `gemini-3.8-flash`, en tareas con citas: inventó 37 de 146 funciones.
- `github/auto` (Copilot) con el harness: 3 de 3 rechazados. Solo se le da una prueba barata en modo `native` (ver Ola 11).
- Modelos `*-free` de OpenCode: dos escribieron fuera de su worktree. Quedan solo como reserva.

**Reparto total** (sin contar el integrador): Kimi ~10 tareas, Codex ~10, Gemini ~11, Copilot 1 de prueba. Ningún proveedor carga con más de una fase seguida.

## 2. Reglas de operación (aprendidas en el dogfooding)

1. **Máximo 2 agentes compilando a la vez**, más los de docs. Hay contención del candado de Cargo, timeouts de 300 s y `LNK1104`, y el disco C: está al 97 %. `cargo clean` antes de la Ola 1.
2. **Los agentes no corren el gate completo.** Solo `cargo nextest run -p <crate>` de lo que tocan. Así se evita lo de `gpt-6-astra` en #32. El gate completo lo corre el integrador al cerrar cada ola y cada fase (decisión de la pregunta 5).
3. **Integración por cherry-pick y no por merge del worktree.** Antes la revisa el revisor de la ola.
4. **Pasos que tocan `crates/tui/src/app.rs`:** van a módulos nuevos con enganches mínimos en `app.rs`; el integrador resuelve el choque.
5. **`unset SYMPHONY_LIVE` antes de cada cargo.** Los live se lanzan solo a propósito.
6. **Cada ola cierra con:** cherry-pick, revisión, gate, bitácora de la fase y STATUS.

## 3. Decisiones que tomé yo (las delegadas)

- **P17.S5 IPC (pregunta 14):** IPC nuevo `agent.processes` + suscripción a la salida en vivo, reutilizando el bus de eventos que ya alimenta la TUI. Los logs persistidos solo no sirven para «en vivo».
- **P19.S1 runtime (pregunta 36):** proceso local **aparte y opcional**. Symphony solo define el contrato (JSON por stdin/stdout) y no trae Python: CONSTRAINTS prohíbe un runtime de Python y ML en la ruta crítica. Sin backend instalado, Symphony usa el router determinista de siempre. Laya es el backend recomendado en la documentación.
- **P18.S4 Graphify al iniciar:** se implementa en Rust sobre `crates/context` (tree-sitter L0–L5 ya existe). No se invoca el `build.py` de Python, por la misma restricción.
- **P16.S5 docs as-built (pregunta 26):** anexo de desviaciones dentro del mdBook (`docs/book/src/`). Es lo que lee el usuario final.
- **Merge vs. rebase (pregunta 51):** guía corta en el mdBook y regla en el harness para los agentes: rebase en ramas propias no pusheadas y merge `--no-ff` para integrar fases. Los agentes la aplican sin pensarla.
- **Pregunta 37 («Jev los elegirá»):** lo interpreto como «el modelo de decisión elige los perfiles». Es Laya por defecto o el backend que tenga el usuario, y el router determinista si no hay ninguno. El PLAN dice «no Jev» como dependencia fija, y esto lo respeta.
- **Medición live de `/compact`:** la pregunta 6 autoriza `SYMPHONY_LIVE=1`, así que entra en el plan (Ola 6) con Codex, que tiene cuota, y con el tope de 2 mensajes por corrida.

## 4. ADRs nuevos (los escribe el orquestador antes de la ola que los usa)

| ADR | Tema | Antes de |
|---|---|---|
| ADR-0022 | Status global: lectura de datos entre proyectos (enmienda de ADR-0018) | Ola 1 |
| ADR-0023 | Protocolo de marcadores de color del chat (P17.S8, opción B) | Ola 3 |
| Enmienda ADR-0013 D2 | `/compact` automático cada 3 tareas y prompt de sesión larga | Ola 5 |
| ADR-0024 | Contrato del modelo de decisión (P19.S1): backend enchufable y umbrales 0.9 / 0.6 | Ola 7 |

---

## 5. Olas de trabajo, en orden

Dentro de una ola, las tareas corren **en paralelo** (respetando el máximo de 2 compilando). Una ola no empieza hasta que la anterior está integrada y con gate en verde.

### Ola 0 · Preparación (orquestador, sin agentes)
- Actualizar PLAN y STATUS con las decisiones:
  - S4 pasa a ser el piano.
  - Habbo sale del plan y queda como idea futura.
  - Se anota la aclaración sobre Jev.
- Escribir ADR-0022.
- `cargo clean` y gate base en `phase/p17-tui`.

### Ola 1 · P17, parte daemon y mascota

| # | Tarea | Modelo | Toca |
|---|---|---|---|
| A1 | **P17.S4** Mascota piano de una octava en el chat principal. Frases con personalidad, tick de 1 s, `SYMPHONY_NO_ANIM` y config en `~/.symphony` con override por proyecto. Respeta `NO_COLOR`. | `kimi-for-coding` | `crates/tui` (módulo nuevo), `crates/core` (config) |
| A2 | **P17.S5 daemon:** IPC `agent.processes` y streaming de stdout/stderr de los procesos lanzados por el agente. Solo lectura, sin stdin. | `gpt-6-luna` | `crates/daemon` (`server.rs`, `executor.rs`), `crates/protocol` |
| A3 | **P17.S6 daemon:** `usage.global` entre proyectos (ADR-0022), datos del heatmap por día y tokens, e IPC de ramas `git.branches / checkout / delete`. El borrado nunca fuerza una rama no mergeada. | `gemini-4-argon-medium` | `crates/daemon`, `crates/git` |

- **Revisión:** `k3-256k`.
- **Compilando:** A2 y A3 a la vez; A1 cuando termine uno de los dos.

### Ola 2 · P17, vistas de la TUI

| # | Tarea | Modelo |
|---|---|---|
| A4 | **P17.S5 TUI:** pestaña «Procesos» en la vista del agente, con salida en vivo. | `gpt-5.6-luna` |
| A5 | **P17.S6 TUI:** avance de agentes en el plan, vista de ramas (cambiar sin confirmar; borrar con confirmación), status global y heatmap estilo GitHub. Snapshots a 80 y 160 columnas. | `gemini-4-argon-low` |

- **Revisión:** `k3-256k`.

### Ola 3 · P17.S8 y cierre de P17
- Antes de empezar: ADR-0023 (orquestador).

| # | Tarea | Modelo |
|---|---|---|
| A6 | **P17.S8:** render de los marcadores en la TUI (texto blanco y color solo en títulos, bullets, destacados y tablas) y persistencia en el transcript (`recorder`/`store`). | `kimi-for-coding` |
| A7 | **P17.S8 harness:** documentar el protocolo de marcadores en el prompt del harness y en el mdBook. | `gemini-4-argon-medium` |

- **Cierre P17.S7:** revisión de toda la fase con `gpt-6-astra`, gate, merge `--no-ff` a `main`, tag `p17-done` y push.

### Ola 4 · P18, medición base y cambios chicos
- Rama nueva: `phase/p18-contexto`.

| # | Tarea | Modelo |
|---|---|---|
| B1a | **P18.S1 benchmark fijo:** mismas tareas con fake-agent (L2) y tokens de contexto por ruta del router y del handoff. Es la línea base del 40 %. | `gemini-4-argon-low` |
| B2 | **P18.S2:** Flintstone activado por defecto en el harness, con opción de apagarlo en `[harness]`. | `gpt-6-luna` |
| B3 | **P18.S5:** `/init` de Symphony actualizado. | `gpt-5.6-luna` |

- **Revisión:** `argon-medium` (rota para no cargar a Kimi).

### Ola 5 · P18, cambios grandes
- Antes de empezar: enmienda de ADR-0013 D2 (orquestador).

| # | Tarea | Modelo |
|---|---|---|
| B1b | **P18.S1 recortes:** quitar lo que sobra en el router y el handoff, medido contra B1a. Meta: −40 %. | `gpt-6.1-sol` |
| B4 | **P18.S3:** `/compact` automático cada 3 tareas por agente y prompt de confirmación en la TUI para sesión larga. | `kimi-for-coding` |
| B5 | **P18.S4:** grafo del proyecto al iniciar la conversación, en Rust sobre `crates/context`, con refresco por fase. | `k3.1-pro` (respaldo `kimi-for-coding`; suele dar «engine overloaded») |
| B6 | **P18.S6:** segundo cerebro en `.symphony/brain/`, ignorado en git, generado por el daemon (decisiones, cambios, bitácora por agente). | `gemini-4-argon-medium` |

- **Choque previsible:** B1b y B4 tocan `handoff.rs`. Va primero B1b y B4 después dentro de la ola.
- **Revisión:** `k3-256k`.

### Ola 6 · Cierre de P18
- Re-medir con B1a. Si no llega al −40 %, otra pasada de `gpt-6.1-sol` (una sola) y se documenta la cifra real.
- **Live de `/compact`** con `gpt-6-luna`, tope de 2 mensajes (ADR-0013 D2).
- **Cierre P18.S7:** revisión con `k3-256k`, gate, merge, tag `p18-done` y push.

### Ola 7 · P19.S1, modelo de decisión
- Rama nueva: `phase/p19-orquestacion`.
- Antes de empezar: ADR-0024, con `gpt-6-astra` como revisor del diseño.

| # | Tarea | Modelo |
|---|---|---|
| C1 | Contrato y adaptador del backend de decisión: proceso local opcional, config para enchufar Laya, Jev o uno propio, cascada 0.9 / 0.6, y fallback al router determinista. | `gpt-6.1-sol` |
| C1t | Backend falso en `testkit` y tests de la cascada (actúa, pide confirmación, escala). | `gemini-4-argon-low` |

### Ola 8 · P19, perfiles y effort

| # | Tarea | Modelo |
|---|---|---|
| C2 | **P19.S2:** perfiles `@best-coding`, `@best-reasoning`, `@fastest`, `@cheap` y `@automatic`, elegidos por el modelo de decisión. Se deciden al inicio de la sesión, solo con modelos con créditos. | `kimi-for-coding` |
| C3 | **P19.S4:** effort en tres niveles (barato, producido, razonado) en la delegación. | `gpt-6-luna` |

### Ola 9 · P19, planeador y `/orchester`

| # | Tarea | Modelo |
|---|---|---|
| C4 | **P19.S3:** planeador con plan por fases y grafo dinámico. Se activa cuando el usuario lo pide o cuando detecta trabajo grande. | `gpt-6-astra` |
| C5 | **P19.S5:** `/orchester <mensaje>` crea el plan y los agentes solos. Test L2 de punta a punta con fake-agents. Va después de C4. | `kimi-for-coding` |

### Ola 10 · P19.S6 y cierre de P19

| # | Tarea | Modelo |
|---|---|---|
| C6a | Lista de capacidades de Claude Code contra las del chat de Symphony, con huecos priorizados: editar archivos, programar y responder bien. | `gemini-4-argon-medium` |
| C6b | Huecos de código: se reparten entre `kimi-for-coding` y `gpt-6-luna` según el tamaño de cada uno. | mixto |

- **Cierre P19.S7:** revisión con `k3-256k`, gate, merge, tag `p19-done` y push.

### Ola 11 · P20, editor y grafo
- Rama nueva: `phase/p20-editor`.

| # | Tarea | Modelo |
|---|---|---|
| D1a | **P20.S1 editor:** buffer, cursor, guardado y atajos de nano tal cual (`^O`, `^X`, `^K`). Lógica pura con tests. | `kimi-for-coding` |
| D1b | **P20.S1:** árbol de archivos, pantalla dividida y marcas `+`/`-` en vivo con los cambios del agente. | `gpt-6-luna` |
| D2 | **P20.S2:** grafo del proyecto navegable al estilo Obsidian, sobre B5. | `gemini-4-argon-medium` |
| D3 | **P20.S4:** releer la guía de Tailscale + SSH + tmux y ajustarla a lo nuevo. Es la **prueba de Copilot** en `[harness] mode = "native"`. Si falla, la hace `gemini-4-argon-low`. | `github/auto` |

- **P20.S3 Habbo:** fuera del plan, solo como idea futura.
- **Cierre P20.S5:** revisión con `gpt-6-astra`, gate, merge, tag `p20-done` y push.

### Ola 12 · P16, validación y release
- Rebasar `phase/p16-validacion` sobre `main` (orquestador).

| # | Tarea | Modelo |
|---|---|---|
| E1 | **P16.S2:** E2E finales con fake-agents, incluidas las features de P17 a P20. | `gemini-4-argon-low` |
| E2 | **P16.S5:** docs as-built y anexo de desviaciones en el mdBook. Cada comando citado se verifica contra `--help`. | `gemini-4-argon-medium` (verifica `k3`) |

- **Orquestador:**
  - Re-medir el benchmark (P16.S3).
  - Mergear P16 a `main` para despachar `install-check.yml` (P16.S4).
  - Crear `Vakyro/homebrew-symphony`.
  - Retrospectiva (P16.S7).
- ⛔ **Leo:** veredicto final del benchmark.
- ⛔ **Leo:** OK escrito en el chat para el tag `v0.9.0` y después para `v1.0.0` (P16.S6).

---

## 6. En paralelo a todo (solo Leo, no bloquea)

- `WAITING_PROVIDER` con reintento automático: ¿sí o no?
- Salud por modelo en vez de por proveedor: ¿sí o no?
- Revisión visual de la GUI (P15.S4).
- Borrar `C:\LATITU~1`: `rmdir /s /q C:\LATITU~1`.
