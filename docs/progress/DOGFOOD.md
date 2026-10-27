# Dogfooding: construir Symphony con Symphony

Desde 2026-10-24 (decisión de Leo): Symphony se usa para mejorar Symphony. Live sin tope de
mensajes, **solo** para tareas que mejoren Symphony y siempre con un plan escrito por agente.

## Montaje

- **Copia estable** en `~/.symphony-stable/bin` (`symphony.exe` + `symphonyd.exe`; el commit
  está en `~/.symphony-stable/VERSION`). Con ella se delega; no se toca mientras se desarrolla.
  Se actualiza solo al cerrar un paso con el gate verde (copiar los binarios de
  `cargo install`). Si sale mala, se vuelve a la anterior.
- **El repo** es lo que se modifica. Su daemon (el de la copia estable) vive en
  `<repo>/.symphony/`, aparte de los proyectos de Leo; los tests usan carpetas temporales.
- **Disco:** los agentes comparten el `build-dir` de Cargo en `<repo>/.symphony/cache/cargo-build`
  (`CARGO_BUILD_BUILD_DIR`; los binarios finales quedan en el `target/` de cada worktree), y los
  worktrees de agentes integrados o cancelados se borran solos.

## Reglas para cada agente delegado

Cada tarea sale con este plan en la descripción (`symphony spawn -d`):

1. Lee `AGENTS.md` y el paso de `PLAN.md` que se te asigna; nada fuera de él.
2. Sin `unwrap`/`expect`/`panic` en runtime; docs, comentarios y commits en español.
3. Verifica solo lo tuyo: `cargo nextest run -p <crate>` y `cargo clippy -p <crate> -- -D warnings`
   (el gate completo lo corre quien integra, también el del inicio). Un build que falla por algo
   ajeno (otro agente, un candado de Cargo) se reintenta, no es motivo para parar.
4. Un commit por tarea, Conventional Commits con el tag del paso.
5. Si algo no se puede como está escrito, escríbelo en tu último mensaje y para.

Quien integra (Claude Code) revisa el diff, corre el gate y anota aquí lo que funcionó y lo
que no.

## Proveedores: prioridad

Kimi, OpenCode y Antigravity primero (Leo casi no los usa). Claude Code y Codex solo si hace
falta. Rotar modelos dentro de cada proveedor para medir.

## Resumen de la primera jornada (2026-10-24)

Seis tareas delegadas, tres proveedores, seis modelos distintos (cuatro de pago, dos gratis):
cinco terminaron con commit útil, una dejó el trabajo hecho pero sin commit. Todas se revisaron
antes de integrar; ninguna inventó comandos. Lo que más falló no fueron los modelos sino
Symphony: siete problemas reales encontrados en un día, cinco arreglados.

## Registro

