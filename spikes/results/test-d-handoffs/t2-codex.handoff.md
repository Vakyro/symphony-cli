Retomas una tarea que otro agente de código dejó a medias (su proceso murió sin aviso). Ya estás en su worktree. No empieces de cero: revisa lo que ya existe y continúa desde donde quedó.

## Objetivo original
Tarea: implementa un conversor de Markdown a HTML en este repo (Node ESM, sin dependencias externas, tests con node:test; corre los tests con `npm test`).

1. src/escape.js: `escapeHtml(s)` que escapa `& < > " '`.
2. src/inline.js: `renderInline(s)` que convierte `**negrita**` → `<strong>`, `*cursiva*` → `<em>`, `` `código` `` → `<code>` (sin procesar markdown dentro del código) y `[texto](url)` → `<a href="url">texto</a>`. Todo el texto se escapa con `escapeHtml` antes.
3. src/slug.js: `slugify(s)` (minúsculas, sin acentos, espacios → `-`, solo `[a-z0-9-]`) y `createSlugger()` que devuelve una función que deduplica: `intro`, `intro-1`, `intro-2`.
4. src/markdown.js: `toHtml(md)` con encabezados `#` a `###` (con `id` generado por el slugger), párrafos separados por línea en blanco, listas no ordenadas con `- `, bloques de código con ``` (contenido escapado, sin inline) y el resto con `renderInline`.
5. Tests en test/escape.test.js, test/inline.test.js, test/slug.test.js y test/markdown.test.js cubriendo cada regla.
6. Exporta `toHtml` y `slugify` desde src/index.js sin romper los tests existentes.

Planea primero con una lista de TODOs y ve marcándolos. Trabaja archivo por archivo y corre los tests después de cada uno. Al final, todos los tests deben pasar.

## Plan del agente anterior ([x] hecho, [~] en curso, [ ] pendiente)
(el agente no dejó una lista estructurada)

## Último mensaje del agente anterior
TODO inicial:

- [ ] `escapeHtml` + pruebas; `npm test`
- [ ] `renderInline` + pruebas, preservando código literal; `npm test`
- [ ] `slugify`/`createSlugger` + pruebas; `npm test`
- [ ] `toHtml` + pruebas de bloques; `npm test`
- [ ] exports públicos en `src/index.js` sin romper smoke tests; `npm test`
- [ ] revisar diff final

El repo está limpio, ya usa ESM y `node:test`; no hace falta agregar paquetes ni configuración. La suite existente incluye una prueba intencionalmente lenta (~8 s), así que cada ciclo tardará eso.

## Último comando que corrió y su resultado
$ npm test

> d-repo@1.0.0 test
> node --test

TAP version 13
# node:internal/modules/esm/resolve:257
#     throw new ERR_MODULE_NOT_FOUND(
#           ^
# Error [ERR_MODULE_NOT_FOUND]: Cannot find module 'C:\\Users\\Latitude 7390\\symphony-spike\\d-t2-codex\\src\\escape.js' imported from C:\\Users\\Latitude 7390\\symphony-spike\\d-t2-codex\\test\\escape.test.js
#     at finalizeResolution (node:internal/modules/esm/resolve:257:11)
#     at moduleResolve (node:internal/modules/esm/resolve:913:10)
#     at defaultResolve (node:internal/modules/esm/resolve:1037:11)
#     at ModuleLoader.defaultResolve (node:internal/modules/esm/loader:650:12)
#     at \#cachedDefaultResolve (node:internal/modules/esm/loader:599:25)
#     at ModuleLoader.resolve (node:internal/modules/esm/loader:582:38)
#     at ModuleLoader.getModuleJobForImport (node:internal/modules/esm/loader:241:38)
#     at ModuleJob._link (node:internal/modules/esm/module_job:132:49) {
#   code: 'ERR_MODULE_NOT_FOUND',
#   url: 'file:///C:/Users/Latitude%207390/symphony-spike/d-t2-codex/src/escape.js'
# }
# Node.js v22.11.0
# Subtest: test\\escape.test.js
not ok 1 - test\\escape.test.js
  ---
  duration_ms: 217.9577
  location: 'C:\\Users\\Latitude 7390\\symphony-spike\\d-t2-codex\\test\\escape.test.js:1:1'
  failureType: 'testCodeFailure'
  exitCode: 1
  signal: ~
  error: 'test failed'
  code: 'ERR_TEST_FAILURE'
  ...
# Subtest: slow integration suite
ok 2 - slow integration suite
  ---
  duration_ms: 8004.5324
  ...
# Subtest: index loads
ok 3 - index loads
  ---
  duration_ms: 3.4879
  ...
1..3
# tests 3
# suites 0
# pass 2
# fail 1
# cancelled 0
# skipped 0
# todo 0
# duration_ms 8332.1579


## Estado de git
?? src/escape.js
?? test/escape.test.js


## Diff de archivos modificados
(ninguno)

## Archivos nuevos (todavía sin commit)
--- src/escape.js (archivo nuevo)
const ENTITIES = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
  "'": "&#39;",
};

export function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (character) => ENTITIES[character]);
}


--- test/escape.test.js (archivo nuevo)
import assert from "node:assert/strict";
import { test } from "node:test";

import { escapeHtml } from "../src/escape.js";

test("escapeHtml escapes HTML-sensitive characters", () => {
  assert.equal(
    escapeHtml(`&<>"'`),
    "&amp;&lt;&gt;&quot;&#39;",
  );
});

test("escapeHtml leaves ordinary text unchanged", () => {
  assert.equal(escapeHtml("hola mundo"), "hola mundo");
});


Termina la tarea. Al final, todos los tests (`npm test`) deben pasar. No hagas commit.
