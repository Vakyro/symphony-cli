# Solución de problemas

Esta guía reúne los problemas reales registrados durante el desarrollo y uso de Symphony en `docs/progress/LEARNINGS.md`, organizados con el formato **Síntoma → Causa → Arreglo**.

---

## Instalación, compilación y entorno

### 1. `cargo build` o `cargo run` falla en Windows con `linker link.exe not found`
- **Síntoma:** Aunque tienes Visual Studio 2022 instalado en Windows, compilar falla con el error `linker link.exe not found`.
- **Causa:** Tener el IDE de Visual Studio instalado no basta; Rust en Windows (toolchain MSVC) requiere el enlazador `link.exe` y el Windows SDK.
- **Arreglo:** Abre *Visual Studio Installer*, elige *Modificar* sobre tu instalación y activa el workload **«Desarrollo para el escritorio con C++»**.

### 2. «Acceso denegado (os error 5)» al actualizar con `cargo install --force` o al recompilar `symphonyd.exe` en Windows
- **Síntoma:** En Windows, `cargo install --path crates/daemon --locked --force` o `cargo build` falla con «Acceso denegado (os error 5)» sobre `symphonyd.exe` (o con `link.exe` error `1104` sobre ejecutables de tests).
- **Causa:** El daemon `symphonyd` sigue corriendo en segundo plano al cerrar la TUI (por diseño), o quedó un proceso `symphonyd.exe` / `cargo-nextest.exe` huérfano tras interrumpir una prueba, y Windows bloquea la escritura sobre un `.exe` en uso.
- **Arreglo:** Detén primero el daemon del proyecto con:
  ```bash
  symphony daemon stop
  ```
  Si el bloqueo viene de un proceso huérfano de pruebas, localízalo y ciérralo:
  ```bash
  tasklist | grep symphonyd
  tasklist | grep -iE "nextest|bus-|runtime-"
  taskkill //F //PID <pid>
  ```
  *(En la suite de tests del repositorio esto se previene con `KillOnDrop`, `DaemonGuard` y `symphony_testkit::pinned_bin`, que copia el binario a `target/test-bins/`).*

### 3. `symphony --version` muestra una versión vieja o `PATH` roto en Git Bash
- **Síntoma:** Después de instalar una versión nueva con `cargo install`, `symphony --version` sigue mostrando una versión anterior, o en Git Bash `which symphony` apunta a otro directorio.
- **Causa:** Hay una copia vieja de `symphony.exe` / `symphonyd.exe` antes que `~/.cargo/bin` en tu `PATH` (por ejemplo, en `%APPDATA%\npm`), o en Git Bash se exportó una ruta con letra de unidad y dos puntos (`PATH="C:/…:$PATH"`), donde el carácter `:` parte la ruta.
- **Arreglo:**
  - Lista todas las copias en el `PATH` con `Get-Command symphony -All` (PowerShell) o `which -a symphony` (bash) y borra las copias antiguas para que `symphony` y `symphonyd` salgan juntos de `~/.cargo/bin`.
  - Para modificar el `PATH` en Git Bash usa formato `/c/ruta/bin:$PATH` (`export PATH="$HOME/.cargo/bin:$PATH"`), y en PowerShell usa `$env:PATH = "C:\ruta\bin;$env:PATH"`.

### 4. Diffs de fin de línea (`CRLF` vs `LF`) en Windows con `core.autocrlf=true`
- **Síntoma:** `cargo fmt --check` o la CI marcan diferencias en archivos sin cambios reales en máquinas Windows con `git config core.autocrlf=true`.
- **Causa:** Git convierte los saltos de línea a `CRLF` al descargar el árbol si el repositorio no fuerza `LF`.
- **Arreglo:** El repositorio incluye `.gitattributes` con `eol=lf` para normalizar los finales de línea en todos los sistemas operativos.

