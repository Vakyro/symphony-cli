# LEARNINGS

Trampas descubiertas y comandos útiles. Anota en el momento, no al final.

## Prevalidación (P00.S0) — detalle en `docs/research/prevalidacion.md` §2.3

- **H1 · Los CLIs headless no siempre dejan TODOs estructurados.** `claude -p` no usó TodoWrite y `codex exec` no produjo `todo_list`; ambos escribieron el plan como texto. El checkpoint toma el **último mensaje del asistente** como fuente del "qué seguía" y usa TODOs estructurados solo si existen.
- **H2 · El transcript de Claude va detrás del disco tras un kill.** Un `Write` ya aplicado no aparecía en el `.jsonl`. Fuente de verdad: `git diff` + archivos nuevos. Si hay conflicto, gana git.
- **H3 · `claude -p --output-format stream-json` emite un evento de rate limit** con `five_hour.utilization`, `seven_day.utilization` y `resetsAt`. La cuota de Claude Code puede ser `KNOWN`. Confirmar en P01.S2.
- **H4 · Windows: lanzar un CLI desde Git Bash bloquea al padre** hasta que el hijo termina (hereda el pipe de stdout). `Start-Process -WindowStyle Hidden` + redirección a archivo lo evita. Para P04 (`ProcessSupervisor`): controlar explícitamente los handles heredables.
- **H5 · Codex tiene dos formatos de eventos:** en disco (`~/.codex/sessions/.../rollout-*.jsonl`, `response_item`/`event_msg`) y en `codex exec --json` (`item.*`). Soportar el modo interactivo implica dos parsers.
- **H6 · El costo del handoff depende de la config global del CLI.** Codex gastó ~880k tokens de entrada (835k cacheados) en una tarea pequeña porque cargó las skills de `~/.agents/skills`; Claude ~243k. Medir en P01.S3, considerar en P09.
- **H7 · La salida TAP de `npm test` entró cruda al handoff** (~2.5 KB). Evidencia a favor del colapso de logs de tests (P09.S3).
- **Kill en Windows:** `taskkill /F /T` sobre el shim `.cmd` de npm mata todo el árbol cmd → node sin dejar huérfanos.

## Entorno (P00.S1)

- **Rust en Windows necesita el workload "Desarrollo para el escritorio con C++" de Visual Studio** (link.exe + Windows SDK). Tener VS 2022 instalado no basta: sin ese workload `cargo run` falla con `linker link.exe not found`.
- **`git config core.autocrlf=true` en la máquina de Leo.** El repo fija `eol=lf` en `.gitattributes` para que rustfmt y la CI no vean diffs de fin de línea.