| Fecha | Proveedor / modelo | Tarea | Resultado | Hallazgo |
|---|---|---|---|---|
| 2026-10-24 | Antigravity · `google/gemini-4-argon-high` | P13.S6 esqueleto del libro (6 capítulos, 830 líneas) | ✅ 10 min, un commit; los 45 comandos citados existen; fiel a LEARNINGS | Sus herramientas nativas (no admite el harness) |
| 2026-10-24 | Kimi · `moonshot/kimi-for-coding` | P13.S6 `cargo xtask er` (parser DDL + 3 tests) | ✅ el trabajo, 5,5 min, un commit con tests y clippy verdes; ❌ Symphony lo marcó `CRASH` | Kimi se cae en Windows al imprimir «⛔» (cp1252); el mensaje de commit salió con los backticks escapados (`\``) |
| 2026-10-24 | OpenCode · `opencode/deepseek-v4.1-flash-free` (gratis) | P13.S6 QUICKSTART al día con P15.5 | ✅ 7 min, un commit; corrigió además comandos viejos que no existían (`symphony merge`) | — |
| 2026-10-24 | Kimi · `moonshot/kimi-for-coding` (ya con UTF-8) | P13.S2 targets de cargo-fuzz + propiedades proptest | ✅ 30 min, un commit; documentó su parte en la bitácora; ningún panic real | No pudo compilar `fuzz/` (sin nightly) y lo dijo |
| 2026-10-24 | OpenCode · `opencode/glm-5.3-flash-free` (gratis) | P13.S5 typos y lychee en CI | ✅ 4 min, un commit; versiones de las acciones existen | El job se dejó `continue-on-error` hasta ver su primera corrida (repo en español) |
| 2026-10-24 | Antigravity · `anthropic/claude-sonnet-5-5-medium` | P13.S3 prueba de larga duración con reinicios | 🟡 escribió `soak.rs` (funciona: 60 s, 22 tandas, RSS 25 → 25 MB) pero no commiteó: el watchdog lo cortó a los 23 min (`NO_HEARTBEAT`) | Ver hallazgo del watchdog |
| 2026-10-26 | Codex · `openai/gpt-6-astra` | P13.S3 checkpoint corrupto | 🟡 implementación buena (499 líneas, test de matar el writer a media transacción) pero paró en `wip`: corrió el gate completo al empezar y chocó con otros agentes. Lo terminó el integrador; dejó una regresión (arranque fatal con una fila rara) que vio la cobertura de CI | AGENTS.md vs. delegados |
| 2026-10-26 | Kimi · `moonshot/kimi-for-coding` | P13.S4 retención de `events` y `resource_samples` | ✅ a la primera: lotes por el writer, respeta las FK, tests con 20 000 eventos | Kimi agotó su ventana de 5 h a las 23:18 |
| 2026-10-26 | Antigravity · `openai/gpt-oss-120b-medium` | P13.S6 capítulos del libro | ❌ **escribió en el repo principal** (rutas absolutas armadas desde la ruta de la caché de Cargo), 166 líneas flojas. Detenido | Aislamiento |
| 2026-10-26 | Kimi · `moonshot/k3-256k` · Antigravity · `google/gemini-4-argon-low` · Codex · `openai/gpt-5.6-luna` | P13.S5 auditorías de store, router/scheduler y merge/validación | ✅ las tres concretas y verificables (una alta de las cinco era falsa) | — |
| 2026-10-26 | Antigravity · `google/gemini-4-argon-high` | P13.S6 adapters, comandos y plugins | ✅ comandos verificados contra `--help`; ❌ reemplazó el capítulo de problemas existente por uno más corto (la tarea no decía que existía) | Tareas: revisar lo que ya hay |
| 2026-10-26 | Codex `gpt-6.1-sol` → Antigravity `claude-sonnet-5-5-high` → `gemini-4-argon-medium` | P13.S5 arreglos del executor | ✅ 4 hallazgos corregidos con test y 8 diferidos con buenas razones; pasó por tres modelos con `switch` (Codex sin créditos, Antigravity-Anthropic sin cuota por 4,5 días) | Agent ≠ Model en vivo |
| 2026-10-26 | OpenCode · `opencode/flint-40b-free` | P13.S5 auditoría de process/git/protocol | ⚠️ no evaluable: 503 del endpoint gratis | — |

## Hallazgos (lo que se arregla)

- **2026-10-24 · OpenCode `NOT_FOUND`.** El paquete npm pasó de `opencode-ai` a `@opencode/cli`
  y el adapter buscaba el `.exe` en la ruta vieja. Arreglado (acepta las dos).
- **2026-10-24 · Disco.** Los worktrees nunca se borraban (60 en `~/.symphony/worktrees`, con
  `node_modules` y `target/` de hasta 1,5 GB cada uno) y cada agente de un proyecto Rust
  compilaba todo desde cero. Arreglado: `target/` compartido por proyecto (tope de 15 GB, se
  vacía al arrancar si lo pasa) y borrado del worktree limpio al integrar o cancelar.
- **2026-10-24 · Índice de contexto.** Archivos que entraron antes de su regla en
  `.gitignore` (p. ej. `apps/gui/src-tauri/target/**`) no salían nunca: ~25 MB de 97 en la base
  de Leo. Arreglado: el recorrido inicial quita lo que ya no corresponde.
- **2026-10-24 · `CARGO_TARGET_DIR` no llegaba a los comandos.** Con el harness, `shell` lo corre
  el broker MCP, y el broker solo recibía las variables `SYMPHONY_*`. Kimi compiló en su propio
  `target/`. Arreglado: el broker también recibe `CARGO_TARGET_DIR`; la comparación de rutas
  ya no distingue mayúsculas en Windows (`Desktop` y `desktop`).
- **2026-10-24 · Kimi se cae en Windows con caracteres fuera de cp1252.** Python escribe su salida
  en cp1252 cuando va a un pipe; el primer «⛔» que leyó de STATUS.md lo tumbó con
  `UnicodeEncodeError` (su log: `~/.kimi/logs/kimi.log`). El trabajo estaba hecho y commiteado,
  pero Symphony dejó de recibir eventos y lo marcó `CRASH`. Arreglado: `PYTHONIOENCODING=utf-8`
  y `PYTHONUTF8=1` en el entorno de Kimi.
