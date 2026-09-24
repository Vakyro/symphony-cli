Tarea: implementa un conversor de Markdown a HTML en este repo (Node ESM, sin dependencias externas, tests con node:test; corre los tests con `npm test`).

1. src/escape.js: `escapeHtml(s)` que escapa `& < > " '`.
2. src/inline.js: `renderInline(s)` que convierte `**negrita**` → `<strong>`, `*cursiva*` → `<em>`, `` `código` `` → `<code>` (sin procesar markdown dentro del código) y `[texto](url)` → `<a href="url">texto</a>`. Todo el texto se escapa con `escapeHtml` antes.
3. src/slug.js: `slugify(s)` (minúsculas, sin acentos, espacios → `-`, solo `[a-z0-9-]`) y `createSlugger()` que devuelve una función que deduplica: `intro`, `intro-1`, `intro-2`.
4. src/markdown.js: `toHtml(md)` con encabezados `#` a `###` (con `id` generado por el slugger), párrafos separados por línea en blanco, listas no ordenadas con `- `, bloques de código con ``` (contenido escapado, sin inline) y el resto con `renderInline`.
5. Tests en test/escape.test.js, test/inline.test.js, test/slug.test.js y test/markdown.test.js cubriendo cada regla.
6. Exporta `toHtml` y `slugify` desde src/index.js sin romper los tests existentes.

Planea primero con una lista de TODOs y ve marcándolos. Trabaja archivo por archivo y corre los tests después de cada uno. Al final, todos los tests deben pasar.
