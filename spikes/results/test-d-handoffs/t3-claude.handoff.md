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
- [x] storage ✔

## Último comando que corrió y su resultado
$ npm test 2>&1 | grep -E "^not ok|^# (pass|fail)|Error" 
{"interrupted":false,"isImage":false,"noOutputExpected":false,"stderr":"","stdout":"# pass 11\n# fail 0"}

## Estado de git
?? src/storage.js
?? src/tasks.js
?? test/storage.test.js
?? test/tasks.test.js


## Diff de archivos modificados
(ninguno)

## Archivos nuevos (todavía sin commit)
--- src/storage.js (archivo nuevo)
import { readFile, writeFile, rename } from 'node:fs/promises';

export async function loadJson(path, fallback) {
  try {
    return JSON.parse(await readFile(path, 'utf8'));
  } catch (e) {
    if (e.code === 'ENOENT') return fallback;
    throw e;
  }
}

export async function saveJson(path, data) {
  const tmp = `${path}.tmp`;
  await writeFile(tmp, JSON.stringify(data, null, 2));
  await rename(tmp, path);
}


--- src/tasks.js (archivo nuevo)
import { loadJson, saveJson } from './storage.js';

export class TaskNotFound extends Error {
  constructor(id) {
    super(`Task not found: ${id}`);
    this.name = 'TaskNotFound';
  }
}

export function isValidDate(s) {
  if (typeof s !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(s)) return false;
  const d = new Date(`${s}T00:00:00Z`);
  return !Number.isNaN(d.getTime()) && d.toISOString().slice(0, 10) === s;
}

export class TaskList {
  constructor(filePath) {
    this.filePath = filePath;
  }

  // ponytail: reloads the file on every call; cache if it ever matters
  #load() {
    return loadJson(this.filePath, { nextId: 1, tasks: [] });
  }

  async add(title, { due } = {}) {
    if (typeof title !== 'string' || !title.trim()) throw new Error('Title must be a non-empty string');
    if (due !== undefined && !isValidDate(due)) throw new Error(`Invalid due date (expected YYYY-MM-DD): ${due}`);
    const db = await this.#load();
    const task = { id: db.nextId++, title: title.trim(), done: false, ...(due && { due }) };
    db.tasks.push(task);
    await saveJson(this.filePath, db);
    return task;
  }

  async list({ done } = {}) {
    const { tasks } = await this.#load();
    return done === undefined ? tasks : tasks.filter((t) => t.done === done);
  }

  async #update(id, fn) {
    const db = await this.#load();
    const i = db.tasks.findIndex((t) => t.id === id);
    if (i < 0) throw new TaskNotFound(id);
    const result = fn(db.tasks, i);
    await saveJson(this.filePath, db);
    return result;
  }

  complete(id) {
    return this.#update(id, (tasks, i) => (tasks[i].done = true, tasks[i]));
  }

  remove(id) {
    return this.#update(id, (tasks, i) => tasks.splice(i, 1)[0]);
  }

  async overdue(today) {
    const { tasks } = await this.#load();
    return tasks.filter((t) => !t.done && t.due && t.due < today);
  }
}


--- test/storage.test.js (archivo nuevo)
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { loadJson, saveJson } from '../src/storage.js';

test('loadJson returns fallback when file is missing', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'st-'));
  assert.deepEqual(await loadJson(join(dir, 'x.json'), { a: 1 }), { a: 1 });
});

test('saveJson round-trips and leaves no .tmp file', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'st-'));
  const p = join(dir, 'x.json');
  await saveJson(p, { n: [1, 2] });
  assert.deepEqual(await loadJson(p, null), { n: [1, 2] });
  assert.deepEqual(await readdir(dir), ['x.json']);
});

test('loadJson rethrows on corrupt JSON', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'st-'));
  const p = join(dir, 'bad.json');
  await saveJson(p, 1);
  const { writeFile } = await import('node:fs/promises');
  await writeFile(p, '{oops');
  await assert.rejects(loadJson(p, null), SyntaxError);
});


--- test/tasks.test.js (archivo nuevo)
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { TaskList, TaskNotFound } from '../src/tasks.js';

const fresh = async () => new TaskList(join(await mkdtemp(join(tmpdir(), 'tk-')), 't.json'));

test('add validates title and due', async () => {
  const l = await fresh();
  await assert.rejects(l.add(''));
  await assert.rejects(l.add('   '));
  await assert.rejects(l.add('x', { due: '2025-13-01' }));
  await assert.rejects(l.add('x', { due: '2025-02-30' }));
  await assert.rejects(l.add('x', { due: 'mañana' }));
  assert.equal((await l.add('x', { due: '2024-02-29' })).due, '2024-02-29');
});

test('ids increment and are not reused after remove', async () => {
  const l = await fresh();
  const a = await l.add('a');
  const b = await l.add('b');
  await l.remove(b.id);
  const c = await l.add('c');
  assert.deepEqual([a.id, b.id, c.id], [1, 2, 3]);
});

test('persists across instances', async () => {
  const l = await fresh();
  await l.add('a');
  assert.equal((await new TaskList(l.filePath).list()).length, 1);
});

test('list filters by done', async () => {
  const l = await fresh();
  const a = await l.add('a');
  await l.add('b');
  await l.complete(a.id);
  assert.equal((await l.list()).length, 2);
  assert.deepEqual((await l.list({ done: true })).map((t) => t.title), ['a']);
  assert.deepEqual((await l.list({ done: false })).map((t) => t.title), ['b']);
});

test('complete/remove throw TaskNotFound', async () => {
  const l = await fresh();
  await assert.rejects(l.complete(9), TaskNotFound);
  await assert.rejects(l.remove(9), TaskNotFound);
});

test('overdue returns only pending tasks due before today', async () => {
  const l = await fresh();
  const a = await l.add('old', { due: '2025-01-01' });
  await l.add('done-old', { due: '2025-01-02' }).then((t) => l.complete(t.id));
  await l.add('today', { due: '2025-06-01' });
  await l.add('nodue');
  assert.deepEqual((await l.overdue('2025-06-01')).map((t) => t.id), [a.id]);
});


Termina la tarea. Al final, todos los tests (`npm test`) deben pasar. No hagas commit.
