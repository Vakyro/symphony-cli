# Protocolo IPC

La comunicación entre el cliente de usuario (`symphony`, ya sea operando en modo línea de comandos o como interfaz gráfica de terminal TUI) y el daemon de fondo (`symphonyd`) se realiza mediante un **protocolo IPC local de alto rendimiento**, seguro y orientado a mensajes (STACK §6.2, §46, §47).

El diseño evita deliberadamente capas de transporte pesadas como gRPC o HTTP para eliminar sobrecargas de serialización y asegurar la menor latencia posible al transmitir flujos continuos de eventos y comandos.

---

## 1. Framing y formato binario

La capa de enmarcado (*framing*, implementada en `crates/protocol/src/frame.rs`) garantiza la delimitación de mensajes sobre flujos de bytes asíncronos continuos:

```
┌───────────────────────────────┬─────────────────────────────────────────────┐
│  Longitud del payload (4 B)   │            Payload JSON (N bytes)           │
│      u32 big-endian           │             Codificado en UTF-8             │
└───────────────────────────────┴─────────────────────────────────────────────┘
```

1. **Cabecera fija (4 bytes):** contiene la longitud exacta del payload en bytes codificada como un entero sin signo de 32 bits en orden de red big-endian (`u32::to_be_bytes()`).
2. **Payload:** contenido del mensaje serializado en JSON y codificado en UTF-8.
3. **Límite máximo de tamaño (`MAX_FRAME_LEN`):** ningún frame puede superar **8 MiB** (`8 * 1024 * 1024` bytes).
4. **Validación previa a la reserva de memoria:** al decodificar un frame (`read_frame` o `FrameDecoder::next_frame`), el decodificador lee primero la cabecera y valida que `len <= MAX_FRAME_LEN` antes de asignar el vector de bytes de recepción. Si la cabecera excede este límite o llega truncada, la conexión falla de inmediato con `ProtocolError::FrameTooLarge`, previniendo ataques de denegación de servicio por agotamiento de memoria (STACK §47).
5. **Separación de datos voluminosos:** el protocolo IPC está optimizado para eventos de control y señalización. Los artefactos extensos (diffs masivos de Git, capturas completas de archivos o volcados de árbol sintáctico) se guardan en el almacén de objetos content-addressed (`symphony-object-store`) y por IPC solo viaja su identificador o resumen.

---

## 2. Tipos de mensajes y versionado

Todos los mensajes que circulan por el protocolo implementan la enumeración `Message` (`crates/protocol/src/message.rs`). Cada mensaje incluye obligatoriamente el campo numérico de versión:

```json
{
  "type": "request",
  "protocol": 1,
  "id": "req-01JM5...",
  "method": "agent.list",
  "params": {}
}
```

- **Versión de protocolo (`PROTOCOL_VERSION`):** actualmente fijada en `1`. Si un mensaje omite el campo `protocol` o especifica una versión distinta a `1`, se rechaza con `ProtocolError::MissingVersion` o `ProtocolError::UnsupportedVersion`.
- **Rechazo estricto de campos desconocidos:** las estructuras Rust están anotadas con `#[serde(deny_unknown_fields)]`. Cualquier campo inesperado invalida el mensaje con un error explícito.

### Categorías de mensajes

| Tipo (`type`) | Estructura | Dirección | Propósito |
|---|---|---|---|
| `request` | `Request { protocol, id, method, params }` | Cliente → Daemon | Invocación de un método RPC en el daemon. `id` es un identificador generado por el cliente para correlacionar la respuesta. `params` debe ser un objeto JSON o nulo. |
| `response` | `Response { protocol, id, outcome }` | Daemon → Cliente | Respuesta a un `request`. `outcome` adopta la forma `{"ok": ...}` en caso de éxito o `{"error": {"code": "...", "message": "..."}}` en caso de fallo. |
| `subscribe` | `Subscribe { protocol, id, topics }` | Cliente → Daemon | Petición para transformar la conexión en un flujo de eventos continuo. Permite suscribirse a tópicos específicos (como `agent.event`). |
| `event` | `Event { protocol, topic, payload }` | Daemon → Cliente | Difusión asíncrona de eventos hacia clientes suscritos (por ejemplo, turnos de conversación, invocaciones de herramientas o cambios de estado). |

---

## 3. Transporte por sistema operativo y seguridad

