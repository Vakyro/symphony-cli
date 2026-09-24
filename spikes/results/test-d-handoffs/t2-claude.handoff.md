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
**TODOs**
- [ ] escape.js + test
- [ ] inline.js + test
- [ ] slug.js + test
- [ ] markdown.js + test
- [ ] index.js exports, full `npm test`

## Último comando que corrió y su resultado
$ ls; cat package.json; ls src test; cat src/index.js
{"interrupted":false,"isImage":false,"noOutputExpected":false,"stderr":"","stdout":"package.json\nsrc/\ntest/\n{\r\n  \"name\": \"d-repo\",\r\n  \"version\": \"1.0.0\",\r\n  \"type\": \"module\",\r\n  \"private\": true,\r\n  \"scripts\": { \"test\": \"node --test\" }\r\n}\r\nsrc:\nindex.js\n\ntest:\nslow.test.js\nsmoke.test.js\nexport {};"}

## Estado de git
?? src/escape.js
?? test/escape.test.js


## Diff de archivos modificados
(ninguno)

## Archivos nuevos (todavía sin commit)
--- src/escape.js (archivo nuevo)
const MAP = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };

export const escapeHtml = (s) => String(s).replace(/[&<>"']/g, (c) => MAP[c]);


--- test/escape.test.js (archivo nuevo)
import test from 'node:test';
import assert from 'node:assert/strict';
import { escapeHtml } from '../src/escape.js';

test('escapes all five characters', () => {
  assert.equal(escapeHtml(`& < > " '`), '&amp; &lt; &gt; &quot; &#39;');
});

test('does not double-escape in a single pass and leaves plain text', () => {
  assert.equal(escapeHtml('a&lt;b'), 'a&amp;lt;b');
  assert.equal(escapeHtml('hola'), 'hola');
});


Termina la tarea. Al final, todos los tests (`npm test`) deben pasar. No hagas commit.