### 5. `python -` en Git Bash de Windows abre la Microsoft Store y se cuelga
- **Síntoma:** Al ejecutar `python -` dentro de un script de shell en Git Bash en Windows, se abre la Microsoft Store y el proceso queda colgado.
- **Causa:** El alias de ejecución de aplicaciones de Windows redirige `python` al stub de la tienda cuando no hay un intérprete antes en el `PATH` de Bash.
- **Arreglo:** No uses `python -` en scripts de shell en esa máquina (o desactiva el alias de la Microsoft Store en la configuración de Windows).

---

## Daemon, procesos y sistema operativo

### 6. Lanzar un CLI o el daemon en Windows bloquea al proceso padre (H4)
- **Síntoma:** Al lanzar un subproceso desde Git Bash o cuando `symphony` autoarranca `symphonyd`, el proceso padre (por ejemplo, `trycmd` o el propio cliente) se queda esperando indefinidamente.
- **Causa:** En Windows el proceso hijo hereda los descriptores de los pipes de `stdout`/`stderr` del padre y mantiene el pipe abierto hasta que el hijo termina.
- **Arreglo:** En `crates/cli/src/client.rs` se limpia `HANDLE_FLAG_INHERIT` mediante `SetHandleInformation` sobre los descriptores estándar propios antes de hacer `spawn` del daemon (y en scripts de PowerShell se usa `Start-Process -WindowStyle Hidden` con redirección a archivo).

### 7. Crear un agente o arrancar el daemon tarda más de 10 segundos en Windows
- **Síntoma:** `daemon start` tarda más de 10 segundos y `agent.create` falla por timeout en Windows.
- **Causa:** Lanzar el daemon con el flag `DETACHED_PROCESS` en Windows anula `CREATE_NO_WINDOW`, provocando que cada programa de consola que lanza el daemon (`git`, CLIs) espere ~3 segundos mientras Windows le crea una consola.
- **Arreglo:** Lanzar el daemon únicamente con `CREATE_NO_WINDOW` (sin `DETACHED_PROCESS`) para que tenga una consola oculta que heredan sus procesos hijos sin demora.

### 8. Autoarranque del daemon falla justo después de `symphony daemon stop`
- **Síntoma:** Si ejecutas `symphony daemon stop` e inmediatamente después un comando que autoarranca el daemon (especialmente en macOS), el nuevo daemon choca con la base de datos o el lock.
- **Causa:** «Socket cerrado» no significa «daemon terminado»: el daemon borra primero el socket IPC y después cierra SQLite y suelta el lock de instancia.
- **Arreglo:** Esperar y verificar que el **lock de instancia** (`transport::daemon_lock_held`) esté libre, no solo que el socket haya dejado de aceptar conexiones.

### 9. Error de socket IPC en macOS (`fchmod` `Unsupported` o ruta mayor de 104 bytes)
- **Síntoma:** En macOS falla la creación del socket Unix por permisos o porque la ruta en `$TMPDIR` (`/var/folders/…`) es demasiado larga.
- **Causa:** macOS no soporta `fchmod` sobre sockets (`ListenerOptionsExt::mode` devuelve `Unsupported`) y el campo `sun_path` tiene un tope de 104 bytes.
- **Arreglo:** Garantizar los permisos mediante el directorio contenedor con modo `0700` y usar el fallback `/tmp/symphony-<hash>/` comprobando dueño y permisos.

### 10. El arranque del daemon tarda ~5 segundos al detectar los CLIs instalados
- **Síntoma:** Cada inicio de `symphonyd` se demora cerca de 5 segundos antes de aceptar peticiones.
- **Causa:** `copilot --version` tarda entre 1,7 y 3,4 segundos (frente a ~0,5 s de `kimi` y ~0,3 s de `agy`); ejecutarlos en serie suma las esperas.
- **Arreglo:** `detect_all` ejecuta la detección de versiones de todos los proveedores en paralelo.

### 11. Límites de memoria o CPU de `ProcessKit` devuelven error en Linux o macOS (y `Unknown` en Windows)
- **Síntoma:** Crear un `ProcessGroup` con límites devuelve `ResourceLimit { reason: Unenforceable }` en Linux (runners sin cgroup v2 delegado) o `Unsupported` en macOS; en Windows, `LimitEvidence.memory` aparece como `Unknown` aunque el proceso haya muerto por límite de memoria.
- **Causa:** Limitaciones de delegación de cgroups v2 en Linux, ausencia de cgroups en macOS y falta de reporte en `LimitEvidence.memory` en Windows.
- **Arreglo:** Compilar `processkit` con las features `limits` y `stats`, manejar el `Err` al aplicar límites degradando a la capa de scheduling sin propagar el error con `?`, e inferir el OOM en Windows por el código de salida del proceso.

