# Uso remoto por SSH

Puedes supervisar tus agentes, revisar diffs, cambiar de modelo o conversar en el chat de Symphony **desde el teléfono móvil** (o desde otra computadora) sin escribir ni instalar código adicional en Symphony.

## Por qué funciona sin cambios en Symphony

Symphony está dividido en dos procesos:
- **`symphonyd` (el daemon por proyecto):** mantiene vivos los agentes, los worktrees, la base de datos SQLite y los checkpoints en `<proyecto>/.symphony/` en tu computadora principal (el *host*).
- **`symphony` (la TUI en `ratatui` y el CLI):** es un cliente ligero de terminal que habla con `symphonyd` por el socket local del proyecto.

Gracias a este desacoplamiento (y a la regla de diseño de no exponer un servidor HTTP en v1), basta con conectarse al equipo *host* mediante **Tailscale + SSH + `tmux`** para abrir la misma TUI desde un cliente SSH móvil como **Termius**, **Prompt** o **Termux**.

---

## Arquitectura del flujo

```text
[ Teléfono móvil ]
  Termius / Prompt / Termux
  + Tailscale (red privada)
         │
         │ SSH (cifrado extremo a extremo)
         ▼
[ Equipo Host (Windows/WSL, Linux o macOS) ]
  Sesión persistente de tmux
         │
         ├──► `symphony` (TUI en ratatui / comandos CLI)
         │          │
         │          ▼ (IPC local en <proyecto>/.symphony/run/)
         └──► `symphonyd` ──► Agentes (Claude Code, Codex, Kimi, …)
```

---

## Paso 1: Preparar el equipo host

1. **Instala y activa Tailscale** en tu computadora donde corre Symphony e inicia sesión en tu cuenta. Anota el nombre o la IP privada de Tailscale del equipo (`100.x.y.z`).
2. **Habilita el servidor SSH** en tu sistema:
   - **Linux:** `openssh-server` activo (`sshd`).
   - **macOS:** activa *Sesión remota* (*Remote Login*) en la configuración del sistema.
   - **Windows:** activa el servidor OpenSSH de Windows o el de tu distribución WSL, asegurándote de que la sesión entre al mismo entorno donde tienes instalados `symphony`, `symphonyd` y los CLIs de los proveedores.
3. **Instala `tmux`** (o `screen`) para mantener la terminal viva aunque el teléfono cambie de red Wi-Fi a datos móviles o bloquee la pantalla.

---

## Paso 2: Iniciar la sesión persistente en tu proyecto

En tu computadora (o directamente al entrar por SSH), abre una sesión de `tmux` con nombre dentro de la carpeta del proyecto:

```bash
cd /ruta/a/tu/proyecto
tmux new -A -s symphony
```

El flag `-A` crea la sesión `symphony` si no existe o se adjunta a ella si ya estaba abierta. Dentro de `tmux`, lanza la TUI:

```bash
symphony
```

Puedes dejar esa ventana abierta o desacoplarte de `tmux` con `Ctrl+B` seguido de `d`: tanto `tmux` como `symphonyd` seguirán ejecutándose.

---

## Paso 3: Conectarte desde el teléfono

1. Instala **Tailscale** en tu teléfono y conéctalo a la misma cuenta.
2. Abre tu cliente SSH móvil (**Termius**, **Prompt** en iOS o **Termux** en Android) y conéctate a tu usuario en la IP o nombre de Tailscale del equipo *host*:
   ```bash
   ssh tu-usuario@nombre-de-tu-host
   ```
3. Entra a la sesión de `tmux`:
   ```bash
   tmux attach -t symphony
   ```

Verás la TUI de Symphony tal como estaba en tu computadora:
- En la vista **Chat**, puedes leer lo que está haciendo el modelo, enviar mensajes nuevos, cambiar de proveedor o integrar a `main` (`Ctrl+G` / `Ctrl+R`).
- En **Home** y en la **vista de agente**, puedes crear agentes (`n`), ver el `git diff` (`d`), cambiar de modelo (`s`), enviar instrucciones (`m`) o aceptar sugerencias de modelo (`S`/`K`/`N`).

---

## Uso rápido por comandos (en pantallas pequeñas)

Si en algún momento prefieres no usar la TUI completa en la pantalla vertical del teléfono, puedes abrir otra ventana de `tmux` (`Ctrl+B` luego `c`) en la carpeta del proyecto y usar los subcomandos directos del CLI:

```bash
cd /ruta/a/tu/proyecto

# Ver el estado de todos los agentes y proveedores
symphony agents
symphony providers

# Leer los últimos mensajes o el diff de un agente
symphony logs 1 --limit 10
symphony diff 1

# Enviar una instrucción o cambiar de modelo/profile
symphony send 1 "Continúa con las pruebas unitarias"
symphony switch 1 --profile @fast

# Revisar e integrar el chat general en main
symphony chat merge
symphony chat merge --review
symphony chat merge --yes
```

---

## Qué ocurre si se corta la conexión del teléfono

- **`tmux` conserva tu terminal:** cuando vuelvas a tener señal y ejecutes `tmux attach -t symphony`, la TUI seguirá exactamente en la misma pantalla.
- **`symphonyd` nunca depende de la TUI:** aunque cierres la TUI con `q` o se apague el cliente SSH, el daemon del proyecto sigue supervisando los procesos de los agentes, aplicando los límites de recursos y guardando los checkpoints incrementales en `<proyecto>/.symphony/`.

> Nota sobre notificaciones móviles (ADR-0014): en v1 el acceso remoto se realiza íntegramente por SSH + `tmux` sin componentes adicionales ni tráfico de red en el core; un *bridge* opcional de notificaciones push para avisar en el teléfono cuando un agente termine o se bloquee queda reservado para v1.1.
