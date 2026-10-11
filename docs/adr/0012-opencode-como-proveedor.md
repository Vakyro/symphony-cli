# ADR-0012 · OpenCode como proveedor

- **Estado:** ACEPTADO
- **Fecha:** 2026-10-10
- **Autor:** claude-code/sonnet-5.5 · **Aprobado por:** Leo (2026-10-10)
- **Fase/paso:** pendiente de ubicar en el PLAN (después de P09, junto con P08; ver Consecuencias)

## Contexto
Leo pidió agregar OpenCode como agente disponible, ejecutado «como los demás» y con persistencia de contexto. Está instalado en su máquina (1.18.34, npm). Precedente: ADR-0008 y P11 agregaron Kimi, Antigravity y Copilot con el mismo contrato (`ProviderAdapter`), sin tocar el core.

Lo observado del CLI real está en [`docs/research/opencode-y-calidad-de-proveedores.md`](../research/opencode-y-calidad-de-proveedores.md) § 2: `opencode run --format json` con el prompt por stdin, un JSON por evento con el id de sesión, continuación con `-s <id>` verificada (recordó el turno anterior), herramientas sin permiso ni sandbox en `run`, errores genéricos (`UnknownError`) y modelos que lista `opencode models`.

## Opciones consideradas
1. **Un paso del PLAN sin ADR.** Es lo que haría un proveedor ya previsto; OpenCode no lo estaba, y CLAUDE.md pide ADR para cambios de alcance y de orden.
2. **ADR corto (elegida).** Registra las tres decisiones de diseño que Leo tomó y deja el paso como trabajo normal.
3. **Posponerlo hasta validar los demás proveedores.** Descartado por Leo: lo quiere ya.

## Decisión
1. **OpenCode entra como un proveedor más**, con un crate `symphony-adapter-opencode` detrás de `ProviderAdapter`. El core no cambia y no se agregan dependencias (solo `serde_json`, `symphony-core` y `symphony-adapter-common`, como Kimi).
2. **Esquema de ids de modelo** (aprobado por Leo):
   - `opencode/<modelo>` cuando el modelo es del proveedor propio de OpenCode (`opencode/big-pickle`).
   - `opencode/<proveedor>:<modelo>` para los de otros proveedores detrás de OpenCode (`opencode/github-copilot:gpt-5`).
   - `opencode/default` usa el modelo configurado en OpenCode (sin `-m`).
   - El `cli_model_id` conserva la forma de OpenCode, `proveedor/modelo`. El canónico no puede llevar dos `/`: la TUI y el `attach` parten por la primera.
3. **Failover automático desde el inicio** (decisión de Leo). OpenCode entra en el failover automático igual que Claude y Codex, sin esperar a validarlo.
4. **Persistencia de contexto en dos capas:**
   - Entre turnos del mismo proveedor, `resume` con `-s <id>`: OpenCode guarda el historial y Symphony no reenvía nada.
   - Entre proveedores, el handoff genérico de Symphony (BALANCED por defecto), igual que con los demás.
5. **Contrato del adapter** (resumen; el detalle va en la bitácora del paso):
   - Turno `run --format json`, prompt por stdin, `close_stdin_after_prompt = true`; mensajes a media tarea van en el siguiente turno.
   - Eventos: `step_start` → sesión; `text` → texto; `tool_use` → pedido, archivo tocado y resultado; `step_finish` → uso (y fin de turno con `reason: "stop"`); `error` → `ProviderError`.
   - El éxito de `bash` se deriva del código de salida (`metadata.exit`), no del estado de la herramienta.
   - Modelos descubiertos con `opencode models`, con tope de tiempo y de cantidad.
   - Errores de cuota, rate limit y auth: clasificación sintética hasta verlos en vivo (como en P11).
6. **Verificación:** fixtures sanitizados y suite de contrato (L1), `fake-agent` (L2) y una prueba live con un modelo gratuito de OpenCode, que no gasta la cuota de Leo (L3, solo con su permiso, CLAUDE.md).

## Consecuencias
- **Riesgo asumido por Leo:** con failover automático, un agente puede pasar a OpenCode sin que el proveedor esté validado (Leo: solo Claude Code y Codex le han funcionado bien; ver el registro). Mitigaciones que ya existen: los puntajes iniciales del router para OpenCode serán bajos (neutro, sin evidencia), así que Claude y Codex se prefieren mientras estén sanos; `--failover none` por agente; y desactivar el proveedor con `provider.set_enabled`.
- **Permisos:** OpenCode ejecuta herramientas sin pedir permiso y sin sandbox en `run` (como Kimi). Va documentado en QUICKSTART, y un agente de OpenCode corre en su propio worktree.
- **Credencial compartida:** su única credencial hoy es GitHub Copilot por OAuth, la misma cuenta que usa el proveedor Copilot. Revisar `docs/research/tos.md` antes de la prueba live.
- **Modelos:** el catálogo depende de las credenciales de Leo; con muchas, se registra solo hasta el tope.
- **Ubicación:** no estaba en el PLAN. Se agrega como un paso con su bitácora; el orden respecto de P09.S8 y P08 lo fija Leo.
- **Verificado en vivo (2026-10-11, modelos gratuitos, bitácora P11 § S7):** escribe en su worktree, `resume` recuerda, handoff entre dos modelos de OpenCode, broker MCP y un 429 de upstream clasificado. Dos correcciones que el diseño no preveía: OpenCode toma el repo principal como raíz y trata el worktree como directorio externo, así que el adapter enmarca el prompt (`frame_prompt`, una adición al trait) y permite por config solo `<worktree>/**`. La protección contra una ruta absoluta al repo principal es del prompt, no del sistema.
- **No cubierto aquí:** validar la calidad de OpenCode (criterios en el registro § 4) y cualquier cambio en cómo el router trata a los proveedores no validados; eso va, si Leo lo quiere, en un ADR aparte.
