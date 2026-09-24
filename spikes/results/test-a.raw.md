

### claude N=1

Error: Os { code: 267, kind: NotADirectory, message: "El nombre del directorio no es válido." }

### claude N=2

| Proveedor | N | Worktree | 1.ª salida (s) | Duración (s) | RSS pico (MB) | RSS medio (MB) | CPU pico (%) | CPU media (%) | Procesos pico | Exit |
|---|---|---|---|---|---|---|---|---|---|---|
| claude | 2 | wt1 | 7.1 | 19.6 | 418 | 295 | 149 | 50 | 14 | 0 |
| claude | 2 | wt2 | 7.1 | 19.0 | 606 | 300 | 165 | 51 | 12 | 0 |

Total N=2: RSS pico sumado de los árboles 1006 MB · RAM usada del sistema +885 MB sobre la base · CPU global media 87% · duración 19.6 s

Procesos de C:\Users\Latitude 7390\symphony-spike\wt1 en el pico: bash.exe×4, claude.exe×1, cmd.exe×1, conhost.exe×4, git.exe×1, node.exe×3

Procesos de C:\Users\Latitude 7390\symphony-spike\wt2 en el pico: bash.exe×4, claude.exe×1, cmd.exe×1, conhost.exe×3, node.exe×3

### claude N=3

| Proveedor | N | Worktree | 1.ª salida (s) | Duración (s) | RSS pico (MB) | RSS medio (MB) | CPU pico (%) | CPU media (%) | Procesos pico | Exit |
|---|---|---|---|---|---|---|---|---|---|---|
| claude | 3 | wt1 | 18.1 | 23.3 | 369 | 225 | 101 | 27 | 10 | 0 |
| claude | 3 | wt2 | 18.1 | 23.8 | 371 | 254 | 70 | 24 | 10 | 0 |
| claude | 3 | wt3 | 18.1 | 22.1 | 413 | 252 | 100 | 27 | 10 | 0 |

Total N=3: RSS pico sumado de los árboles 1090 MB · RAM usada del sistema +731 MB sobre la base · CPU global media 83% · duración 23.8 s

Procesos de C:\Users\Latitude 7390\symphony-spike\wt1 en el pico: claude.exe×1, cmd.exe×1, conhost.exe×3, git.exe×5

Procesos de C:\Users\Latitude 7390\symphony-spike\wt2 en el pico: claude.exe×1, cmd.exe×1, conhost.exe×3, git.exe×5

Procesos de C:\Users\Latitude 7390\symphony-spike\wt3 en el pico: bash.exe×4, claude.exe×1, cmd.exe×1, conhost.exe×3, node.exe×1

### codex N=1

Error: Os { code: 267, kind: NotADirectory, message: "El nombre del directorio no es válido." }

### codex N=2

| Proveedor | N | Worktree | 1.ª salida (s) | Duración (s) | RSS pico (MB) | RSS medio (MB) | CPU pico (%) | CPU media (%) | Procesos pico | Exit |
|---|---|---|---|---|---|---|---|---|---|---|
| codex | 2 | wt1 | 2.9 | 36.2 | 342 | 221 | 158 | 19 | 8 | 0 |
| codex | 2 | wt2 | 2.9 | 35.1 | 324 | 220 | 146 | 24 | 8 | 0 |

Total N=2: RSS pico sumado de los árboles 599 MB · RAM usada del sistema +464 MB sobre la base · CPU global media 72% · duración 36.2 s

Procesos de C:\Users\Latitude 7390\symphony-spike\wt1 en el pico: cmd.exe×1, codex.exe×1, git-remote-https.exe×1, git.exe×3, node.exe×2

Procesos de C:\Users\Latitude 7390\symphony-spike\wt2 en el pico: cmd.exe×1, codex-code-mode-host.exe×1, codex-command-runner.exe×1, codex.exe×1, conhost.exe×1, node.exe×2, powershell.exe×1

### codex N=3

| Proveedor | N | Worktree | 1.ª salida (s) | Duración (s) | RSS pico (MB) | RSS medio (MB) | CPU pico (%) | CPU media (%) | Procesos pico | Exit |
|---|---|---|---|---|---|---|---|---|---|---|
| codex | 3 | wt1 | 5.3 | 35.5 | 303 | 208 | 97 | 18 | 13 | 0 |
| codex | 3 | wt2 | 5.3 | 35.5 | 305 | 205 | 91 | 18 | 11 | 0 |
| codex | 3 | wt3 | 5.3 | 33.3 | 319 | 209 | 145 | 17 | 15 | 0 |

Total N=3: RSS pico sumado de los árboles 758 MB · RAM usada del sistema +373 MB sobre la base · CPU global media 80% · duración 35.5 s

Procesos de C:\Users\Latitude 7390\symphony-spike\wt1 en el pico: cmd.exe×1, codex.exe×1, git-remote-https.exe×1, git.exe×9, node.exe×1

Procesos de C:\Users\Latitude 7390\symphony-spike\wt2 en el pico: cmd.exe×1, codex.exe×1, git-remote-https.exe×1, git.exe×7, node.exe×1

Procesos de C:\Users\Latitude 7390\symphony-spike\wt3 en el pico: cmd.exe×1, codex.exe×1, git-remote-https.exe×1, git.exe×11, node.exe×1

### claude N=1 (repetición)

| Proveedor | N | Worktree | 1.ª salida (s) | Duración (s) | RSS pico (MB) | RSS medio (MB) | CPU pico (%) | CPU media (%) | Procesos pico | Exit |
|---|---|---|---|---|---|---|---|---|---|---|
| claude | 1 | wt1 | 5.0 | 14.2 | 928 | 338 | 211 | 69 | 17 | 0 |

Total N=1: RSS pico sumado de los árboles 928 MB · RAM usada del sistema +758 MB sobre la base · CPU global media 62% · duración 14.2 s

Procesos de C:\Users\Latitude 7390\symphony-spike\wt1 en el pico: bash.exe×8, claude.exe×1, cmd.exe×1, conhost.exe×5, node.exe×2

### codex N=1 (repetición)

| Proveedor | N | Worktree | 1.ª salida (s) | Duración (s) | RSS pico (MB) | RSS medio (MB) | CPU pico (%) | CPU media (%) | Procesos pico | Exit |
|---|---|---|---|---|---|---|---|---|---|---|
| codex | 1 | wt1 | 1.6 | 25.0 | 287 | 211 | 161 | 20 | 12 | 0 |

Total N=1: RSS pico sumado de los árboles 287 MB · RAM usada del sistema +180 MB sobre la base · CPU global media 56% · duración 25.0 s

Procesos de C:\Users\Latitude 7390\symphony-spike\wt1 en el pico: cmd.exe×1, codex.exe×1, git-remote-https.exe×1, git.exe×8, node.exe×1
