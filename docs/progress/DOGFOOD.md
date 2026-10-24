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
- **Disco:** los agentes comparten `<repo>/.symphony/cache/cargo-target` (`CARGO_TARGET_DIR`),
  y los worktrees de agentes integrados o cancelados se borran solos.

## Reglas para cada agente delegado

Cada tarea sale con este plan en la descripción (`symphony spawn -d`):

1. Lee `AGENTS.md` y el paso de `PLAN.md` que se te asigna; nada fuera de él.
2. Sin `unwrap`/`expect`/`panic` en runtime; docs, comentarios y commits en español.
3. Verifica solo lo tuyo: `cargo nextest run -p <crate>` y `cargo clippy -p <crate> -- -D warnings`
   (el gate completo lo corre quien integra).
4. Un commit por tarea, Conventional Commits con el tag del paso.
5. Si algo no se puede como está escrito, escríbelo en tu último mensaje y para.

Quien integra (Claude Code) revisa el diff, corre el gate y anota aquí lo que funcionó y lo
que no.

## Proveedores: prioridad

Kimi, OpenCode y Antigravity primero (Leo casi no los usa). Claude Code y Codex solo si hace
falta. Rotar modelos dentro de cada proveedor para medir.

## Registro

| Fecha | Proveedor / modelo | Tarea | Resultado | Hallazgo |
|---|---|---|---|---|

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
- **2026-10-24 · RAM.** El daemon en reposo usa 26 MB (9 hilos): no es problema.