### 12. En Unix un CLI que aborta no llega como código de salida numérico
- **Síntoma:** Cuando un CLI muere por una señal en Linux o macOS, no trae exit code (en Windows sí llega como `Exited(134)`).
- **Causa:** En Unix `ExitStatus` reporta la muerte por señal como `Killed(Some(señal))`.
- **Arreglo:** Tratar cualquier señal no enviada por Symphony como un crash (`FAILED`), reservando `KILLED` exclusivamente para cuando el usuario ejecuta `stop` o `kill`.

### 13. En Windows quedan procesos `node.exe` huérfanos al lanzar shims `.cmd` de npm
- **Síntoma:** En Rust `Command::new("claude")` no encuentra el shim `claude.cmd` de npm, o al matar el proceso quedan vivos `cmd.exe`, `conhost.exe` y `node.exe`.
- **Causa:** Los paquetes globales de npm en Windows instalan wrappers `.cmd` que levantan un árbol de procesos.
- **Arreglo:** Resolver el ejecutable real o invocar el `.cmd` supervisando y matando el árbol completo de procesos (`ProcessGroup` / Job Objects o `taskkill /F /T`).

---

## Git, worktrees y rutas

### 14. Peligro en Windows: `git worktree remove --force` borra el `node_modules` del repositorio base
- **Síntoma:** Al borrar el worktree de un agente en Windows donde `node_modules` estaba enlazado mediante una *junction*, se borra el contenido de `node_modules` del repositorio principal.
- **Causa:** `git worktree remove --force` en Windows sigue las *junctions* de directorio y vacía su destino.
- **Arreglo:** `Repo::worktree_remove` desenlaza las *junctions* y enlaces del primer nivel del worktree antes de llamar a `git worktree remove`.

