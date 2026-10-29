# Instalación

## Requisitos previos

- **Rust 1.95+** (Edition 2024, MSRV 1.95 según ADR-0001) y **Git** instalados.
- **Windows, macOS o Linux.**
  - En **Windows**, Rust necesita el workload **«Desarrollo para el escritorio con C++»** de Visual Studio (`link.exe` y Windows SDK). Tener Visual Studio instalado sin ese workload hace fallar la compilación con `linker link.exe not found`.
- Al menos uno de estos CLIs de proveedores instalado y autenticado por tu cuenta:
  - **Claude Code** (`claude`)
  - **Codex** (`codex`)
  - **Kimi Code** (`kimi`)
  - **Antigravity** (`agy`)
  - **GitHub Copilot CLI** (`copilot`)
  - **OpenCode** (`opencode`)

Comprueba las herramientas instaladas:

```bash
rustc --version
git --version
claude --version    # si lo tienes
codex --version     # si lo tienes
kimi --version      # si lo tienes
agy --version       # si lo tienes
copilot --version   # si lo tienes
opencode --version  # si lo tienes
```

## Obtener Symphony

### Opción A (recomendada): instalación global

Instalar globalmente permite abrir `symphony` desde cualquier carpeta de proyecto:

```bash
git clone https://github.com/Vakyro/symphony-cli.git
cd symphony-cli

# Instala `symphony` y `symphonyd` en ~/.cargo/bin (toma varios minutos la primera vez)
cargo install --path crates/cli --locked
cargo install --path crates/daemon --locked

# Comprueba desde otra carpeta cualquiera:
cd ~/mi-proyecto
symphony --version
```

`symphony` busca el ejecutable `symphonyd` en la misma carpeta donde reside el propio binario `symphony`. Al instalar ambos con `cargo install`, quedan juntos en `~/.cargo/bin` (que `rustup` agrega al `PATH`).

#### Cómo actualizar una instalación existente

Repite los dos comandos `cargo install` añadiendo `--force`. En **Windows**, detén primero el daemon: si `symphonyd.exe` sigue corriendo en segundo plano, el instalador fallará con «Acceso denegado (os error 5)» porque el archivo está en uso:

```bash
symphony daemon stop
cargo install --path crates/cli --locked --force
cargo install --path crates/daemon --locked --force
```

#### Si `symphony --version` sigue mostrando una versión vieja

Significa que hay otra copia de `symphony` antes en tu `PATH` (por ejemplo, en `%APPDATA%\npm`). Búscalas todas:

- En **PowerShell**:
  ```powershell
  Get-Command symphony -All
  ```
- En **bash / zsh**:
  ```bash
  which -a symphony
  ```

Borra la copia antigua o coloca `~/.cargo/bin` primero en el `PATH`. Recuerda que `symphony` y `symphonyd` deben provenir siempre de la misma carpeta.

### Opción B: solo compilar en modo release

```bash
cargo build -p symphony-cli -p symphony-daemon --release
```

Los binarios quedan en `target/release/symphony` y `target/release/symphonyd` (con extensión `.exe` en Windows). Si quieres usarlos desde fuera del repositorio de Symphony, agrega esa carpeta al `PATH` o cópialos juntos a un directorio de tu `PATH`.

### Opción C: ejecutar sin instalar (solo dentro del repositorio de Symphony)

```bash
cd symphony-cli
cargo run -p symphony-cli -- --version
```

> Nota: `cargo run -p symphony-cli --` busca `Cargo.toml` en la carpeta actual o sus padres, por lo que **no** sirve para abrir Symphony directamente dentro de otro repositorio externo.

## Qué se instala y qué no

- **Una sola instalación global por computadora:** instalas `symphony` y `symphonyd` una vez, y autenticas los CLIs de los proveedores una vez.
- **Nada se instala a mano por proyecto:** no tienes que copiar binarios, hooks, skills ni configurar el servidor MCP manualmente en cada repositorio. Symphony los inyecta en cada ejecución del CLI del proveedor sin ensuciar los archivos versionados de tu proyecto.

## Dónde guarda Symphony sus datos (ADR-0018)

Symphony separa los datos de cada proyecto de la configuración personal del usuario:

1. **En cada proyecto (`<proyecto>/.symphony/`):**
   - `project.toml`: configuración del proyecto (lo único versionable si lo deseas).
   - `symphony.db`: base de datos SQLite exclusiva del proyecto.
   - `objects/`: almacén de objetos comprimidos (BLAKE3 + zstd) del motor de contexto.
   - `worktrees/`: directorios de trabajo aislados de cada agente y del chat (`symphony/chat`).
   - `logs/`: registros de `symphonyd` de ese proyecto.
   - `run/`: socket/pipe y archivo de bloqueo del daemon del proyecto.
   - Un `.gitignore` automático dentro de `.symphony/` deja fuera de Git todo excepto `project.toml`.

2. **En el directorio del usuario (`~/.symphony/` o `SYMPHONY_USER_HOME`):**
   - `config.toml`: configuración global, límites de rendimiento, políticas de routing y permisos de proveedores.
   - `skills/` y `mcp.toml`: skills y servidores MCP compartidos del usuario.
   - `plugins/`: plugins instalados globalmente (`~/.symphony/plugins/<nombre>/plugin.toml`).
   - `learning.db`: base de datos con el desempeño aprendido por `(profile, modelo)` compartido entre proyectos (ADR-0016).

3. **Ubicación del proyecto y `SYMPHONY_HOME`:**
   - Al ejecutar `symphony`, el cliente sube desde el directorio actual hasta encontrar el primer `.symphony/project.toml` o `.git` (y si estás dentro de `<proyecto>/.symphony/worktrees/…`, resuelve al proyecto dueño).
   - **No abras Symphony directamente en tu carpeta personal (`~`):** como `~/.symphony` es el directorio global del usuario, Symphony rechazará usar tu carpeta personal como proyecto. Entra primero a la carpeta de tu proyecto (`cd /ruta/a/tu/proyecto`).
   - La variable de entorno `SYMPHONY_HOME` fuerza un único directorio para todo (proyecto y usuario), útil en pruebas automatizadas.

## Migrar desde una instalación anterior a P15.5

Si usabas Symphony antes de P15.5 (cuando existía un único daemon global y una sola base en `~/.symphony/symphony.db`), ejecuta una vez:

```bash
symphony migrate split
```

Este comando detiene el daemon global antiguo y reparte las filas de `~/.symphony/symphony.db`, los objetos y los worktrees hacia la carpeta `<proyecto>/.symphony/` de cada proyecto registrado. No borra la base global antigua; queda como respaldo hasta que decidas eliminarla.
