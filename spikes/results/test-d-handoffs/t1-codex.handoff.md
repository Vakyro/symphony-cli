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
Inspección terminada: repo ESM mínimo, `npm test` usa `node --test` y hay un smoke test que solo exige que `src/index.js` cargue. El árbol está limpio; la suite incluye además una prueba deliberadamente lenta de ~8 s.

- [x] Inspeccionar estructura, scripts y smoke test.
- [ ] Implementar y probar `src/email.js`.
- [ ] Implementar y probar `src/password.js`.
- [ ] Implementar y probar `src/users.js`.
- [ ] Actualizar `src/index.js` y verificar smoke.
- [ ] Ejecutar `npm test` completo.

## Último comando que corrió y su resultado
$ npm test

> d-repo@1.0.0 test
> node --test

TAP version 13
# node:internal/modules/esm/resolve:257
#     throw new ERR_MODULE_NOT_FOUND(
#           ^
# Error [ERR_MODULE_NOT_FOUND]: Cannot find module 'C:\\Users\\Latitude 7390\\symphony-spike\\d-t1-codex\\src\\email.js' imported from C:\\Users\\Latitude 7390\\symphony-spike\\d-t1-codex\\test\\email.test.js
#     at finalizeResolution (node:internal/modules/esm/resolve:257:11)
#     at moduleResolve (node:internal/modules/esm/resolve:913:10)
#     at defaultResolve (node:internal/modules/esm/resolve:1037:11)
#     at ModuleLoader.defaultResolve (node:internal/modules/esm/loader:650:12)
#     at \#cachedDefaultResolve (node:internal/modules/esm/loader:599:25)
#     at ModuleLoader.resolve (node:internal/modules/esm/loader:582:38)
#     at ModuleLoader.getModuleJobForImport (node:internal/modules/esm/loader:241:38)
#     at ModuleJob._link (node:internal/modules/esm/module_job:132:49) {
#   code: 'ERR_MODULE_NOT_FOUND',
#   url: 'file:///C:/Users/Latitude%207390/symphony-spike/d-t1-codex/src/email.js'
# }
# Node.js v22.11.0
# Subtest: test\\email.test.js
not ok 1 - test\\email.test.js
  ---
  duration_ms: 121.1683
  location: 'C:\\Users\\Latitude 7390\\symphony-spike\\d-t1-codex\\test\\email.test.js:1:1'
  failureType: 'testCodeFailure'
  exitCode: 1
  signal: ~
  error: 'test failed'
  code: 'ERR_TEST_FAILURE'
  ...
# Subtest: slow integration suite
ok 2 - slow integration suite
  ---
  duration_ms: 8014.9106
  ...
# Subtest: index loads
ok 3 - index loads
  ---
  duration_ms: 1.2722
  ...
1..3
# tests 3
# suites 0
# pass 2
# fail 1
# cancelled 0
# skipped 0
# todo 0
# duration_ms 8182.6195


## Estado de git
?? src/email.js
?? test/email.test.js


## Diff de archivos modificados
(ninguno)

## Archivos nuevos (todavía sin commit)
--- src/email.js (archivo nuevo)
export const normalizeEmail = (email) => email.trim().toLowerCase();

export function validateEmail(email) {
  const normalized = normalizeEmail(email);
  if (!normalized) return { ok: false, reason: "Email is required" };
  if (!normalized.includes("@")) return { ok: false, reason: "Email must contain @" };
  if (normalized.split("@").length !== 2) return { ok: false, reason: "Email must contain one @" };
  if (/\s/.test(normalized)) return { ok: false, reason: "Email must not contain spaces" };

  const [local, domain] = normalized.split("@");
  if (local.length > 64) return { ok: false, reason: "Email local part is too long" };
  if (!domain.includes(".")) return { ok: false, reason: "Email domain must contain a dot" };
  return { ok: true };
}


--- test/email.test.js (archivo nuevo)
import assert from "node:assert/strict";
import { test } from "node:test";

import { normalizeEmail, validateEmail } from "../src/email.js";

test("normalizeEmail trims and lowercases", () => {
  assert.equal(normalizeEmail("  User@Example.COM  "), "user@example.com");
});

for (const [name, email, reason] of [
  ["empty email", "   ", "Email is required"],
  ["email without @", "user.example.com", "Email must contain @"],
  ["email with more than one @", "user@@example.com", "Email must contain one @"],
  ["domain without a dot", "user@example", "Email domain must contain a dot"],
  ["email with internal spaces", "user name@example.com", "Email must not contain spaces"],
  ["local part longer than 64 characters", `${"a".repeat(65)}@example.com`, "Email local part is too long"],
]) {
  test(`validateEmail rejects ${name}`, () => {
    assert.deepEqual(validateEmail(email), { ok: false, reason });
  });
}

test("validateEmail accepts a valid email after trimming", () => {
  assert.deepEqual(validateEmail("  User@example.com  "), { ok: true });
});


Termina la tarea. Al final, todos los tests (`npm test`) deben pasar. No hagas commit.
