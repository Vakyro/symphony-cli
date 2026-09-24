Retomas una tarea que otro agente de código dejó a medias (su proceso murió sin aviso). Ya estás en su worktree. No empieces de cero: revisa lo que ya existe y continúa desde donde quedó.

## Objetivo original
Tarea: implementa una librería de lista de tareas con almacenamiento en archivo en este repo (Node ESM, sin dependencias externas, tests con node:test; corre los tests con `npm test`).

1. src/storage.js: `loadJson(path, fallback)` (devuelve `fallback` si el archivo no existe) y `saveJson(path, data)` con escritura atómica: escribir a `<path>.tmp` y luego `rename`.
2. src/tasks.js: clase `TaskList` con constructor `(filePath)`, `add(title, { due } = {})` (título no vacío; `due` opcional en formato `YYYY-MM-DD` válido; ids incrementales que no se reutilizan tras borrar), `list({ done } = {})` (filtra por estado si se pasa `done`), `complete(id)`, `remove(id)` (lanzan `TaskNotFound` si no existe) y `overdue(today)` (pendientes con `due` anterior a `today`). Cada cambio se persiste con `saveJson`.
3. src/cli.js: `parseArgs(argv)` que entiende `add <título> [--due YYYY-MM-DD]`, `list [--done|--pending]`, `done <id>` y `rm <id>`, y devuelve `{ command, args, options }`, o lanza `UsageError` con un mensaje claro.
4. Tests en test/storage.test.js, test/tasks.test.js y test/cli.test.js (usa un directorio temporal con `fs.mkdtemp`).
5. Exporta `TaskList` y `parseArgs` desde src/index.js sin romper los tests existentes.

Planea primero con una lista de TODOs y ve marcándolos. Trabaja archivo por archivo y corre los tests después de cada uno. Al final, todos los tests deben pasar.

## Plan del agente anterior ([x] hecho, [~] en curso, [ ] pendiente)
(el agente no dejó una lista estructurada)

## Último mensaje del agente anterior
Repo limpio y mínimo: Node ESM, `node --test`, sin convenciones adicionales. La suite existente incluye una prueba deliberadamente lenta (~8 s), así que cada `npm test` tardará al menos eso.

Empiezo el ciclo RED de almacenamiento: prueba de fallback, serialización y ausencia del `.tmp` tras el rename atómico.

## Último comando que corrió y su resultado
$ npm test

> d-repo@1.0.0 test
> node --test

TAP version 13
# Subtest: slow integration suite
ok 1 - slow integration suite
  ---
  duration_ms: 8005.9937
  ...
# Subtest: index loads
ok 2 - index loads
  ---
  duration_ms: 1.7127
  ...
# node:internal/modules/esm/resolve:257
#     throw new ERR_MODULE_NOT_FOUND(
#           ^
# Error [ERR_MODULE_NOT_FOUND]: Cannot find module 'C:\\Users\\Latitude 7390\\symphony-spike\\d-t3-codex\\src\\storage.js' imported from C:\\Users\\Latitude 7390\\symphony-spike\\d-t3-codex\\test\\storage.test.js
#     at finalizeResolution (node:internal/modules/esm/resolve:257:11)
#     at moduleResolve (node:internal/modules/esm/resolve:913:10)
#     at defaultResolve (node:internal/modules/esm/resolve:1037:11)
#     at ModuleLoader.defaultResolve (node:internal/modules/esm/loader:650:12)
#     at \#cachedDefaultResolve (node:internal/modules/esm/loader:599:25)
#     at ModuleLoader.resolve (node:internal/modules/esm/loader:582:38)
#     at ModuleLoader.getModuleJobForImport (node:internal/modules/esm/loader:241:38)
#     at ModuleJob._link (node:internal/modules/esm/module_job:132:49) {
#   code: 'ERR_MODULE_NOT_FOUND',
#   url: 'file:///C:/Users/Latitude%207390/symphony-spike/d-t3-codex/src/storage.js'
# }
# Node.js v22.11.0
# Subtest: test\\storage.test.js
not ok 3 - test\\storage.test.js
  ---
  duration_ms: 112.952
  location: 'C:\\Users\\Latitude 7390\\symphony-spike\\d-t3-codex\\test\\storage.test.js:1:1'
  failureType: 'testCodeFailure'
  exitCode: 1
  signal: ~
  error: 'test failed'
  code: 'ERR_TEST_FAILURE'
  ...
1..3
# tests 3
# suites 0
# pass 2
# fail 1
# cancelled 0
# skipped 0
# todo 0
# duration_ms 8176.6189


## Estado de git
?? src/storage.js
?? test/storage.test.js


## Diff de archivos modificados
(ninguno)

## Archivos nuevos (todavía sin commit)
--- src/storage.js (archivo nuevo)
import { readFile, rename, writeFile } from "node:fs/promises";

export async function loadJson(path, fallback) {
  try {
    return JSON.parse(await readFile(path, "utf8"));
  } catch (error) {
    if (error.code === "ENOENT") return fallback;
    throw error;
  }
}

export async function saveJson(path, data) {
  const temporaryPath = `${path}.tmp`;
  await writeFile(temporaryPath, JSON.stringify(data), "utf8");
  await rename(temporaryPath, path);
}


--- test/storage.test.js (archivo nuevo)
import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { loadJson, saveJson } from "../src/storage.js";

test("loadJson returns the fallback for a missing file", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "tasks-storage-"));
  t.after(() => rm(directory, { recursive: true, force: true }));

  const fallback = { tasks: [] };
  assert.equal(await loadJson(join(directory, "missing.json"), fallback), fallback);
});

test("saveJson atomically writes JSON that loadJson can read", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "tasks-storage-"));
  const file = join(directory, "tasks.json");
  t.after(() => rm(directory, { recursive: true, force: true }));

  await saveJson(file, { nextId: 2, tasks: [{ id: 1 }] });

  assert.deepEqual(await loadJson(file, null), {
    nextId: 2,
    tasks: [{ id: 1 }],
  });
  await assert.rejects(readFile(`${file}.tmp`), { code: "ENOENT" });
});


Termina la tarea. Al final, todos los tests (`npm test`) deben pasar. No hagas commit.
