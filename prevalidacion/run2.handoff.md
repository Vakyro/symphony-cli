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
- [x] Inspeccionar estructura, scripts y smoke
- [x] Implementar y probar email — 9/9 pasan
- [ ] Implementar y probar password
- [ ] Implementar y probar usuarios
- [ ] Exportar API y verificar suite completa

Sigo con password: `crypto.scrypt` asíncrono, salt aleatorio y comparación segura.

## Último comando que corrió y su resultado
$ "C:\\Users\\Latitude 7390\\AppData\\Local\\Microsoft\\WindowsApps\\pwsh.exe" -Command 'npm test'

> td-users@0.0.0 test
> node --test

TAP version 13
# Subtest: normalizeEmail trims and lowercases
ok 1 - normalizeEmail trims and lowercases
  ---
  duration_ms: 3.6324
  ...
# Subtest: validateEmail accepts a valid email
ok 2 - validateEmail accepts a valid email
  ---
  duration_ms: 4.1701
  ...
# Subtest: validateEmail rejects empty emails
ok 3 - validateEmail rejects empty emails
  ---
  duration_ms: 0.4579
  ...
# Subtest: validateEmail rejects emails without @
ok 4 - validateEmail rejects emails without @
  ---
  duration_ms: 0.334
  ...
# Subtest: validateEmail rejects emails with more than one @
ok 5 - validateEmail rejects emails with more than one @
  ---
  duration_ms: 0.5373
  ...
# Subtest: validateEmail rejects domains without a dot
ok 6 - validateEmail rejects domains without a dot
  ---
  duration_ms: 0.1897
  ...
# Subtest: validateEmail rejects internal spaces
ok 7 - validateEmail rejects internal spaces
  ---
  duration_ms: 0.3665
  ...
# Subtest: validateEmail rejects local parts longer than 64 characters
ok 8 - validateEmail rejects local parts longer than 64 characters
  ---
  duration_ms: 0.2569
  ...
# Subtest: smoke
ok 9 - smoke
  ---
  duration_ms: 1.6225
  ...
1..9
# tests 9
# suites 0
# pass 9
# fail 0
# cancelled 0
# skipped 0
# todo 0
# duration_ms 220.9921

(exit 0)

## Estado de git
?? src/email.js
?? test/email.test.js
?? test/password.test.js


## Diff de archivos modificados
(ninguno)

## Archivos nuevos (todavía sin commit)
--- src/email.js (archivo nuevo)
export function normalizeEmail(s) {
  return s.trim().toLowerCase();
}

export function validateEmail(s) {
  const email = normalizeEmail(s);
  if (!email) return { ok: false, reason: "empty" };
  if (/\s/.test(email)) return { ok: false, reason: "internal-space" };

  const at = email.indexOf("@");
  if (at === -1) return { ok: false, reason: "missing-at" };
  if (at !== email.lastIndexOf("@")) return { ok: false, reason: "multiple-at" };
  if (!email.slice(at + 1).includes(".")) return { ok: false, reason: "domain-without-dot" };
  if (at > 64) return { ok: false, reason: "local-too-long" };
  return { ok: true };
}


--- test/email.test.js (archivo nuevo)
import { test } from "node:test";
import assert from "node:assert/strict";
import { normalizeEmail, validateEmail } from "../src/email.js";

test("normalizeEmail trims and lowercases", () => {
  assert.equal(normalizeEmail("  Alice@Example.COM  "), "alice@example.com");
});

test("validateEmail accepts a valid email", () => {
  assert.deepEqual(validateEmail(" Alice@Example.COM "), { ok: true });
});

for (const [name, email, reason] of [
  ["empty emails", "   ", "empty"],
  ["emails without @", "alice.example.com", "missing-at"],
  ["emails with more than one @", "alice@@example.com", "multiple-at"],
  ["domains without a dot", "alice@example", "domain-without-dot"],
  ["internal spaces", "ali ce@example.com", "internal-space"],
  ["local parts longer than 64 characters", `${"a".repeat(65)}@example.com`, "local-too-long"],
]) {
  test(`validateEmail rejects ${name}`, () => {
    assert.deepEqual(validateEmail(email), { ok: false, reason });
  });
}


--- test/password.test.js (archivo nuevo)
import { test } from "node:test";
import assert from "node:assert/strict";
import { hashPassword, validatePassword, verifyPassword } from "../src/password.js";

test("validatePassword accepts a password satisfying every rule", () => {
  assert.deepEqual(validatePassword("StrongPass1"), { ok: true, reasons: [] });
});

for (const [name, password, reason] of [
  ["fewer than 10 characters", "Short1A", "minimum-length"],
  ["no uppercase letter", "lowercase1x", "uppercase-required"],
  ["no lowercase letter", "UPPERCASE1X", "lowercase-required"],
  ["no digit", "NoDigitsHere", "digit-required"],
]) {
  test(`validatePassword reports ${name}`, () => {
    const result = validatePassword(password);
    assert.equal(result.ok, false);
    assert.ok(result.reasons.includes(reason));
  });
}

test("validatePassword reports every failing rule", () => {
  assert.deepEqual(validatePassword(""), {
    ok: false,
    reasons: ["minimum-length", "uppercase-required", "lowercase-required", "digit-required"],
  });
});

test("hashPassword returns random salt:hash hex values without the password", async () => {
  const first = await hashPassword("StrongPass1");
  const second = await hashPassword("StrongPass1");

  assert.match(first, /^[0-9a-f]+:[0-9a-f]+$/);
  assert.notEqual(first, second);
  assert.equal(first.includes("StrongPass1"), false);
});

test("verifyPassword accepts the matching password", async () => {
  const hash = await hashPassword("StrongPass1");
  assert.equal(await verifyPassword("StrongPass1", hash), true);
});

test("verifyPassword rejects a wrong password", async () => {
  const hash = await hashPassword("StrongPass1");
  assert.equal(await verifyPassword("WrongPass1", hash), false);
});

test("verifyPassword rejects a malformed hash", async () => {
  assert.equal(await verifyPassword("StrongPass1", "not-a-hash"), false);
});


Termina la tarea. Al final, todos los tests (`npm test`) deben pasar. No hagas commit.
