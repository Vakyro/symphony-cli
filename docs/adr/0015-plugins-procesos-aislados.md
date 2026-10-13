# ADR-0015 · Plugins: procesos aislados con un protocolo JSON de una vuelta

- **Estado:** ACEPTADO
- **Fecha:** 2026-10-12
- **Autor:** agente (claude-code/sonnet-5.5) · **Aprobado por:** Leo (delegó el orden y el alcance en ADR-0014; las decisiones de este ADR no cambian comportamiento existente)
- **Fase/paso:** P12.S1

## Contexto
STACK §17 pide plugins de terceros como **procesos externos** con `plugin.toml` y un protocolo JSON versionado, nunca bibliotecas dentro de `symphonyd`; §46 exige `plugin_api` declarado y rechazar lo desconocido; §47 prohíbe ejecutar un comando recibido de un plugin sin pasar por las políticas. IDEA §5.12 lista los tipos: validators, context transformers, tools y provider adapters. CONSTRAINTS A12 pide que nada se instale por proyecto.

## Opciones consideradas
1. **Servidor persistente por plugin** (socket local, varias peticiones). Más rápido por llamada, pero hay que gestionar su vida, reinicios y fugas.
2. **Un proceso por llamada** (stdin → stdout, una línea JSON cada uno). Más lento por llamada (arranque), pero sin estado, sin fugas y con los límites del supervisor en cada ejecución.
3. **WASM** (STACK §17 «alternativa futura»): descartada en v1.

## Decisión
Opción 2.

**Instalación y activación.** Un plugin vive en `~/.symphony/plugins/<nombre>/` (instalación global; A12). Un proyecto no instala nada: solo **activa** plugins ya instalados con `[plugins]` en su `project.toml`:

```toml
[plugins]
validators = ["validator-todo-check"]      # corren en el tier 2 de la validación
context_transformers = []                   # se aplican al handoff
```

**Manifiesto (`plugin.toml`).**

```toml
name = "validator-todo-check"      # [a-z0-9-], 1–40
version = "0.1.0"
plugin_api = 1                      # solo se acepta 1
kind = "validator"                  # validator | context-transformer | tool | provider-adapter
exec = ["./validator-todo-check"]   # programa + argumentos, sin shell
description = "…"                   # opcional
timeout_secs = 10                   # opcional, 1–300 (por defecto 10)
```
`exec[0]` es un archivo relativo dentro de la carpeta del plugin (sin `..`; en Windows se prueba `.exe`) o un nombre simple del `PATH` (`node`, `python`). Rutas absolutas, `..` y un manifiesto con claves desconocidas se rechazan.

**Protocolo v1.** Una petición y una respuesta, una línea JSON cada una, en UTF-8:

```json
{"plugin_api":1,"kind":"validator","method":"validate","params":{…}}
{"plugin_api":1,"ok":true,"result":{…}}      // o {"plugin_api":1,"ok":false,"error":"…"}
```
- stdout es **solo** la respuesta (la última línea no vacía); los registros van a stderr (se conservan los últimos 2 KiB para el diagnóstico).
- Métodos: `describe` (todos los tipos: devuelve `name`, `version`, `plugin_api`, `kind`; el host verifica que coincida con el manifiesto), `validate` (validator), `transform` (context-transformer).
- Una respuesta con `plugin_api` distinta, JSON inválido, sin la línea o de más de 1 MiB es un error del plugin, nunca se interpreta «a medias».

**Qué puede y qué no puede un plugin.**
- Corre como proceso hijo del supervisor (`symphony-process`): grupo propio, árbol matable, tiempo máximo, memoria y procesos limitados. Un plugin que se cuelga se mata; uno que muere devuelve un error y el daemon sigue.
- Solo ve lo que se le manda en la petición. **No** abre la base de datos (A10), **no** habla por el IPC del daemon y **no** pide a Symphony que ejecute comandos: el protocolo no tiene ese mensaje (STACK §47). Lo que devuelve (resúmenes, hallazgos, texto transformado) pasa por `redact` y se recorta antes de guardarse o enviarse a un proveedor.
- **Confianza.** El usuario instaló el plugin y es código que decidió ejecutar con sus permisos, como cualquier CLI: el aislamiento protege al daemon de fallos, no es un sandbox contra un plugin malicioso. Se documenta así en el QUICKSTART.

**Alcance de v1.** Se implementan `validator` y `context-transformer` (tienen punto de enganche: el gate de validación de P08 y el ensamblador de handoff de P09). `tool` y `provider-adapter` quedan **definidos en el manifiesto y rechazados por el host** con «tipo no soportado en esta versión»; se difieren a v1.1 porque no tienen todavía un caso real ni un punto de enganche (un adapter externo exige que el core acepte proveedores dinámicos).

**Validación de un plugin.** Un check de plugin entra en `validation_checks` con el nombre `plugin` (migración 006, que amplía el CHECK de `name`; se actualiza DB §3.H). El resultado cuenta como cualquier otro check: `PASS`, `FAIL` (con `passed`/`failed` si trae hallazgos) o `SKIPPED` cuando el plugin no pudo correr (no se da por bueno).

## Evidencia
STACK §17, §36.14, §46–§47; IDEA §5.12; ADR-0002 (supervisor de procesos); ADR-0013 (gate de validación); P08.S6 (tiers).

## Consecuencias
- Crate nuevo `symphony-plugins` (manifiesto, protocolo y host); `symphony-daemon` lo usa en el gate y en el handoff.
- `core`: `CheckName::Plugin` + `[plugins]` en `ProjectConfig`; **migración 006** (la de sugerencias de P14 pasa a ser la **007**).
- CLI: `symphony plugin list` y `symphony plugin check <nombre>`.
- Un plugin ejecuta código arbitrario del usuario: la guía de seguridad de P13.S1 lo revisa y P13.S2 hace fuzzing del parser de protocolo y manifiesto.