El transporte físico (`crates/protocol/src/transport.rs`) se adapta a las primitivas nativas de cada sistema operativo utilizando la biblioteca `interprocess`, aplicando estrictos controles de acceso para impedir que otros usuarios locales del sistema accedan al socket o inyecten comandos:

### En sistemas Unix (Linux y macOS)
- **Mecanismo:** Unix Domain Sockets en modo stream asíncrono (`tokio`).
- **Ruta preferida:** `<proyecto>/.symphony/run/symphonyd.sock`.
- **Manejo de `sun_path`:** la longitud máxima permitida para la ruta de un socket Unix está limitada por el kernel (104 bytes en macOS, 108 bytes en Linux). Si la ruta del proyecto hace que la ruta del socket supere los 100 caracteres (`MAX_SOCKET_PATH`), Symphony conmuta automáticamente a una ruta alternativa corta:
  ```
  /tmp/symphony-<fnv1a_hash>/symphonyd.sock
  ```
  donde `<fnv1a_hash>` es un hash FNV-1a determinista de 64 bits calculado a partir de la ruta canónica del proyecto.
- **Permisos de seguridad:** el directorio contenedor (`run/` o el directorio en `/tmp`) se crea exclusivamente con permisos octales `0700` (`rwx------`) y se valida que el propietario (`uid`) coincida con el usuario actual. En Linux y BSD, el archivo del socket se configura con permisos `0600` (`rw-------`). En macOS (donde `fchmod` en sockets retorna `Unsupported`), el aislamiento se garantiza mediante el directorio padre `0700`.
- **Rechazo deliberado del namespace abstracto de Linux:** no se utiliza el namespace abstracto (`abstract sockets`) porque carece de permisos de sistema de archivos, lo que permitiría a cualquier proceso local en el mismo host interactuar con el socket.
- **Verificación de identidad del servidor (`verify_server`):** al conectar, el cliente examina las credenciales del socket del par (`peer_creds()`):
  - En Linux: valida que el `pid` del proceso servidor coincida exactamente con el registrado en `<proyecto>/.symphony/run/symphonyd.pid`.
  - En macOS: valida que el `euid` del proceso servidor coincida con el propietario del directorio `run/`.

### En Windows
- **Mecanismo:** Named Pipes (tuberías con nombre locales) en la ruta:
  ```
  \\.\pipe\symphonyd-<fnv1a_hash>
  ```
- **Ruta canónica larga (`long_path`):** para evitar que nombres cortos 8.3 de MS-DOS (por ejemplo `C:\Users\LATITU~1\...`) generen identificadores de tubería distintos para la misma carpeta, la ruta se expande a su formato largo y canónico en minúsculas antes de calcular el hash FNV-1a.
- **Descriptor de seguridad (DACL):** las tuberías con nombre en Windows comparten un espacio global. Para impedir accesos no autorizados, Symphony asigna una lista de control de acceso discrecional (DACL) protegida:
  ```
  D:P(A;;GA;;;OW)
  ```
  *(D:P = DACL protegida sin herencia; A;;GA;;;OW = Concede Generic All exclusivamente a Owner Rights, el usuario que creó la instancia).* Ningún otro usuario del sistema puede abrir, leer ni escribir en la tubería.
- **Verificación de PID:** el cliente valida el identificador de proceso contra el archivo `symphonyd.pid` antes de enviar cualquier información de control.

---

## 4. Tópicos de suscripción y streaming de eventos

Cuando un cliente como la TUI necesita recibir cambios en tiempo real, envía un mensaje `Message::Subscribe`:

```json
{
  "type": "subscribe",
  "protocol": 1,
  "id": "sub-1",
  "topics": ["agent.event"]
}
```

El daemon confirma la suscripción con `Response::ok` y comienza a transmitir `Message::Event` a medida que ocurren:

| Tópico | Generado cuando | Payload |
|---|---|---|
| `agent.event` | Ocurre un evento canónico en cualquier agente (cambio de estado, salida de herramienta, mensaje recibido, turno completado). | Objeto con `project_id`, `agent_id`, `run_id`, `type`, `source` y `occurred_at`. |
| `bus.lagged` | Un cliente lento desborda el búfer interno del bus de eventos (`TUI_BUFFER = 1024`). | Objeto `{"missed": <número>}`. Señala a la TUI que debe recargar el estado completo desde SQLite. |

