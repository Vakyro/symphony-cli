# Cómo escribir un plugin

Los plugins de Symphony (ADR-0015, `crates/plugins`) son ejecutables externos que corren en un proceso separado por cada llamada: leen **una línea JSON** de `stdin`, escriben **una línea JSON** en `stdout` y terminan.

- **Sin estado compartido ni acceso interno:** un plugin nunca abre `symphony.db` (CONSTRAINTS A10), no habla con el socket IPC del daemon y no puede pedir a Symphony que ejecute comandos arbitrarios (STACK §47).
- **Aislamiento por llamada:** `symphony-plugins` lanza cada invocación mediante `symphony-process` en su propio grupo de procesos, con límite de **512 MiB de memoria**, **16 procesos**, tiempo máximo `timeout_secs` (10 s por defecto, máximo 300 s), salida `stdout` acotada a **1 MiB**, variables de entorno con secretos (`*KEY*`, `*TOKEN*`, `*SECRET*`, `*PASSWORD*`, `*AUTH*`) vaciadas y redacción automática de secretos sobre cualquier texto devuelto.
- **Modelo de confianza:** el aislamiento protege a `symphonyd` frente a cuelgues o fallos del plugin, pero el plugin corre con los permisos de tu usuario como cualquier herramienta que instales.

---

## 1. Tipos de plugin

| Tipo (`kind`) | Estado en v1 | Punto de enganche |
|---|---|---|
| `validator` | Soportado | Corre en el **tier 2** del gate de validación (`validation_checks` con nombre `plugin`). Si no puede ejecutarse o vence por timeout, el check queda `SKIPPED` (nunca se da por aprobado). |
| `context-transformer` | Soportado | Transforma el texto del paquete de handoff (`stage = "handoff"`), en el orden declarado en `project.toml`. La salida no puede quedar vacía ni superar 4× el tamaño de entrada. |
| `tool` | Reservado (v1.1) | Aceptado en el esquema de `plugin.toml`, pero el host lo rechaza en ejecución. |
| `provider-adapter` | Reservado (v1.1) | Aceptado en el esquema de `plugin.toml`, pero el host lo rechaza en ejecución. |

---

## 2. Instalación global y activación por proyecto

Siguiendo la regla de instalación única (CONSTRAINTS A12), los plugins se instalan una sola vez en el directorio del usuario y cada proyecto decide cuáles activar:

1. **Instalación global (`~/.symphony/plugins/<nombre>/`):**
   El nombre de la carpeta debe coincidir exactamente con el campo `name` de su `plugin.toml`.
2. **Activación en el proyecto (`<proyecto>/.symphony/project.toml`):**
   ```toml
   [plugins]
   validators = ["validator-todo-check"]
   context_transformers = []
   ```

Comandos de diagnóstico:

```bash
symphony plugin list
symphony plugin check validator-todo-check
```

---

## 3. El manifiesto `plugin.toml`

Cada carpeta `~/.symphony/plugins/<nombre>/` debe contener un archivo `plugin.toml` sin claves desconocidas (`deny_unknown_fields`):

```toml
name = "validator-todo-check"
version = "0.1.0"
plugin_api = 1
kind = "validator"
exec = ["./validator-todo-check"]
description = "Falla si el diff agrega un TODO sin issue (TODO(#123), TODO #123 o TODO(ABC-12))"
timeout_secs = 10
```

Reglas de validación (`crates/plugins/src/manifest.rs`):
- `name`: de 1 a 40 caracteres, solo `[a-z0-9-]` y sin empezar por `-`.
- `version`: cadena no vacía.
- `plugin_api`: debe ser `1`.
- `kind`: `"validator"` o `"context-transformer"` (en v1).
- `exec`: lista no vacía de programa y argumentos (se ejecuta sin shell). `exec[0]` debe ser un archivo relativo dentro de la carpeta del plugin (ej. `./validator-todo-check`, añadiendo `.exe` automáticamente en Windows si existe) o un binario del `PATH` (`node`, `python`), **nunca** una ruta absoluta ni con `..`.
- `timeout_secs`: entero entre `1` y `300` (por defecto `10`).

