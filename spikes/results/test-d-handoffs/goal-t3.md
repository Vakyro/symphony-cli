Tarea: implementa una librería de lista de tareas con almacenamiento en archivo en este repo (Node ESM, sin dependencias externas, tests con node:test; corre los tests con `npm test`).

1. src/storage.js: `loadJson(path, fallback)` (devuelve `fallback` si el archivo no existe) y `saveJson(path, data)` con escritura atómica: escribir a `<path>.tmp` y luego `rename`.
2. src/tasks.js: clase `TaskList` con constructor `(filePath)`, `add(title, { due } = {})` (título no vacío; `due` opcional en formato `YYYY-MM-DD` válido; ids incrementales que no se reutilizan tras borrar), `list({ done } = {})` (filtra por estado si se pasa `done`), `complete(id)`, `remove(id)` (lanzan `TaskNotFound` si no existe) y `overdue(today)` (pendientes con `due` anterior a `today`). Cada cambio se persiste con `saveJson`.
3. src/cli.js: `parseArgs(argv)` que entiende `add <título> [--due YYYY-MM-DD]`, `list [--done|--pending]`, `done <id>` y `rm <id>`, y devuelve `{ command, args, options }`, o lanza `UsageError` con un mensaje claro.
4. Tests en test/storage.test.js, test/tasks.test.js y test/cli.test.js (usa un directorio temporal con `fs.mkdtemp`).
5. Exporta `TaskList` y `parseArgs` desde src/index.js sin romper los tests existentes.

Planea primero con una lista de TODOs y ve marcándolos. Trabaja archivo por archivo y corre los tests después de cada uno. Al final, todos los tests deben pasar.
