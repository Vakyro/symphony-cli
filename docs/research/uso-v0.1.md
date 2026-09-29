# Uso real de Symphony v0.1

Registro de evidencia para P07.S10. Se agregan observaciones por día; no implica aprobar todavía la replanificación ni empezar P08.

## Día 1 · 2026-09-26

**Entorno:** Windows PowerShell, Rust 1.98.1. `cargo build -p symphony-cli -p symphony-daemon --release` terminó correctamente en 1 min 34 s.

### Recorrido y observaciones

1. Tras compilar, `symphony` no se reconoce desde el proyecto Arete. `cargo run -p symphony-cli --` tampoco funciona desde ahí porque Cargo busca `Cargo.toml` en el directorio actual o sus padres. Symphony solo arranca al ejecutarlo desde su propio repositorio.
2. Al enviar un mensaje a un agente, la respuesta indica que no tiene un executor corriendo.
3. La lectura de mensajes y el desplazamiento de la conversación necesitan mejorar.
4. La experiencia de v0.1 se percibe centrada en agentes autónomos/multiagente: crear una tarea, dejar correr el agente y repetir o paralelizar.
5. Se propone un chat general para interacciones breves o enfocadas, con selección de modelos disponibles y failover automático entre proveedores que conserve el contexto.
6. Symphony debería instalarse globalmente y abrirse desde cualquier carpeta. Al usarlo en un proyecto, debería guardar ahí sus datos del proyecto bajo `.symphony/`, sin requerir una instalación por proyecto.

### Diagnóstico y relación con el plan actual

| Observación | Lectura | Relación con el plan |
|---|---|---|
| Binario no disponible desde otro directorio | El build genera binarios, pero no los instala ni agrega `target/release` al PATH. `cargo run` es específico al workspace; el QUICKSTART explica ambos caminos, pero no cómo instalar o invocar el ejecutable desde otro repo. | Documentar/instruir instalación global es una mejora de distribución (P13 instaladores). La invocación desde cualquier cwd debería verificarse allí. No exige instalar Symphony dentro de cada proyecto. |
| Datos locales `.symphony` por proyecto | El código ya guarda configuración de proyecto en `<proyecto>/.symphony/project.toml`; la base de datos y el estado global usan `~/.symphony`. | El requisito de configuración ya existe desde P02. Aclarar qué datos son locales y cuáles globales en QUICKSTART; no mover DB/worktrees sin una decisión de arquitectura y migración. |
| Mensaje falla si no hay executor vivo | `Send` está definido como mensaje al executor activo; al terminar un turno, el proceso sale por el comportamiento de Claude descrito en la adenda de ADR-0005. La bitácora P07 ya registra `resume` como paso natural pendiente para mensajes posteriores. | Pendiente conocido P06–P07; debe entrar en la replanificación de S10 como prioridad de uso individual. La solución candidata es reanudar la sesión con el texto del mensaje, preservando su contexto. |
| Lectura y scroll deficientes | La vista de conversación se dibuja con texto envuelto y las teclas permiten mover una línea o diez; la UI muestra lo más nuevo inicialmente. No se encontraron controles de rueda del mouse. | Mejora de usabilidad TUI no asignada explícitamente a fase posterior. La replanificación debe decidir si es corrección v0.1.x antes de P08. |
| Chat general con elección de modelo y failover | La entidad y experiencia actuales son agentes asociados a tarea/worktree; no existe un modo conversacional general. El runtime ya tiene cambio de executor y política de failover, pero está integrado al ciclo de vida del agente. | Cambio de producto relevante; discutir en S10. Definir si el chat reutiliza agente/historial sin worktree o requiere entidad/flujo distinto. La continuidad del contexto durante failover necesita validar cada adapter/proveedor. |

### Priorización provisional

- **Bloquea el uso básico:** instalar o invocar `symphony` globalmente; mejorar el error y el flujo de mensajes después de completar una respuesta.
- **Ajuste de usabilidad acotado:** scroll de conversación (rueda, navegación por página y señal clara de posición).
- **Decisión de producto para S10:** chat general, selector de modelo y failover con contexto. Mantener como propuesta hasta decidir alcance y evidencia.
- **Ya implementado en parte:** `.symphony/project.toml` local; documentar que el estado operativo compartido sigue en `~/.symphony`.

### Datos todavía no observados

No se reportaron tareas reales completadas, número de handoffs, uso de ambos proveedores, fallas del agente durante trabajo ni impacto en el rendimiento de la máquina. El gate de dos semanas y dos proveedores sigue pendiente.

## Opinión de Codex sobre las primeras observaciones

