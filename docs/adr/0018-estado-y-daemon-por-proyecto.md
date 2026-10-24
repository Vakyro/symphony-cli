# ADR-0018 · Estado y daemon por proyecto

- **Estado:** ACEPTADO
- **Fecha:** 2026-10-23
- **Autor:** agente (claude-code/opus-5.5) · **Aprobado por:** Leo (2026-10-23)
- **Fase/paso:** P15.5.S0

## Contexto
Hoy el estado es global: `~/.symphony/` guarda `symphony.db`, `objects/`, `logs/`, `plugins/` y `worktrees/` (`crates/core/src/config.rs`, `crates/daemon/src/runtime.rs`), y hay un solo daemon por usuario (`crates/protocol/src/transport.rs`). Algunas vistas filtran por `project_id` y otras (`batch.*`, `task.list`, `milestone.*`, `suggestion.list`, `chat.get`, `usage.get`) no, así que desde un proyecto se ven agentes, oleadas y chats de otros. Leo pidió que cada proyecto sea independiente y que su memoria viva en su carpeta.

## Opciones consideradas
1. **Seguir con un daemon y una DB global y filtrar todo por `project_id`.** El aislamiento depende de que cada consulta nueva se acuerde del filtro.
2. **Un daemon global que abre una DB por proyecto.** Aísla los datos, pero el daemon sigue siendo un punto común (un fallo o una DB bloqueada afecta a todos).
3. **Un daemon por proyecto, con el estado dentro del proyecto.** El aislamiento es por construcción.

## Decisión
Opción 3 (elegida por Leo).

- **Dentro de `<proyecto>/.symphony/`:** `symphony.db`, `objects/`, `worktrees/`, `logs/`, `run/` (socket, pid) y la memoria (facts, checkpoints, chat). `project.toml` sigue ahí.
- **En `~/.symphony/`:** solo lo del usuario: `config.toml` (proveedores y permisos), caché de modelos, plugins globales y skills globales.
- **Desempeño aprendido (ADR-0016):** se comparte entre proyectos (decisión de Leo). Se guarda en `~/.symphony/learning.db`, escrita con su propio writer. El daemon de un proyecto lee de ahí y le envía muestras; la tabla `profile_models` de cada proyecto ya no lleva `sample_count` ni `outcome_score_sum` (migración 008 mueve las columnas).
- **Socket o pipe:** derivado de un hash de la ruta canónica del proyecto. `symphony` arranca o se conecta al daemon de la carpeta actual (sube hasta encontrar `.symphony/project.toml` o la raíz git). `symphony daemon stop|status` actúa sobre ese daemon; `--all` sobre todos.
- **Topes de recursos:** cada daemon aplica los suyos. Un archivo de lock en `~/.symphony/run/` publica el uso para que la suma de daemons no pase el tope del usuario (`ponytail:` solo informativo en v1).
- **Git:** `project.init` añade `.symphony/` al `.gitignore`, salvo `project.toml`.
- **Migración:** `symphony migrate split` reparte la DB global por `project_id` hacia cada proyecto, mueve objects y worktrees, y deja la global como respaldo. Es reversible hasta que Leo la borra.

## Consecuencias
- Se tocan `SymphonyHome`, el arranque del daemon, el cliente (CLI, TUI, gui-bridge) y `runtime.rs` (ruta de worktrees).
- Los worktrees dentro del proyecto también quedan accesibles para los sandboxes de los CLIs (P15.5.S2).
- Hace falta un test L2 con dos proyectos a la vez que verifique que ninguno ve datos del otro.
- ADR-0006 (rama `symphony/chat`) no cambia: el chat general pasa a ser el chat del proyecto.
