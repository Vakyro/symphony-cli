Tarea: implementa un módulo de usuarios en este repo (Node ESM, sin dependencias externas, tests con node:test; corre los tests con `npm test`).

1. src/email.js: `normalizeEmail(s)` (trim + minúsculas) y `validateEmail(s)` que devuelve `{ ok: boolean, reason?: string }`. Rechaza: vacío, sin @, más de un @, dominio sin punto, espacios internos, local-part de más de 64 caracteres.
2. src/password.js: `validatePassword(p)` (mínimo 10 caracteres, al menos una mayúscula, una minúscula, un dígito; devuelve `{ ok, reasons: string[] }`), `hashPassword(p)` y `verifyPassword(p, hash)` usando `crypto.scrypt` con salt aleatorio (formato `salt:hash` en hex).
3. src/users.js: clase `UserStore` en memoria con `register(email, password)` (valida ambos, normaliza el email, rechaza duplicados sin importar mayúsculas, guarda solo el hash, devuelve `{ id, email }`) y `login(email, password)` que devuelve el usuario o lanza un error `InvalidCredentials`.
4. Tests en test/email.test.js, test/password.test.js y test/users.test.js cubriendo cada regla y cada caso de error.
5. Exporta todo desde src/index.js sin romper el test de smoke.

Planea primero con una lista de TODOs y ve marcándolos. Trabaja archivo por archivo y corre los tests después de cada uno. Al final, todos los tests deben pasar.
