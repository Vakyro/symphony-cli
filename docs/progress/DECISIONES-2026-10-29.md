# Decisiones de Leo — 2026-10-29

Respuestas de Leo a las 53 preguntas de proceso y de fase, recogidas en una sesión de OpenCode y documentadas aquí. Rama de referencia: `phase/p17-tui`.

Convención: **Decidido** = respuesta clara. **Interpretado** = respuesta ambigua, con la lectura que tomo. **Confirmar** = Leo tiene que aclararlo antes de actuar.

---

## A. Proceso (afecta a todo)

| # | Tema | Decisión |
|---|---|---|
| 1 | Orden de fases | **Decidido:** P17 → P18 → P19 → P20 → cierre de P16 (v1.0). |
| 2 | Merge a main | **Decidido:** mergear cada fase a `main` al cerrarla con gate en verde, sin esperar OK. |
| 3 | Tags `pNN-done` | **Decidido:** crear y pushear el tag al cerrar cada fase. |
| 4 | Modelos para delegar | **Decidido:** repartir el trabajo entre proveedores según cuota disponible, sin sobrecargar a ninguno, conservando calidad. Estado de cuotas que dio Leo: **Claude Code** y **Antigravity con modelos Claude/GPT** agotados; **Copilot**, **Antigravity con modelos Gemini**, **Codex** y **Kimi** en buen estado; **OpenCode** a la mitad semanal pero al 100 % por sesión. Reparto: Codex, Kimi, Antigravity-Gemini y Copilot como base equilibrada; OpenCode solo como reserva; gpt-6-sol / gpt-6-astra solo si el modelo base no llega. |
| 5 | Gate `cargo xtask check` (~310 s) | **Decidido:** criterio mío. Se corre al cerrar cada fase; entre pasos grandes, a discreción. |
| 6 | Tests live (`SYMPHONY_LIVE=1`) | **Decidido:** autorizados. Se pueden ejecutar cuando haga falta. |

## B. P17 · TUI (fase en curso)

### P17.S4 · Mascota que «piensa»
- **7. Dónde:** solo en el chat principal. **No** es un fuego artificial (ese era solo el ejemplo). La mascota es un **mini piano de una octava** cuyas teclas se presionan solas, como si el agente estuviera trabajando.
- **8. Frases:** con personalidad (tipo «pondering», «cogitating»), no frases neutras.
- **9. Animación:** el tick actual de 1 s se mantiene. No hace falta un timer nuevo.
- **10. Desactivable:** **configurable**, con variable de entorno `SYMPHONY_NO_ANIM` y opción en la config. La config vive en **`~/.symphony` (usuario) con override por proyecto en `.symphony`** (ver sección «Segunda ronda»).

### P17.S5 · Shell en curso
- **11. Alcance:** solo procesos lanzados por los agentes (tests, builds). **No** una shell interactiva.
- **12. Salida:** solo lectura de stdout/stderr en streaming. **No** envío de stdin.
- **13. Dónde:** pestaña nueva dentro de la vista del agente.
- **14. IPC:** **decisión delegada a mí.** Criterio: añadir `agent.processes` + streaming solo si el streaming es necesario para la vista en vivo; si basta con los logs persistidos, leer esos. Lo decido al implementar y lo registro en la bitácora de S5.

### P17.S6 · Vistas avanzadas
- **15. Datos entre proyectos (ADR-0018):** pantalla de status **global** (mezcla todos los proyectos). Desbloquea este paso.
- **16. Heatmap mensual:** estilo GitHub (días × semanas, intensidad por tokens).
- **17. Ramas de git:** listar con estado **y** acciones. **Segunda ronda:** cambiar de rama sin confirmación; borrar siempre con confirmación, y nunca forzar el borrado de ramas no mergeadas sin preguntar.

### P17.S7 · Cierre
- **18.** P17 se cierra con merge a `main` y tag `p17-done` **cuando esté S8 terminado**. No antes.

### P17.S8 · Renderizado rico del chat
- **19. Opción B:** solo colorear lo que el agente marque. Todo el texto en blanco salvo títulos, bullets, elementos a destacar, tablas, etc., con colores distintos.
- **20. Quién asigna el color:** **marcadores del agente** (segunda ronda). La TUI solo pinta lo marcado.
- **21. Persistencia:** también en el transcript, no solo en memoria.

## C. P16 · Validación final y release

- **22. Benchmark (41 s):** Leo no quiere publicar hasta terminar todo. La v1 se publica **al final** del plan, no tras el benchmark. **Interpretado:** no hay que bajar el benchmark antes de publicar; la publicación espera a que termine P20.
- **23. Release:** primero `v0.9.0`, luego `v1.0.0`.
- **24. Tags `v*`:** Leo los pushea él mismo **con su OK escrito en el chat**. Yo no pusheo tags `v*` sin ese OK.
- **25. Homebrew:** **Decidido:** crear el repo `Vakyro/homebrew-symphony` para el tap (confirmado en segunda ronda).
- **26. Docs as-built (S5):** **decisión delegada a mí.**

## D. P18 · Contexto y ahorro de tokens

