# P13 · Endurecimiento y pre-release

Rama `phase/p13-endurecimiento`. Pasos en PLAN.md § P13. Desde 2026-10-24 parte del trabajo se
delega con Symphony mismo (dogfooding, `docs/progress/DOGFOOD.md`).

## S0 · Decisiones de Leo (2026-10-24)
- El harness de Symphony pasa a ser el defecto (`[harness] mode = "symphony"`).
- Antigravity ofrece sus 13 modelos (Google, Anthropic, OpenAI), con el proveedor en el id y en
  el nombre. Copilot solo tiene `auto`.
- La validación por tipo de proyecto (recortada de ADR-0020) va a P16.S0.
- El arreglo de tokio (RUSTSEC-2026-0386) entra a `main` al cerrar P13.
- Live sin tope, solo para mejorar Symphony y con plan por agente; preferir Kimi, OpenCode y
  Antigravity.

## S1 · Seguridad
Revisado: IPC, `hook emit`, `ctx://`, plugins, redacción, permisos del socket y el harness nuevo
(P15.5).
- **IPC:** ya estaba bien (P02.S8). Windows: DACL solo del dueño (`D:P(A;;GA;;;OW)`) y el pipe
  rechaza clientes remotos (`interprocess` pone `PIPE_REJECT_REMOTE_CLIENTS` por defecto; visto
  en su fuente 2.4.5). Unix: directorio `0700` propio y socket `0600`. El cliente verifica el
  pid del servidor. Frames de 8 MB como máximo.
- **`hook emit`:** stdin acotado a 4 MB.
- **`ctx://`:** se rechazan `..`, `.`, `%`, `\`, rutas absolutas y letras de unidad (test de
  intentos de escape).
- **Plugins:** corren con los permisos del usuario, como cualquier programa que instala.
- **Harness (hallazgos, corregidos):** `shell` juntaba toda la salida en memoria antes de
  recortarla (un comando que escupe gigas tumbaba el broker) y `read` cargaba el archivo entero.
  Ahora `shell` guarda solo cabeza y cola y `read` lee línea a línea. Tests:
  `a_huge_output_keeps_only_head_and_tail_in_memory`, `read_streams_lines_and_still_rejects_binaries`.
- **tokio 1.54.1** por RUSTSEC-2026-0386 (`cargo deny check` verde).

## S4 · Rendimiento y disco (adelantado por pedido de Leo: «Symphony debería ser super ligero»)
- RAM: el daemon en reposo usa 26 MB.
- Disco: worktrees que se borran solos (cancelado, integrado o completado hace 7 días; nunca
  con cambios sin commitear), `target/` de Cargo compartido entre agentes con tope de 15 GB,
  índice de contexto que suelta lo borrado o ignorado. Detalle en DOGFOOD.md.
- Diagnóstico: la cola de stderr de un CLI caído queda con el agente.

## S6 · Documentación (delegado)
- Libro mdBook en `docs/book/` (Antigravity), `cargo xtask er` con el ER (Kimi), QUICKSTART al
  día (OpenCode, modelo gratis). Revisados e integrados.
