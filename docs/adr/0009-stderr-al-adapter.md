# ADR-0009 · El executor entrega stderr al adapter

- **Estado:** PROPUESTO (pendiente de aprobación de Leo)
- **Fecha:** 2026-10-01
- **Autor:** claude-code/sonnet-5.5 · **Aprobado por:** —
- **Fase/paso:** P11.S2 (adapter Kimi)

## Contexto
PLAN P11 dice «sin cambios en el core: si un adapter necesita cambiar el core, se hace un ADR primero». Kimi lo necesita:
- **Kimi solo da su id de sesión por stderr** (`To resume this session: kimi -r <uuid>`); `--verbose` no lo mueve a stdout (`docs/research/cli-p11.md`).
- **El executor descarta stderr:** `executor.rs:285` (`Some(OutputLine::Stderr(_)) => self.beat(l.run_id)`) solo cuenta como latido.
- El runtime aprende el id únicamente por un `AgentEvent::SessionStarted` del stream (`handle_line`, ~l.387) y lo guarda con `set_run_process`; `launch_inner` pasa `session_id: None` al adapter. Sin id guardado no hay `resume` (`continue_session` exige `run.cli_session_id`), así que **un chat con Kimi perdería la sesión tras el primer turno**.
- Además, el doc del trait dice que `parse_error` recibe «texto de error (stderr…)», pero nadie lo llama con stderr. Copilot, por ejemplo, avisa de un modelo inexistente solo por stderr (`Error: Model "x" from --model flag is not available.`).

## Opciones
1. **Pasar las líneas de stderr a `parse_stream_line`.** Cero cambios al trait, pero mezcla dos flujos en un método documentado como stdout.
2. **Método nuevo `parse_stderr_line` con implementación por defecto vacía (elegida).** Los adapters actuales no cambian; el que lo necesita lo implementa.
3. **El executor genera el id y se lo pasa al adapter (`SpawnRequest.session_id`).** Exige una bandera «este CLI acepta id fijo» y cambia cómo arranca Claude, que hoy aprende su id por `system/init`.
4. **`kimi -C` (continuar la última sesión del directorio) con un id falso.** Funciona por casualidad y deja ids que no son ids en la base.

## Decisión propuesta
1. Añadir al trait `ProviderAdapter`: `fn parse_stderr_line(&self, _line: &str) -> Vec<AgentEvent> { Vec::new() }`.
2. En `pump`, cada línea de stderr sigue contando como latido **y además** pasa por `parse_stderr_line`; los eventos resultantes se tratan igual que los de stdout (`SessionStarted` guarda el id; un `ProviderError` que requiera failover lo dispara). Para eso `handle_line` se parte en «parsear» y «aplicar eventos».
3. `KimiAdapter` lo implementa con `session_from_stderr` (ya escrita y probada) y con `parse_error` sobre el resto de líneas.
4. Claude y Codex no cambian (usan el valor por defecto). Copilot (P11.S4) lo usará para sus errores de stderr.

## Consecuencias
- Cambio en `crates/adapters/common` (1 método) y `crates/daemon/src/executor.rs` (~15 líneas), con test L2: un proceso que escribe el id y un error por stderr (el `fake-agent` / `fake_adapter.rs` de testkit lo soporta o se extiende).
- **Riesgo:** una línea de stderr con texto parecido a un error de cuota podría disparar un failover; solo afecta a adapters que implementen el método, y `parse_error` ya es el clasificador de los demás.
- Sin migración ni dependencias nuevas.
- Si no se aprueba: el adapter de Kimi queda registrado pero sin `resume`; habría que sacarlo del selector del chat.

## Pregunta para Leo
¿Apruebas la opción 2? Si no, ¿prefieres la 3 (el executor fija el id)?
