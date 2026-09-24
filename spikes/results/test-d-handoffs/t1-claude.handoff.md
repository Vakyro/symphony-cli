Retomas una tarea que otro agente de código dejó a medias (su proceso murió sin aviso). Ya estás en su worktree. No empieces de cero: revisa lo que ya existe y continúa desde donde quedó.

## Objetivo original
Tarea: implementa un módulo de usuarios en este repo (Node ESM, sin dependencias externas, tests con node:test; corre los tests con `npm test`).

1. src/email.js: `normalizeEmail(s)` (trim + minúsculas) y `validateEmail(s)` que devuelve `{ ok: boolean, reason?: string }`. Rechaza: vacío, sin @, más de un @, dominio sin punto, espacios internos, local-part de más de 64 caracteres.
2. src/password.js: `validatePassword(p)` (mínimo 10 caracteres, al menos una mayúscula, una minúscula, un dígito; devuelve `{ ok, reasons: string[] }`), `hashPassword(p)` y `verifyPassword(p, hash)` usando `crypto.scrypt` con salt aleatorio (formato `salt:hash` en hex).
3. src/users.js: clase `UserStore` en memoria con `register(email, password)` (valida ambos, normaliza el email, rechaza duplicados sin importar mayúsculas, guarda solo el hash, devuelve `{ id, email }`) y `login(email, password)` que devuelve el usuario o lanza un error `InvalidCredentials`.
4. Tests en test/email.test.js, test/password.test.js y test/users.test.js cubriendo cada regla y cada caso de error.
5. Exporta todo desde src/index.js sin romper el test de smoke.

Planea primero con una lista de TODOs y ve marcándolos. Trabaja archivo por archivo y corre los tests después de cada uno. Al final, todos los tests deben pasar.

## Plan del agente anterior ([x] hecho, [~] en curso, [ ] pendiente)
(el agente no dejó una lista estructurada)

## Último mensaje del agente anterior
(ninguno)

## Último comando que corrió y su resultado
$ ls; cat package.json; ls src test; cat src/index.js test/*.js
{"interrupted":false,"isImage":false,"noOutputExpected":false,"stderr":"","stdout":"package.json\nsrc/\ntest/\n{\r\n  \"name\": \"d-repo\",\r\n  \"version\": \"1.0.0\",\r\n  \"type\": \"module\",\r\n  \"private\": true,\r\n  \"scripts\": { \"test\": \"node --test\" }\r\n}\r\nsrc:\nindex.js\n\ntest:\nslow.test.js\nsmoke.test.js\nexport {};\r\n// Simula una suite lenta (Test D: kill durante los tests).\r\nimport { test } from \"node:test\";\r\n\r\ntest(\"slow integration suite\", async () => {\r\n  await new Promise((r) => setTimeout(r, 8000));\r\n});\r\nimport { test } from \"node:test\";\r\nimport assert from \"node:assert/strict\";\r\nimport * as index from \"../src/index.js\";\r\n\r\ntest(\"index loads\", () => {\r\n  assert.equal(typeof index, \"object\");\r\n});"}

## Estado de git
?? src/email.js
?? test/email.test.js


## Diff de archivos modificados
(ninguno)

## Archivos nuevos (todavía sin commit)
--- src/email.js (archivo nuevo)
export const normalizeEmail = (s) => String(s).trim().toLowerCase();

export function validateEmail(s) {
  if (typeof s !== "string" || s.trim() === "") return { ok: false, reason: "empty" };
  if (/\s/.test(s.trim())) return { ok: false, reason: "whitespace" };
  const parts = s.trim().split("@");
  if (parts.length < 2) return { ok: false, reason: "missing @" };
  if (parts.length > 2) return { ok: false, reason: "multiple @" };
  const [local, domain] = parts;
  if (!local) return { ok: false, reason: "empty local part" };
  if (local.length > 64) return { ok: false, reason: "local part too long" };
  if (!/^[^.]+(\.[^.]+)+$/.test(domain)) return { ok: false, reason: "invalid domain" };
  return { ok: true };
}


--- test/email.test.js (archivo nuevo)
import { test } from "node:test";
import assert from "node:assert/strict";
import { normalizeEmail, validateEmail } from "../src/email.js";

test("normalizeEmail trims and lowercases", () => {
  assert.equal(normalizeEmail("  Foo@Bar.COM "), "foo@bar.com");
});

test("validateEmail accepts a valid email", () => {
  assert.deepEqual(validateEmail("a@b.co"), { ok: true });
  assert.equal(validateEmail("a".repeat(64) + "@b.co").ok, true);
});

test("validateEmail rejects invalid emails", () => {
  const bad = {
    "": "empty",
    "   ": "empty",
    "ab.com": "missing @",
    "a@b@c.com": "multiple @",
    "a@localhost": "invalid domain",
    "a@.com": "invalid domain",
    "a@b.": "invalid domain",
    "a b@c.com": "whitespace",
    "a@b .com": "whitespace",
    "@b.com": "empty local part",
  };
  for (const [input, reason] of Object.entries(bad)) {
    assert.deepEqual(validateEmail(input), { ok: false, reason }, JSON.stringify(input));
  }
  assert.deepEqual(validateEmail("a".repeat(65) + "@b.co"), { ok: false, reason: "local part too long" });
  assert.equal(validateEmail(undefined).ok, false);
});


Termina la tarea. Al final, todos los tests (`npm test`) deben pasar. No hagas commit.
