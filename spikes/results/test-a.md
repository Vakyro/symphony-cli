# Test A · Recursos (P01.S3)

- **Fecha:** 2026-09-24 · **Agente:** claude-code/opus-5.5
- **Máquina:** Latitude 7390, Intel i7-8650U (4 núcleos / 8 hilos, 1.9 GHz), 15.9 GB RAM, Windows 11 Pro. Uso en reposo durante la prueba: ~6.7 GB.
- **CLIs:** Claude Code 2.1.281 (`--model haiku`) y codex-cli 0.154.0 (`-m gpt-5.6-luna`, `-s read-only`), con la configuración global de Leo (plugins, hooks y skills incluidos).
- **Herramienta:** `spikes/spike-resources`. Lanza N CLIs a la vez, cada uno en su worktree, y muestrea el árbol de procesos cada 500 ms con `sysinfo`. Salida cruda: [`test-a.raw.md`](test-a.raw.md).
- **Tarea:** trivial y de solo lectura ("Read package.json and reply with only the project name"). Mide el **costo fijo** de un agente, no el de un build. Los builds y tests se miden en P08.
- **Repo:** Vite + React + TS con pnpm, 3 worktrees (`git worktree add`).

## 1. CLIs

| Proveedor | N | Arranque hasta 1.ª salida (s) | Duración (s) | RSS pico por agente (MB) | RSS medio por agente (MB) | Procesos pico por agente | RAM del sistema sobre la base (MB) | CPU global media |
|---|---|---|---|---|---|---|---|---|
| Claude | 1 | 5.0 | 14.2 | 928 | 338 | 17 | +758 | 62 % |
| Claude | 2 | 7.1 | 19.6 | 418–606 | ~300 | 12–14 | +885 | 87 % |
| Claude | 3 | 18.1 | 23.8 | 371–413 | ~253 | 10 | +731 | 83 % |
| Codex | 1 | 1.6 | 25.0 | 287 | 211 | 12 | +180 | 56 % |
| Codex | 2 | 2.9 | 36.2 | 324–342 | ~220 | 8 | +464 | 72 % |
| Codex | 3 | 5.3 | 35.5 | 303–319 | ~207 | 11–15 | +373 | 80 % |

RSS = working set de Windows sumado sobre todo el árbol de procesos (incluye memoria compartida, así que la suma **sobreestima**). "RAM del sistema sobre la base" es el delta de memoria usada total: es la cifra más realista.

### Qué procesos hay en cada árbol (en el pico)

- **Claude:** `cmd.exe` (shim npm) → `node.exe` → `claude.exe`, más **`bash.exe`×4–8 + `conhost.exe`×3–5**. Los bash son los hooks de los plugins globales de Leo (ponytail, context-mode…) que corren en `SessionStart`/`UserPromptSubmit` de **cada** sesión.
- **Codex:** `cmd.exe` → `node.exe` → `codex.exe`, más **`git.exe`×3–11 y `git-remote-https.exe`**: Codex hace git (incluido un fetch de red) al arrancar en un repo. Con tools: `codex-command-runner.exe`, `codex-code-mode-host.exe`, `powershell.exe`.

## 2. Dependencias por worktree

| Estrategia | Tiempo | Disco |
|---|---|---|
| `pnpm install` en frío (store vacío) | 6.2 s | 51 MB en el store |
| `pnpm install --offline` con el store compartido (2.º y 3.º worktree) | **2.2 s** | ~0 extra (hardlinks al store) |
| `npm install` sin store compartido | 24.3 s | 51 MB por worktree |

(El "frío" de pnpm todavía tenía el caché de metadatos del registro, así que en una máquina nueva sería más lento.)

## 3. Conclusiones

1. **El costo fijo por agente es de ~200–350 MB de RAM real** (Codex ~200, Claude ~250–350), con picos de arranque de ~1 GB en Claude. Con 3 agentes, la suma queda en **~1 GB sobre la base**. En 16 GB esto es manejable: la RAM no es el cuello de botella con tareas livianas.
2. **La CPU sí lo es:** con 2–3 agentes y una tarea trivial, la CPU global media ya sube a 72–87 % en 4 núcleos. Casi todo es arranque (node, hooks, git). Esto confirma IDEA §3: lo que traba la máquina son los procesos paralelos, y hace falta el scheduler (P08) antes de sumar builds.
3. **El arranque se degrada con la concurrencia:** Claude pasa de 5 s a 18 s hasta la primera salida con 3 en paralelo. Symphony debería **escalonar los arranques** (clase de operación de arranque en el scheduler) en vez de lanzar todo a la vez.
4. **La configuración global del usuario pesa:** los hooks de plugins globales (bash×4–8 por sesión de Claude) y las skills cargadas (H6: Codex usó ~151k tokens de entrada en una tarea trivial) se multiplican por agente. El adapter debería poder **aislar** esa configuración por agente cuando Leo quiera (Claude: `--setting-sources`; Codex: `--ignore-user-config`). Medirlo en P08.
5. **Los shims `.cmd` de npm suman `cmd.exe` + `conhost.exe` por agente** y obligan a escapar argumentos como batch. P04 debería resolver el ejecutable real (`node` + script, o `claude.exe` / `codex.exe`) en lugar del `.cmd`.
6. **pnpm con store compartido** hace barata la estrategia de dependencias por worktree (2.2 s, sin disco extra). Esto respalda STACK §12.2 para P04.S2.

## 4. Límites

- Una sola corrida por combinación, tarea trivial y 500 ms de muestreo: los picos cortos pueden perderse.
- La suma de RSS por árbol cuenta dos veces la memoria compartida.
- No se midió con builds, tests ni MCP pesados. Eso queda para el benchmark de P08.

## 5. Hallazgo aparte: rutas cortas de Windows

La primera corrida de Codex usó un worktree bajo `%TEMP%`, que en esta máquina resuelve al nombre corto `C:\Users\LATITU~1\…`. Codex falló al leer archivos, reintentó varias veces y tardó 89 s en vez de 25 s. **Symphony tiene que pasar a los CLIs rutas largas y canónicas**, sin nombres 8.3 y sin el prefijo `\\?\`.
