# Guía rápida: Usar Symphony v0.1

## 0. Requisitos previos

- **Rust 1.95+** y Git instalados
- **Claude Code** o **Codex** (o ambos) instalados y autenticados
- Windows, macOS o Linux

Verifica:
```bash
rustc --version
claude --version    # si lo tienes
codex --version     # si lo tienes
```

---

## 1. Obtener Symphony

### Opción A: Compilar desde el repo
```bash
git clone https://github.com/Vakyro/symphony-cli.git
cd symphony-cli

# Compilar (toma ~2 min la primera vez)
cargo build -p symphony-cli -p symphony-daemon --release

# Binarios listos en target/release/
# - symphony-cli (o symphony-cli.exe en Windows)
# - symphonyd (o symphonyd.exe en Windows)
```

### Opción B: Usar desde el repo directamente
```bash
cd symphony-cli
# Todos los comandos abajo funcionan con "cargo run -p symphony-cli --" 
# en lugar de "symphony"
```

Asume `symphony` en el PATH de aquí en adelante.

---

## 2. Entrar a tu proyecto

```bash
cd /ruta/a/tu/proyecto
symphony
```

**Primera vez:** Se abre la TUI. Symphony detecta:
- ✅ CLIs de proveedores (Claude Code, Codex) en tu máquina
- ✅ El proyecto Git
- ✅ Dependencias (pnpm, npm, pip, etc.)

Si todo está verde, pasas a **Home**.

---

## 3. Home: El dashboard

Ves una lista de agentes (vacía la primera vez) y opciones:

```
SYMPHONY · proyecto-name

 HOME

 Agentes         Activos: 0

 (lista vacía)

─────────────────────────────────────────────────────────────

 [ n ]  Crear agente       [ p ]  Proveedores
 [ r ]  Recuperación       [ q ]  Salir

 Presiona : para la barra de comandos
```

---

## 4. Crear tu primer agente

**Opción A: Con menú (teclas)**
1. Presiona `n`
2. Escribe la tarea:
   ```
   Agregar validación de email con tests
   ```
3. Presiona Enter para avanzar por el menú (selecciona proveedor, modelo)
4. El agente se crea y abre en la vista de trabajo

**Opción B: Comando rápido (barra de comandos)**
1. Presiona `:`
2. Escribe:
   ```
   spawn claude/sonnet Agregar validación de email con tests
   ```
3. Presiona Enter → agente creado al instante

### ¿Qué hace Symphony?
- Crea un `worktree` (rama aislada) para el agente
- Lanza Claude Code (o el modelo que elegiste)
- Muestra la vista de trabajo del agente en vivo

---

## 5. Trabajar con el agente

### Ves la vista de agente:
```
AGENTE #1 · Agregar validación de email con tests

 Estado: RUNNING (Claude Code / haiku-4.5)
 Worktree: symphony/session-xyz/agent-1

 ┌─────────────────────────────────────────┐
 │ Conversation                            │
 │                                         │
 │ [Claude empieza a trabajar]             │
 │                                         │
 └─────────────────────────────────────────┘

 [ ↑ ↓ ]  Scroll  [ a ]  Attach
 [ d ]  Diff     [ s ]  Switch  [ : ]  Comando
```

El agente trabaja solo. Tú ves:
- ✅ El progreso en tiempo real
- ✅ Los archivos que toca
- ✅ Los tests que corre
- ✅ Cuándo termina o falla

---

## 6. Intervenir (opciones)

### Ver el diff de lo que cambió
```
Presiona [ d ]  →  Ves git diff del worktree del agente
```

### Abrir el agente en su CLI nativo
```
Presiona [ a ]  →  Se abre Claude Code en su propia terminal
                   Puedes escribir mensajes nuevos
                   Después vuelve a Symphony
```

### Hacer un comando en la barra
```
Presiona [ : ]
agents           # ver todos los agentes
logs 1           # ver logs del agente #1
diff 1           # ver diff del agente #1
inspect 1        # detalles del agente #1
```

---

## 7. Cambiar de modelo (handoff)

Si Claude se queda sin cuota o quieres otro modelo:

```
Presiona [ s ]  →  Men menú de "Switch model"
```

O comando:
```
:
switch 1 codex/gpt-5.x
```

**Lo que pasa:**
- Claude se detiene sin perder su trabajo
- Symphony crea un checkpoint (diff + última línea del transcript)
- Codex arranca **en el mismo worktree** sin que le vuelvas a explicar
- Continúa desde donde Claude se quedó

---

## 8. Ver cambios y mergear