---

## 4. El protocolo JSON v1

En cada invocación, Symphony define `SYMPHONY_PLUGIN_API=1`, escribe una línea JSON terminada en `\n` en `stdin` y cierra `stdin`. El plugin debe escribir su respuesta JSON en la **última línea no vacía de `stdout`** (cualquier log de diagnóstico debe ir a `stderr`).

### Método `describe` (obligatorio en todos los plugins)

Petición:
```json
{"plugin_api":1,"kind":"validator","method":"describe","params":{}}
```

Respuesta:
```json
{"plugin_api":1,"ok":true,"result":{"name":"validator-todo-check","version":"0.1.0","plugin_api":1,"kind":"validator"}}
```

### Método `validate` (`kind = "validator"`)

Petición:
```json
{"plugin_api":1,"kind":"validator","method":"validate","params":{"task_code":"T-1","task_title":"Arreglar login","diff":"+++ b/src/a.rs\n+// TODO revisar\n","files":["src/a.rs"]}}
```

Respuesta:
```json
{"plugin_api":1,"ok":true,"result":{"passed":false,"summary":"1 TODO nuevo(s) sin issue","findings":[{"severity":"medium","file":"src/a.rs","text":"TODO sin issue: +// TODO revisar"}]}}
```

### Método `transform` (`kind = "context-transformer"`)

Petición:
```json
{"plugin_api":1,"kind":"context-transformer","method":"transform","params":{"stage":"handoff","text":"# Handoff\n..."}}
```

Respuesta:
```json
{"plugin_api":1,"ok":true,"result":{"text":"# Handoff\n..."}}
```

### Respuesta de error

Si el plugin encuentra un error controlado:
```json
{"plugin_api":1,"ok":false,"error":"motivo legible del fallo"}
```

---

## 5. Ejemplo paso a paso (`validator-todo-check`)

El repositorio incluye un plugin completo en `examples/plugins/validator-todo-check` que rechaza líneas añadidas (`+`) con un `TODO` sin referencia a un issue (`TODO(#123)`, `TODO #123` o `TODO(ABC-12)`).

Esqueleto mínimo en Rust (`src/main.rs`):

```rust
use std::io::{BufRead, Write};
use serde_json::{Value, json};

fn respond(result: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", json!({"plugin_api": 1, "ok": true, "result": result}));
}

fn fail(message: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", json!({"plugin_api": 1, "ok": false, "error": message}));
}

fn main() {
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        fail("no se pudo leer la petición");
        return;
    }
    let Ok(req) = serde_json::from_str::<Value>(line.trim()) else {
        fail("la petición no es JSON");
        return;
    };
    match req["method"].as_str() {
        Some("describe") => respond(&json!({
            "name": "validator-todo-check",
            "version": "0.1.0",
            "plugin_api": 1,
            "kind": "validator",
        })),
        Some("validate") => {
            let diff = req["params"]["diff"].as_str().unwrap_or("");
            let bad = diff.lines().any(|l| l.starts_with('+') && l.contains("TODO "));
            respond(&json!({
                "passed": !bad,
                "summary": if bad { "TODO nuevo sin issue" } else { "ok" },
                "findings": [],
            }));
        }
        other => fail(&format!("método no soportado: {other:?}")),
    }
}
```

Compilar, instalar y verificar el ejemplo incluido en el repositorio:

```bash
cargo build -p validator-todo-check --release
mkdir -p ~/.symphony/plugins/validator-todo-check
cp examples/plugins/validator-todo-check/plugin.toml ~/.symphony/plugins/validator-todo-check/
cp target/release/validator-todo-check* ~/.symphony/plugins/validator-todo-check/

symphony plugin list
symphony plugin check validator-todo-check
```