- **27.** Éxito = **40 %** de reducción de tokens, medido contra un **benchmark fijo del repo** (mismas tareas antes y después de P18).
- **28.** Flintstone **activado por defecto**, con opción de apagarlo.
- **29. Sesiones cortas:** para agentes, el límite es **cada 3 tareas**; `/compact` se dispara **automáticamente** al superarlo. **Decidido (segunda ronda):** se implementa **ahora, en P18**, aunque CLAUDE.md marque `/compact` (ADR-0013 D2) como no implementado. Luego se mide en vivo y se ajusta. Medición L3 = gasta cuota, así que requiere tu OK por separado.
- **30. Confirmación de sesión larga:** prompt en la TUI.
- **31. Graphify:** se ejecuta **al iniciar**.
- **32. Segundo cerebro:** los markdown van dentro del repo, en `.symphony/brain/`. Se **ignoran** en git (no se commitean).

## E. P19 · Orquestación

### P19.S1 · Modelo de decisión (System One)
- **33. Backend por defecto:** **Laya**, por ser el más popular.
- **34. Personalización:** **confirmada.** El usuario conecta el backend que tenga instalado (Jev, open-source o propio) vía config. Symphony define solo el contrato.
- **35. Umbrales de confianza:** **confirmados:** > 0.9 actúa solo; 0.6–0.9 pide confirmación; < 0.6 escala al LLM grande.
- **36. Proceso Python aparte vs. 100 % Rust:** **decisión delegada a mí.** Lo que resulte mejor técnicamente, documentando el motivo en el ADR.

### P19.S2–S5 · Perfiles, planeador, delegación, `/orchester`
- **37. Perfiles:** los decide **Jev** (coding / reasoning / fast / cheap / auto). Symphony no fija a mano qué modelos entran en cada perfil.
- **38. Planeador:** el plan por fases se genera **cuando el usuario lo pide, o cuando se detecta un trabajo grande** (no un cambio simple).
- **39. Nombre del comando:** **`/orchester`** (en inglés, consistente con el resto del proyecto).
- **40. Effort:** **tres niveles** (barato / producido / razonado) bastan.

### P19.S6 · Paridad con Claude Code
- **41.** Prioridad: que el agente del chat **edite archivos, codifique bien y responda bien**.

## F. P20 · Editor y movilidad

- **42. Editor:** **edición manual con guardado** (no solo lectura).
- **43. Atajos:** los de nano tal cual (`^O` guardar, `^X` salir, `^K` cortar línea).
- **44. Vista Habbo:** **fuera del plan**. Se documenta como «puede que se haga en el futuro».
- **45. Móvil:** **confirmado.** La guía Tailscale + SSH + tmux basta; no hace falta nada más.

## G. Ideas futuras (backlog)

- **46. Cooperativo en línea:** solo como **idea documentada**, no como fase formal. Modelo de cuotas **compartidas (pooled)**.
- **47. Política configurable del harness:** alcance que incluye **archivos, shell y git**. **Decidido (segunda ronda):** modo por defecto **lista blanca + aprobación**. Comandos conocidos (tests, build, `git status/diff`) pasan solos; lo demás pide confirmación; lo destructivo (`push --force`, `rm -rf`, `reset --hard`) pide confirmación siempre.
- **48. Agentes especializados:** roles **code, research, chore, review**. Leo los ve bien por ahora.
- **49. Ingeniería de grafos:** el grafo lo **genera el planeador dinámicamente** en cada ejecución; no es un grafo fijo por tipo de tarea.
- **50. Compaction con Jev:** debe preservar **decisiones tomadas, constraints, archivos tocados y plan vigente**.
- **51. Merge vs. rebase:** **decisión delegada a mí.** Criterio que maximice que los agentes lo apliquen bien.

## H. Confirmaciones

- **52.** Leo confía en los snapshots y tests de la TUI; no revisa la TUI a mano.
- **53.** Siguiente acción: **P17.S4** (mascota), según la lista propuesta.

---

## Segunda ronda (2026-10-29)

Respuestas a las preguntas que quedaban abiertas:

- **Cuotas (4):** estado dado por Leo, detallado en la fila 4.
- **Política harness (47):** lista blanca + aprobación.
- **Línea base 40 % (27):** benchmark fijo del repo.
- **Config (10 y ajustes P17–P18):** `~/.symphony` con override por proyecto en `.symphony`.
- **Color del chat (20):** marcadores del agente.
- **`/compact` (29):** implementar ya en P18.
- **Homebrew (25):** crear `Vakyro/homebrew-symphony`.
- **Ramas git (17):** cambiar sin confirmar; borrar siempre con confirmación.

**No hay pendientes abiertos de Leo.** Solo queda la medición L3 de `/compact`, que requiere su OK por separado.

## Decisiones delegadas a mí (con criterio documentado al implementar)

- 14: IPC de procesos (streaming vs. logs persistidos).
- 26: formato de docs as-built de P16.S5.
- 36: backend de decisión en proceso aparte vs. in-process.
- 51: documentar merge vs. rebase en mdBook o como skill.