*Invariante del bus:* todo evento se persiste en la base de datos relacional SQLite mediante el hilo escritor antes de ser difundido por el bus, garantizando que el historial guardado en disco nunca quede desfasado frente a lo emitido en vivo.

---

## 5. Tabla de métodos IPC despachados por `server.rs`

La siguiente tabla recoge **todos los métodos IPC implementados en la tabla de despacho del servidor** (`crates/daemon/src/server.rs`, función `dispatch`). Cada entrada proviene directamente de la lógica de bifurcación del servidor:

| Método IPC | Alias | Parámetros principales | Descripción y propósito en el daemon |
|---|---|---|---|
| `ping` | — | Ninguno | Comprueba la conectividad con el daemon. Responde inmediatamente con `{"pong": true}`. |
| `status` | — | Ninguno | Devuelve las estadísticas globales del daemon (PID, uptime, versión, proyecto activo, contadores de agentes, tareas y uso de recursos). |
| `shutdown` | — | Ninguno | Ordena el apagado limpio y controlado del daemon, cancelando tareas activas, liberando sockets y esperando el drenado de conexiones. |
| `hook.emit` | — | Objeto `AgentEvent` de adapter | Recibe eventos de ciclo de vida o de herramientas emitidos por los hooks de los CLIs de IA (`symphony hook emit`) y los transfiere al bus de eventos y al registro relacional. |
| `providers.list` | — | Ninguno | Consulta la lista de proveedores soportados, indicando su disponibilidad, estado de autenticación, cuotas y métricas de salud. |
| `usage.get` | — | `days` *(opcional)* | Obtiene el histórico y desglose de consumo de tokens (informados por el proveedor y estimados internamente) agrupados por proveedor y modelo. |
| `providers.refresh` | — | Ninguno | Fuerza un nuevo escaneo en el sistema operativo para redescubrir CLIs de IA instalados, modelos disponibles y actualizar su salud. |
| `agent.create` | `agent.spawn` | `title`, `project_root`, `model`, `profile`, `failover`, `context_mode`, `priority`, `depends_on`, `description`, `cwd` | Crea un nuevo agente: provisiona su tarea, rama Git, directorio `worktree` aislado, configuración inicial de contexto y arranca su primer ejecutor. |
| `agent.list` | `agents.list` | `all` *(bool opcional)* | Lista los agentes del proyecto actual. Si `all` es verdadero, incluye también agentes en estados terminales (`COMPLETED`, `FAILED`, `CANCELLED`). |
| `chat.get` | — | `project_root` *(opcional)* | Obtiene o crea el agente permanente asociado al chat general del proyecto, vinculado a la rama Git `symphony/chat`. |
| `skills.list` | — | Ninguno | Devuelve la lista de skills compartidas registradas en `~/.symphony/skills` y en la carpeta `.symphony/skills` del proyecto. |
| `agent.inspect` | — | `agent` (ID o prefijo) | Inspecciona a fondo la ficha completa de un agente: estado actual, tarea asociada, rama, ruta del worktree, checkpoints acumulados y ejecuciones pasadas. |
| `agent.send` | — | `agent`, `message` | Envía un mensaje de texto al agente. Si el proceso del ejecutor había finalizado su turno, reanuda su sesión para atender la nueva entrada. |
| `agent.pause` | — | `agent` | Suspende temporalmente la ejecución del agente, pausando el proceso hijo del CLI supervisor mediante señales de sistema operativo. |
| `agent.resume` | — | `agent` | Reanuda la ejecución del proceso supervisor de un agente previamente pausado. |
| `agent.stop` | — | `agent` | Solicita una detención ordenada (*graceful stop*) del ejecutor activo del agente. |
| `agent.kill` | — | `agent` | Termina de forma inmediata e incondicional el árbol de procesos del ejecutor del agente (*kill-tree*) y marca al agente como cancelado. |
| `agent.switch` | — | `agent`, `model` o `profile`, `message` *(opcional)* | Realiza un cambio en caliente del modelo o perfil de un agente, guardando un checkpoint incremental e iniciando un nuevo ejecutor con handoff. |
| `agent.next_phase` | — | `agent` | Avanza la ejecución del agente a la siguiente fase de su plan formal (`.symphony/plan/<task_id>.md`), arrancando una sesión limpia con contexto sintetizado. |
| `agent.diff` | — | `agent` | Obtiene la salida de `git diff` correspondiente a los cambios introducidos en el worktree del agente respecto a su commit base. |
| `agent.logs` | — | `agent`, `limit` *(opcional)* | Recupera el historial secuencial de mensajes, intervenciones del modelo y llamadas a herramientas del agente. |
| `agent.export` | — | `agent` | Exporta la conversación completa y el registro de trabajo del agente en formato Markdown estructurado. |
| `agent.attach` | — | `agent` | Obtiene el comando de terminal y entorno necesarios para conectar manualmente el CLI nativo del proveedor a la sesión viva del agente. |
| `agent.activity` | — | `agent` | Devuelve la lista cronológica de las últimas 200 actividades y eventos relevantes ocurridos en el agente. |
| `agent.history` | — | `agent` | Consulta el histórico completo de cambios de ejecutor (*executor changes*), transiciones de estado y causas de relevo del agente. |
| `models.list` | — | Ninguno | Devuelve el catálogo unificado de todos los modelos soportados por los proveedores detectados, junto con sus capacidades y ventanas de contexto. |
| `profiles.list` | — | Ninguno | Lista los perfiles de enrutamiento configurados (como `@code`, `@fast`, `@review`) junto con sus descripciones y pesos de optimización. |
| `route.explain` | — | `agent` | Proporciona la explicación detallada y los cálculos matemáticos por los cuales el enrutador determinista seleccionó un modelo o descartó a otros candidatos. |
| `context.inspect` | — | `agent` | Inspecciona el paquete de contexto ensamblado para el último handoff del agente (archivos adjuntos, fragmentos comprimidos, nivel de compresión aplicado). |
| `context.stats` | — | Ninguno | Proporciona estadísticas globales del subsistema de contexto: volumen total comprimido, ratios de ahorro de tokens y tamaños de caché. |
| `context.raw` | — | `uri` | Recupera el contenido textual original de un archivo o recurso referenciado por una URI virtual de contexto `ctx://`. |
| `context.search` | — | `query`, `limit` | Búsqueda léxica e indexada sobre el código y contexto del proyecto, utilizada por las herramientas MCP. |
| `context.retrieve` | — | `uri` | Recupera fragmentos de contexto indexados según su identificador para consumo del broker MCP. |
| `context.lines` | — | `uri`, `start`, `end` | Obtiene un rango específico de líneas de un recurso indexado en el motor de contexto. |
| `skills.shared` | `mcp.list` | Ninguno | Devuelve la lista unificada de servidores MCP configurados en `mcp.toml` y de skills compartidas disponibles en el entorno. |
| `skills.set_enabled` | `mcp.set_enabled` | `name`, `enabled` | Activa o desactiva dinámicamente un servidor MCP o una skill compartida para futuras invocaciones. |
| `fact.set` | — | `key`, `value`, `kind`, `project_root` | Registra o actualiza un hecho persistente del proyecto (`ARCHITECTURE`, `DECISION`, `CONSTRAINT`, `CONVENTION`) para alimentar el contexto de los agentes. |
| `fact.list` | — | `project_root`, `all` *(opcionales)* | Consulta la lista de hechos consolidados del proyecto, filtrados por vigencia o clasificación. |
| `provider.set_enabled` | — | `provider`, `enabled` | Habilita o deshabilita manualmente un proveedor de IA específico en la configuración en caliente. |
| `project.status` | — | `project_root` *(opcional)* | Consulta el estado general del proyecto: identificador único, ruta raíz, tipo de proyecto (`code`, `document`, `media`, `general`) y ramas activas. |
| `project.init` | — | `path`, `kind`, `name` | Inicializa una carpeta como proyecto de Symphony, creando su estructura `.symphony/project.toml` y configurando reglas de exclusión en `.gitignore`. |
| `merge.prepare` | — | `agent`, `target_branch` | Inicia el preflight de integración de la rama del agente en la rama destino, analizando posibles conflictos y ejecutando verificaciones previas. |
| `merge.approve` | — | `agent` | Registra la aprobación formal de una integración previamente analizada y revisada. |
| `merge.abort` | — | `agent` | Cancela un proceso de integración en curso y restituye el estado de la rama a su situación previa. |
| `merge.integrate` | — | `agent` | Aplica la fusión efectiva (*merge*) de la rama del agente sobre la rama base en el repositorio Git. |
| `merge.resolve` | — | `agent`, `resolution` | Registra la resolución de discrepancias o conflictos surgidos durante la preparación de la integración. |
| `merge.get` | — | `agent` | Consulta el estado detallado y el resultado de los análisis de integración de un agente. |
| `review.start` | — | `agent`, `model`, `profile` | Lanza un agente especializado de revisión (*review agent*) para auditar los cambios de código producidos antes de autorizar su integración. |
| `suggestion.list` | — | `project_root` *(opcional)* | Lista las recomendaciones generadas por el sistema sugiriendo cambios de modelo (por lentitud, agotamiento de cuota o umbrales de coste). |
| `suggestion.resolve` | — | `id`, `action` (`switch`, `keep`, `mute`) | Procesa la decisión del usuario ante una sugerencia: realizar el cambio de modelo (`switch`), mantener el modelo actual (`keep`) o silenciar la alerta (`mute`). |
| `plugin.list` | — | `project_root` *(opcional)* | Lista los plugins detectados en el sistema (`~/.symphony/plugins`) y en el proyecto local. |
| `plugin.check` | — | `name` | Comprueba el estado de un plugin ejecutando su comando de autodescripción (`describe`). |
| `chat.merge.prepare` | — | `project_root` | Prepara la propuesta de integración de los commits acumulados en la rama `symphony/chat` hacia la rama principal `main`. |
| `chat.merge.review` | — | `project_root`, `model`, `profile` | Invoca una revisión por un agente de código sobre los cambios de la rama de chat antes de integrarlos en `main`. |
| `chat.merge.integrate` | — | `project_root` | Integra de manera definitiva los cambios validados y aprobados de `symphony/chat` en la rama `main`. |
| `batch.create` | — | `tasks`, `strategy` | Crea un lote coordinado de tareas múltiples con dependencias para ejecución paralela. |
| `batch.status` | — | `batch_id` | Consulta el avance general de un lote de tareas, sus agentes asignados y métricas de ejecución. |
| `batch.integrate` | — | `batch_id` | Ejecuta la integración secuencial y ordenada de todas las ramas generadas por las tareas completadas de un lote. |
| `validation.run` | — | `agent`, `tier` | Dispara la ejecución de la suite de validación en el nivel especificado (compilación, linters, pruebas unitarias o pruebas completas). |
| `validation.get` | — | `validation_id` | Recupera el estado, registros y veredicto resultante de una ejecución de validación. |
| `task.list` | — | `project_root` *(opcional)* | Lista las tareas registradas en el proyecto y su estado en el flujo (`BACKLOG`, `READY`, `RUNNING`, `DONE`, `FAILED`, etc.). |
| `system.get` | — | Ninguno | Obtiene métricas en tiempo real del sistema anfitrión: carga de CPU, memoria RAM disponible y saturación de slots del planificador. |
| `performance.set` | — | `profile` | Cambia el perfil de rendimiento (`[performance] profile`) del `config.toml`. |
| `task.depend` | — | `task_id`, `depends_on_id` | Establece una dependencia dirigida en el DAG del proyecto: la primera tarea esperará a que la segunda finalice con éxito. |
| `task.undepend` | — | `task_id`, `depends_on_id` | Elimina una dependencia previamente establecida entre dos tareas. |
| `task.deps` | — | `task_id` | Devuelve el grafo de tareas antecedentes y consecuentes vinculadas a una tarea. |
| `task.resolve_dependency` | — | `task_id`, `dependency_id` | Resuelve o desbloquea manualmente una dependencia pendiente sobre una tarea. |
| `milestone.create` | — | `title`, `description` | Crea un hito de decisión humana dentro del flujo de trabajo del proyecto. |
| `milestone.list` | — | `project_root` *(opcional)* | Consulta la lista de hitos definidos en el proyecto y su estado. |
| `milestone.assign` | — | `milestone_id`, `task_id` | Asocia un hito a una tarea o agente específico. |
| `milestone.decide` | — | `milestone_id`, `decision` | Registra la resolución humana (aprobación, rechazo o continuación) de un hito bloqueante. |
| `recovery.list` | — | Ninguno | Lista las sesiones interrumpidas o huérfanas detectadas tras un reinicio inesperado para su tratamiento en el Recovery Center. |
| `recovery.act` | — | `session_id`, `action` (`resume`, `discard`, `archive`) | Aplica una acción correctiva de recuperación sobre una sesión interrumpida. |