Cuando el agente dice "listo":

1. **Revisar diff** desde Home:
   ```
   Elige el agente #1
   [ d ]  →  git diff
   ```

2. **Mergear a main** desde Home:
   ```
   [ : ]
   merge 1
   ```

O manualmente:
```bash
cd /ruta/a/tu/proyecto
git branch      # ves symphony/session-xyz/agent-1
git switch main
git merge symphony/session-xyz/agent-1
git branch -d symphony/session-xyz/agent-1
```

---

## 9. Múltiples agentes en paralelo

Crea 2 agentes a la vez:

```
Home → [ n ]  Agregar autenticación
Home → [ n ]  Refactorizar estilos
```

Symphony **limita recursos automáticamente** para que ambos corran sin trabar la máquina.

Ves ambos:
```
Home

Agentes: 2

 #1  Agregar autenticación    RUNNING (Claude)
 #2  Refactorizar estilos     RUNNING (Codex)

─────────────────────────────────────────────────
```

Selecciona uno con `↑ ↓` / número y entra a su vista.

---

## 10. Si algo falla

### Agente se cuelga
```
[ : ]
kill 1
```

### Necesitas recuperarte de un crash
```
Home → [ r ]  Recovery Center
       ↓
       Ver agentes interrumpidos
       Elegir uno → retomar
```

### Ver logs
```
[ : ]
logs 1 --tail 50    # últimas 50 líneas
```

---

## 11. Cerrar y volver

```
Home → [ q ]  Salir
```

Cuando vuelvas a entrar al proyecto:
```bash
cd /ruta/a/tu/proyecto
symphony
```

**Se restaura todo:**
- ✅ Agentes anteriores listos
- ✅ Worktrees intactos
- ✅ Tareas sin terminar

---

## 12. Desde el terminal (sin TUI)

Si prefieres no usar la TUI, hay comandos:

```bash
# Crear agente
symphony spawn "Tarea aquí"

# Ver agentes
symphony agents

# Inspeccionar uno
symphony inspect 1

# Ver diff
symphony diff 1

# Ver logs
symphony logs 1

# Matar agente
symphony kill 1

# Merge
symphony merge 1
```

---

## 13. Configuración (opcional)

Edita `~/.symphony/config.toml`:

```toml
[providers]
# Modelo favorito para "decide_later" (sin especificar)
default_model = "claude/sonnet"

# Cuántos agentes en paralelo
max_concurrent_agents = 3

[scheduler]
# Segundos entre checks de salud
health_check_interval = 5

# Si un agente tarda más de esto, aviso
warning_timeout_secs = 300
```

---

## 14. Flujo completo (resumen)

```
1. cd proyecto
2. symphony
3. Presiona [ n ] → tarea nueva
4. Ves al agente trabajar
5. Presiona [ d ] para ver cambios
6. Presiona [ s ] para cambiar modelo si falta cuota
7. Cuando termina, presiona [ : ] y escribe "merge 1"
8. Los cambios van a main
9. [ q ] para salir
```

---

## 15. Tips

- **Speed mode:** Presiona `T` en Home para cambiar tema (algo te lo dice con teclas)
- **Múltiples terminales:** Puedes tener 2 ventanas `symphony` simultáneas en proyectos distintos
- **Repo de prueba:** Clona https://github.com/Vakyro/symphony-demo (pequeño Next.js con autenticación lista)
- **Debug:** Seteá `RUST_LOG=debug` antes de `symphony` para ver tracing
- **Problema:** Si `symphonyd` queda huérfano (test interrupido), `pkill symphonyd` / Task Manager

---

## Próximos pasos (P08+)

Cuando Leo use v0.1 durante 2+ semanas y documente learnings:
- ⏳ **P08**: Scheduler de 3+ agentes, DAG de tareas
- ⏳ **P09**: Context Engine (handoff sin explicación)
- ⏳ **P10**: Failover automático
- ⏳ **P11**: Más proveedores (Kimi, Antigravity, Copilot)
- ⏳ **P15**: GUI de escritorio (Tauri)

Por ahora: **v0.1 = Claude Code + Codex + TUI + handoff**.

---

## ¿Preguntas?

- Spec completo: [`docs/spec/idea.md`](docs/spec/idea.md)
- Flow de UX: [`docs/spec/Symphony_CLI_User_Flow_and_Views.html`](docs/spec/Symphony_CLI_User_Flow_and_Views.html)
- Plan: [`PLAN.md`](PLAN.md)
- Issues: crea un ticket en GitHub

¡Bienvenido a Symphony!