- **2026-10-24 · El stderr de un run caído no se guarda.** Solo quedó «kimi terminó con código 1»;
  la causa estaba en el log de Kimi. Arreglado: las últimas 3 líneas de stderr (redactadas) van con
  la razón del fallo, en el agente y en el recovery item.
- **2026-10-24 · Integrar fuera de Symphony deja el worktree para siempre.** Symphony integra en
  `symphony/chat`; aquí se integra en la rama de fase (cherry-pick), así que el agente nunca
  llega a `MERGED`. Arreglado: un completado sin tocar en 7 días también libera su worktree.
- **2026-10-24 · Harness: `shell` y `read` sin tope de memoria.** La salida de `shell` se juntaba
  entera antes de recortarla y `read` cargaba el archivo completo. Arreglado (revisión P13.S1):
  cabeza y cola acotadas, lectura línea a línea.
- **2026-10-24 · El watchdog mata agentes que compilan en silencio.** Un run sin salida durante
  15 min (`DEFAULT_STALE_AFTER`) se cierra como `NO_HEARTBEAT`, aunque su `cargo` siga trabajando.
  Le pasó a Antigravity (sus herramientas nativas no emiten eventos mientras corre un comando)
  compilando los tests del daemon. **Pendiente:** contar como latido la CPU del árbol de procesos
  del run (ya se muestrea cada pocos segundos en `record_processes`).
- **2026-10-24 · Antigravity no deja mensajes en el stream** hasta el final: `symphony logs` solo
  muestra el prompt. Pendiente: revisar su parser de stdout con una corrida real.
- **2026-10-24 · Symphony integra en `symphony/chat`, no en una rama de fase.** Para este repo la
  integración se hizo a mano (cherry-pick). Pendiente: elegir la rama destino al integrar.
- **2026-10-24 · RAM.** El daemon en reposo usa 26 MB (9 hilos): no es problema.
- **2026-10-26 · `target/` compartido: los binarios de un agente pisaban los de otro.** Con
  `CARGO_TARGET_DIR` compartido, los archivos finales (`target/debug/symphonyd.exe`, lo que
  apunta `CARGO_BIN_EXE_*`) son los mismos para dos worktrees en la misma carpeta: los tests de
  uno pueden correr el binario del otro. `gpt-6-astra` lo vio como un fallo de compilación y paró.
  Arreglado: se comparte solo el `build-dir` (lo intermedio, con hash por ruta del paquete) y
  cada worktree guarda sus binarios. Verificado con un crate mínimo: `CARGO_BIN_EXE_*` apunta al
  `target/` del worktree.
- **2026-10-26 · Aislamiento: Antigravity no confina rutas.** Sin el harness de Symphony
  (Antigravity no lo admite) y con `--dangerously-skip-permissions`, `gpt-oss-120b` escribió
  capítulos del libro directo en el repo principal usando rutas absolutas. Ninguna guardia de
  Symphony lo detectó; el integrador lo vio en `git status`. **Pendiente:** comparar el estado
  del repo principal antes y después de cada run y abrir un recovery `WORKSPACE_DAMAGED`; y
  avisar en la tarea (hecho en las reglas del integrador). Mismo riesgo que P11.5.S9 (`n1.txt`).
- **2026-10-26 · Los agentes cambian de rama.** AGENTS.md dice «nunca en main, usa ramas
  `phase/`» y dos agentes crearon su propia rama dentro del worktree. Symphony no lo nota (el
  diff y la integración miran la rama asignada). Regla del integrador: no cambiar de rama.
- **2026-10-26 · Ventanas de cuota sin hora de reinicio.** Kimi corta a las 5 h («You've
  reached your 5-hour usage limit») y Antigravity dice «Resets in 108h54m13s», pero
  `provider_failures.reset_at` queda vacío: el router no sabe cuándo vuelve el proveedor.
- **2026-10-26 · Disco lleno.** El `target/` del integrador llegó a 104 GB y dejó el disco en
  0: los agentes se colgaron y Symphony solo lo notó 15 min después, por `NO_HEARTBEAT`. El
  Resource Manager mira CPU y RAM, no el disco. Arreglo inmediato: `cargo clean`.
- **2026-10-26 · AGENTS.md pide el gate completo al empezar.** Con varios agentes compilando a
  la vez se vence o choca. Para delegados, quien integra corre el gate; el agente verifica solo
  su crate (reglas de delegación).