Esta sección es análisis y recomendación del agente; no son observaciones reportadas por Leo ni decisiones aprobadas del producto.

1. **Invocación global:** Symphony debe poder abrirse desde cualquier directorio una vez instalado globalmente. El problema inmediato es que compilar crea binarios pero no los instala ni configura el `PATH`; `cargo run` solo funciona desde el workspace. Recomiendo resolver el flujo de instalación/invocación y verificarlo desde un proyecto externo. El plan ubica los instaladores en P13.
2. **Mensajes después de terminar el turno:** el envío actual requiere un executor vivo, así que no permite continuar la sesión completada. Recomiendo priorizar el `resume` del CLI oficial incluyendo el mensaje, para conservar el contexto existente. P07 ya registra este pendiente.
3. **Lectura de conversación:** el desplazamiento actual es por teclado, en saltos de una o diez líneas, y no incluye rueda del mouse. Recomiendo mejorar el scroll y hacer visible la posición de lectura; es una corrección acotada de TUI que S10 debe ubicar antes o después de P08.
4. **Chat general:** recomiendo tratarlo como una decisión de producto en S10, no asumir que es solo otra vista. El flujo actual se centra en agentes, tareas y worktrees. Antes de diseñarlo hay que decidir cómo un chat mantiene historial y contexto sin el ciclo de trabajo autónomo, y cómo transfiere el contexto al cambiar de proveedor.
5. **Datos del proyecto:** ya se crea `.symphony/project.toml` en el proyecto; la DB y el estado compartido viven en `~/.symphony`. Recomiendo documentar claramente esta separación. No movería DB ni worktrees al proyecto sin una decisión explícita de arquitectura y migración.

**Prioridad que recomiendo:** primero instalación/invocación global y continuación de mensajes; después la usabilidad del scroll; chat general y failover entre proveedores pasan a decisión de alcance en el ADR de P07.S10, usando también las notas que se acumulen durante las dos semanas de prueba.

## Recomendación clave: Chat agentico con failover entre proveedores

**Propuesta de Leo (observaciones generales tras pruebas adicionales):**

El atractivo principal de Symphony debería ser un **chat agentico simple** — sin requerir múltiples agentes autónomos ni despliegue complejo — que funcione como Claude Code o Codex pero con un killer feature: **cambiar automáticamente de proveedor cuando se agoten los tokens/contexto de uno, sin perder continuidad**.

### Detalles de la idea

1. **Vista única de chat:** un prompt agentico familiar (similar a Claude Code, Codex o ChatGPT), donde el usuario pide tareas y el agente responde con skills, herramientas y razonamiento.

2. **Multiproveedor transparente:**
   - Seleccionar qué proveedores están disponibles (ej: Claude Code + Codex).
   - Mientras un proveedor maneja la sesión, Symphony lo usa.
   - Cuando se agote su contexto/tokens, el sistema cambia automáticamente al siguiente sin perder el historial.
   - El usuario puede cambiar manualmente entre proveedores si lo desea.

3. **Preservación de contexto en failover:**
   - El historial de la conversación se guarda localmente (.symphony/ del proyecto).
   - Al cambiar de proveedor, se resume/reinicia la sesión con el contexto acumulado (ej: "aquí está el historial hasta ahora, continuemos con...").
   - Cada proveedor adapta el contexto a sus límites y habilidades.

4. **Integración con skills:** el chat hereda todas las skills configuradas (build, test, review, etc.) y las invoca según el proveedor activo.

### Por qué esto es el "ahora" de Symphony

- **Resuelve un problema real:** los usuarios no necesitan coordinar múltiples agentes desde el inicio; comienzan con una experiencia simple.
- **Diferenciador clave:** el failover sin pérdida de contexto es lo que hace única a Symphony respecto a Claude Code, Codex o similares por separado.
- **Reciclaje de P05–P07:** el event bus, adapters y runtime ya soportan esto; solo se necesita una nueva vista y entidad de "chat".

### Ubicación en el plan

- **Decisión en P07.S10 (ahora):** ¿es este el alcance para P08, o se posterga hasta tener más datos de uso?
- **Implementación (P08+):** entidad de chat persistent, selector de proveedor, política de failover con contexto, vista terminal y posiblemente GUI.
- **Validar antes:** que ambos proveedores (Claude Code + Codex) mantengan consistencia al retomar sesiones.

## Referencias externas

Conversaciones con ChatGPT y Claude sobre Herdr, AX y la evaluación del repo: [referencias-herdr-ax.md](referencias-herdr-ax.md). Sus puntos sobre `phase + conditions`, event bus, suspend/resume y `Workspace` entran al ADR de P07.S10.
