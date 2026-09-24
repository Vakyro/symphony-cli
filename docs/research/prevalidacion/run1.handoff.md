Retomas una tarea que otro agente de código dejó a medias (su proceso murió sin aviso). Ya estás en su worktree. No empieces de cero: revisa lo que ya existe y continúa desde donde quedó.

## Objetivo original
Tarea: implementa un módulo de usuarios en este repo (Node ESM, sin dependencias externas, tests con node:test; corre los tests con `npm test`).

1. src/email.js: `normalizeEmail(s)` (trim + minúsculas) y `validateEmail(s)` que devuelve `{ ok: boolean, reason?: string }`. Rechaza: vacío, sin @, más de un @, dominio sin punto, espacios internos, local-part de más de 64 caracteres.
2. src/password.js: `validatePassword(p)` (mínimo 10 caracteres, al menos una mayúscula, una minúscula, un dígito; devuelve `{ ok, reasons: string[] }`), `hashPassword(p)` y `verifyPassword(p, hash)` usando `crypto.scrypt` con salt aleatorio (formato `salt:hash` en hex).
3. src/users.js: clase `UserStore` en memoria con `register(email, password)` (valida ambos, normaliza el email, rechaza duplicados sin importar mayúsculas, guarda solo el hash, devuelve `{ id, email }`) y `login(email, password)` que devuelve el usuario o lanza un error `InvalidCredentials`.
4. Tests en test/email.test.js, test/password.test.js y test/users.test.js cubriendo cada regla y cada caso de error.
5. Exporta todo desde src/index.js sin romper el test de smoke.

Planea primero con una lista de TODOs y ve marcándolos. Trabaja archivo por archivo y corre los tests después de cada uno. Al final, todos los tests deben pasar.

## Plan del agente anterior (del transcript; [x] hecho, [~] en curso, [ ] pendiente)
(sin lista de TODOs en el transcript)

## Último mensaje del agente anterior
Paso 1 escrito; ahora corro los tests.

## Último comando que corrió y su resultado
$ npm test 2>&1 | grep -E "^# (tests|pass|fail)|not ok"
# tests 14
# pass 14
# fail 0

## Estado de git
?? src/email.js
?? src/password.js
?? test/email.test.js


## Diff de archivos modificados
(ninguno)

## Archivos nuevos (todavía sin commit)
--- src/email.js (archivo nuevo)
export function normalizeEmail(s) {
  return String(s ?? "").trim().toLowerCase();
}

export function validateEmail(s) {
  const email = String(s ?? "").trim();
  if (!email) return { ok: false, reason: "empty" };
  if (/\s/.test(email)) return { ok: false, reason: "whitespace" };
  const parts = email.split("@");
  if (parts.length < 2) return { ok: false, reason: "missing_at" };
  if (parts.length > 2) return { ok: false, reason: "multiple_at" };
  const [local, domain] = parts;
  if (!local) return { ok: false, reason: "empty_local" };
  if (local.length > 64) return { ok: false, reason: "local_too_long" };
  if (!domain.includes(".")) return { ok: false, reason: "domain_no_dot" };
  return { ok: true };
}


--- src/password.js (archivo nuevo)
import { scrypt, randomBytes, timingSafeEqual } from "node:crypto";
import { promisify } from "node:util";

const scryptAsync = promisify(scrypt);
const KEYLEN = 64;

export function validatePassword(p) {
  const s = typeof p === "string" ? p : "";
  const reasons = [];
  if (s.length < 10) reasons.push("too_short");
  if (!/[A-Z]/.test(s)) reasons.push("no_uppercase");
  if (!/[a-z]/.test(s)) reasons.push("no_lowercase");
  if (!/[0-9]/.test(s)) reasons.push("no_digit");
  return { ok: reasons.length === 0, reasons };
}

export async function hashPassword(p) {
  const salt = randomBytes(16);
  const hash = await scryptAsync(p, salt, KEYLEN);
  return `${salt.toString("hex")}:${hash.toString("hex")}`;
}

export async function verifyPassword(p, stored) {
  const [saltHex, hashHex] = String(stored ?? "").split(":");
  if (!saltHex || !hashHex) return false;
  const expected = Buffer.from(hashHex, "hex");
  if (expected.length !== KEYLEN) return false;
  const actual = await scryptAsync(p, Buffer.from(saltHex, "hex"), KEYLEN);
  return timingSafeEqual(actual, expected);
}


--- test/email.test.js (archivo nuevo)
import { test } from "node:test";
import assert from "node:assert/strict";
import { normalizeEmail, validateEmail } from "../src/email.js";

test("normalizeEmail hace trim y minúsculas", () => {
  assert.equal(normalizeEmail("  Ana.Perez@Example.COM \n"), "ana.perez@example.com");
});

test("validateEmail acepta emails válidos", () => {
  assert.deepEqual(validateEmail("ana@example.com"), { ok: true });
  assert.deepEqual(validateEmail("  ana@example.com  "), { ok: true });
  assert.deepEqual(validateEmail("a".repeat(64) + "@example.com"), { ok: true });
});

const invalid = [
  ["", "empty"],
  ["   ", "empty"],
  [undefined, "empty"],
  ["ana.example.com", "missing_at"],
  ["ana@b@example.com", "multiple_at"],
  ["ana@example", "domain_no_dot"],
  ["ana@", "domain_no_dot"],
  ["an a@example.com", "whitespace"],
  ["ana@exa\tmple.com", "whitespace"],
  ["@example.com", "empty_local"],
  ["a".repeat(65) + "@example.com", "local_too_long"],
];

for (const [input, reason] of invalid) {
  test(`validateEmail rechaza ${JSON.stringify(input)} (${reason})`, () => {
    assert.deepEqual(validateEmail(input), { ok: false, reason });
  });
}


Termina la tarea. Al final, todos los tests (`npm test`) deben pasar. No hagas commit.