### 15. Rutas cortas 8.3 en Windows (`C:\Users\LATITU~1\…`) bloquean a Claude Code y degradan a Codex
- **Síntoma:** Claude Code rechaza hasta la herramienta `Read` con `the permission system is flagging the Windows path`, y Codex falla al leer archivos o tarda 3,5× más.
- **Causa:** `%TEMP%` y `tempfile` en Windows pueden devolver rutas con nombres cortos 8.3 (`LATITU~1`), que los sistemas de permisos de los CLIs rechazan.
- **Arreglo:** El daemon normaliza las rutas con `transport::long_path` (usando la ruta larga completa, sin el prefijo `\\?\`).

### 16. Errores intermitentes `index.lock` / `Permission denied` en Git o `BrokenPipe` al iniciar un turno
- **Síntoma:** Operaciones concurrentes de Git sobre un mismo worktree fallan con `index.lock` (Linux) o `Permission denied` (Windows), o escribir el prompt a un CLI que falla al instante devuelve `EPIPE` (`BrokenPipe`).
- **Causa:** Varios procesos `git` simultáneos pisan el bloqueo del índice del worktree, y un subproceso que sale antes de leer `stdin` cierra el pipe.
- **Arreglo:** Ejecutar `git` con `GIT_OPTIONAL_LOCKS=0`, serializar `commit_all` con un candado de escritura por worktree (`crates/git`) e ignorar `BrokenPipe` en `launch_inner` para que se reporte la causa real de salida del CLI. Si falta el commit automático de un turno del chat, revisa los avisos `warn` en `<proyecto>/.symphony/logs/symphonyd.*.log`.

### 17. `git merge-tree --write-tree` devuelve código de salida 1 tanto por conflictos como por rama inexistente
- **Síntoma:** Un chequeo de preflight de merge no distingue entre un conflicto de fusión real y una referencia o rama que no existe.
- **Causa:** Ambos casos terminan con exit code `1` en `git merge-tree --write-tree`.
- **Arreglo:** Distinguirlos inspeccionando si la primera línea de `stdout` comienza con el OID válido del árbol generado.

---

## Proveedores, hooks y sesiones

### 18. Claude Code se queda en `RUNNING` indefinidamente al terminar su respuesta
- **Síntoma:** En una corrida real, Claude Code termina de contestar pero el proceso no sale y el agente sigue `RUNNING` hasta que salta el watchdog (`NO_HEARTBEAT`).
- **Causa:** Con `--input-format stream-json`, Claude Code mantiene `stdin` abierto esperando el siguiente mensaje del usuario.
- **Arreglo:** Al recibir el evento `result` (`TurnFinished`) en el stream, el ejecutor cierra el `stdin` del proceso para que Claude Code termine limpiamente (ADR-0005, adenda).

### 19. `claude --bare` falla pidiendo `ANTHROPIC_API_KEY`
- **Síntoma:** Claude Code rechaza arrancar con la suscripción del usuario y exige `ANTHROPIC_API_KEY`.
- **Causa:** El flag `--bare` ignora el inicio de sesión de la suscripción.
- **Arreglo:** No usar `--bare`; inyectar los hooks pasando `claude -p --settings '<json>'`.

### 20. Codex en Windows no ejecuta los hooks o ignora `.codex/hooks.json`
- **Síntoma:** Codex no dispara los hooks definidos en `<worktree>/.codex/hooks.json`, los rechaza por hash de confianza, o en Windows un comando `"C:/ruta con espacios/symphony.exe" hook emit` no hace nada y falla en silencio.
- **Causa:** Codex no lee `.codex/hooks.json` del worktree, exige confianza por hash y en Windows corre los hooks en PowerShell (donde una cadena entre comillas sin el operador `&` no se ejecuta).
- **Arreglo:** Inyectar los hooks por invocación con `-c 'hooks.<Evento>=[{hooks=[{type="command",command="…"}]}]'` y `--dangerously-bypass-hook-trust` (sin modificar `CODEX_HOME`), y en Windows usar la sintaxis `& 'ruta' args`.

### 21. Retención larga en `PreToolUse` falla a los 120 s en Codex o el comando se ejecuta al vencer el `timeout`
- **Síntoma:** Si un hook retiene un comando más allá de su `timeout`, tanto Claude como Codex ejecutan la herramienta igualmente; en Codex, además, una espera de 120 s hace fallar el tool call, el modelo reintenta y duplica la espera, y el proceso del hook vencido no se mata.
- **Causa:** El vencimiento de `PreToolUse` no bloquea en ningún CLI, y Codex tiene una tolerancia práctica de ~60 s por llamada retenida.
- **Arreglo:** Fijar `timeout` explícito en el hook, limitar la retención en Codex a 60 s (respondiendo `deny` con la explicación de la cola para esperas mayores, ADR-0003) y aplicar un límite de tiempo interno en `symphony hook emit`.

### 22. Codex se queda colgado en `waiting for network` o falla con `EPERM` en Windows
- **Síntoma:** Ante un corte de internet, Codex imprime `Reconnecting... n/5` y luego `waiting for network` esperando indefinidamente; o bien comandos como `node --test` y `pnpm` fallan con `EPERM` bajo `workspace-write`, y `codex exec resume -s …` rechaza el flag `-s`.
- **Causa:** Codex no aborta por sí solo al perder la red; su sandbox `workspace-write` en Windows bloquea lecturas en `C:\Users\<usuario>`; y el subcomando `exec resume` no admite `-s`.
- **Arreglo:** El watchdog de inactividad del daemon corta los runs sin latido (`NO_HEARTBEAT`), el permiso por defecto en `[providers.permissions]` es `codex_sandbox = "danger-full-access"` (P15.5.S2) y en `exec resume` el adaptador pasa el modo con `-c sandbox_mode="…"`.

### 23. Particularidades de Kimi, Copilot, Antigravity y OpenCode
- **Síntoma:**
  - Un chat con Kimi pierde el contexto tras el primer turno porque no se guardó el ID de sesión.
  - `agy --print` falla pidiendo argumento, `agy --sandbox` se cuelga en Windows, o Antigravity devuelve `SUCCESS` con respuesta vacía al intentar correr un comando.
  - `copilot -p -` envía el carácter `-` como prompt literal.
  - En OpenCode al principio solo aparece `opencode/default`, o un modelo escribe fuera de su worktree usando una ruta absoluta.
- **Causa:**
  - Kimi emite el ID de sesión únicamente por `stderr` (`To resume this session: kimi -r <uuid>`).
  - Antigravity requiere `--print= --input-format stream-json`, su flag `--sandbox` se bloquea en Windows y sin permisos deniega herramientas en headless (`denied_actions`).
  - Copilot lee `stdin` cuando se invoca sin `-p`.
  - OpenCode tarda ~7 s en listar `opencode models` en segundo plano y toma como raíz el repositorio principal.
- **Arreglo:**
  - El ejecutor pasa `stderr` a `parse_stderr_line` (ADR-0009) para capturar la sesión de Kimi y errores de Copilot.
  - Usar los flags exactos de cada adaptador, mantener `antigravity_skip_permissions = true` y `copilot_allow_all_tools = true` (o el harness de Symphony, salvo en Antigravity que no lo soporta).
  - En OpenCode, ejecutar `symphony providers refresh` unos segundos después del arranque para ver todos los modelos.

---

## Checkpoints, contexto, TUI y base de datos

### 24. El transcript va por detrás del disco tras un kill o no contiene TODOs estructurados (H1, H2)
- **Síntoma:** Tras matar un agente (`kill`), los últimos cambios de archivos no aparecen aún en el `.jsonl` del CLI, o el checkpoint no encuentra una lista de tareas `TodoWrite` / `update_plan`.
- **Causa:** En headless ningún CLI usa `TodoWrite` / `update_plan`, y el archivo de transcript puede ir con retraso respecto al sistema de archivos cuando el proceso muere abruptamente.
- **Arreglo:** El handoff toma siempre el estado vivo de Git (`git diff` y archivos del worktree) como fuente de verdad sobre los archivos y extrae el «qué seguía» del último mensaje del asistente en la cola del transcript (ADR-0004).

### 25. El chat cambia de proveedor en bucle o anida prompts de handoff
- **Síntoma:** Con cambio automático por umbral de tokens (`switch_at_tokens`), el chat vuelve a cambiar de proveedor en el turno inmediato aunque el run nuevo no haya reportado uso; o al hacer varios cambios de proveedor los prompts de handoff se anidan unos dentro de otros.
- **Causa:** Consultar el último `TurnUsage` del agente sin filtrar por `run_id` hereda el uso del run anterior, e incluir el primer mensaje `USER` de un run que arrancó desde un handoff duplica el bloque de handoff anterior.
- **Arreglo:** Filtrar siempre `TurnUsage` por el `run_id` actual y descartar los mensajes iniciales de runs de handoff (`repo::handoff_run_ids`) al construir la conversación del siguiente handoff.

### 26. Error `FOREIGN KEY constraint failed` en el worker de checkpoints
- **Síntoma:** Se pierden nuevos checkpoints después de que el daemon poda checkpoints viejos.
- **Causa:** Al borrar un checkpoint antiguo se eliminaba un objeto de diff que todavía tenía fragmentos en `context_chunks` referenciándolo por clave foránea.
- **Arreglo:** Conservar los objetos referenciados al podar checkpoints. Si un diagnóstico muestra que el daemon pierde escrituras de checkpoints, revisa primero los avisos `warn` del worker en los logs del daemon.

### 27. Cada tecla en la TUI se pulsa dos veces en Windows
- **Síntoma:** En la terminal de Windows, presionar una tecla (como `n`, `q` o las flechas) dispara la acción por duplicado.
- **Causa:** En Windows, `crossterm` entrega tanto el evento `KeyEventKind::Press` como `KeyEventKind::Release`.
- **Arreglo:** Ignorar los eventos que no sean `KeyEventKind::Press` en la TUI.
