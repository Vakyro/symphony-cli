# ADR-0020 · Symphony para cualquier tipo de trabajo

- **Estado:** ACEPTADO
- **Fecha:** 2026-10-23
- **Autor:** agente (claude-code/opus-5.5) · **Aprobado por:** Leo (2026-10-23)
- **Fase/paso:** P15.5.S0

## Contexto
Leo quiere usar Symphony para video, documentos o lo que se necesite, no solo código, manteniendo el resto como está. Lo que hoy supone código: un repo git por proyecto, el gate de validación (build y tests), el clasificador de operaciones, los perfiles del router y las sugerencias (P14), el motor de contexto con tree-sitter, el review y el merge, y los prompts.

## Opciones consideradas
1. **Un producto aparte para cada tipo de trabajo.** Duplica todo.
2. **Mismo núcleo con un tipo de trabajo (`kind`) que cambia los defaults.**
3. **Quitar todo lo específico de código.** Rompe lo que ya funciona.

## Decisión
Opción 2.

- **`kind`** en `project.toml` y por tarea: `code` (hoy, por defecto), `document`, `media`, `general`. No cambia la máquina de estados ni las tablas; cambia defaults.
- **Git se mantiene** como mecanismo de worktrees, checkpoints y merge. Si la carpeta no es un repo, `project.init` ofrece `git init`.
- **Validación por tipo:** `code` como hoy. `document` y `media`: el archivo existe y su formato se abre (comprobación por extensión, sin dependencias nuevas), más cualquier plugin validator (ADR-0015) y la revisión de Leo. Un proyecto sin tests no se bloquea por no tener checks.
- **Binarios grandes:** pasan al `object-store` (BLAKE3 + zstd); git guarda un archivo puntero (`*.symphony-blob`) y el agente ve el archivo real materializado en su worktree. Umbral configurable (por defecto 5 MB).
- **Router y sugerencias:** perfiles nuevos `@write`, `@media` y `@research` con los mismos pesos deterministas; el aprendizaje (ADR-0016) los trata como cualquier otro profile.
- **Clasificador de operaciones:** las herramientas del harness (ADR-0019) clasifican por nombre y no solo por comando; `shell` se sigue clasificando por comando.
- **Prompts** neutros: «trabajo» en lugar de «código»; el de código se añade cuando `kind = code`.
- **Quién produce el video o el documento:** los skills y MCP que lleve el harness (ADR-0019). Symphony orquesta, persiste y versiona; no incluye generadores.

## Consecuencias
- Migración de `project.toml` (campo opcional, sin migración de DB).
- Tests con un proyecto `document` sin tests que completa una tarea y la integra.
- El motor de contexto trata los demás formatos como texto o blob; sin cambios por ahora.
- Depende de ADR-0019 para las herramientas.
