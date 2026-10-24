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

## Enmienda (2026-10-23, P15.5.S5)
Hecho: `[project] kind` en `project.toml` (code por defecto; los proyectos de antes siguen como código), `symphony init --kind … [--git-init]` (y `kind`/`git_init` en `project.init`), `Repo::init` con commit inicial vacío (los worktrees lo necesitan), migración 008 con los profiles `@write`, `@media` y `@research` (puntajes de arranque derivados de lo que cada modelo ya hace bien) y prompts sin «código». Un proyecto `document`, `media` o `general` arranca con el profile de su tipo cuando el usuario no cambió `routing.default_profile`.
Recortado, a propósito, con su razón:
- **Validación propia por tipo:** no se añadió un check «el archivo existe y se abre». Un check nuevo es un nombre nuevo en `validation_checks`, o sea migración y ADR del núcleo (como pasó con el check `plugin`, ADR-0015). Sin `[validation]` el proyecto no tiene gate (ya era así), y los validators de plugin cubren los formatos que importen.
- **Tipo por tarea:** solo por proyecto. Hasta que alguien mezcle tipos en un mismo proyecto no hace falta.
- **Binarios grandes en el almacén de objetos con un puntero en git:** sin hacer. Exige materializar el archivo real en cada worktree y entender el puntero en el merge; es un paso propio. Mientras tanto git los guarda como cualquier archivo (o `git lfs` si el proyecto lo tiene).
- Los handoffs que dicen «Retomas una tarea de código» siguen reconociéndose (la marca es el prefijo común «Retomas una tarea»).
- El profile por defecto según el tipo se lee al arrancar el daemon (cambiar `kind` o correr `symphony init` con el daemon vivo aplica tras `symphony daemon stop`), y solo cuando `routing.default_profile` es el `@code` de fábrica: quien lo fija a mano a `@code` no se distingue de quien no lo tocó.
