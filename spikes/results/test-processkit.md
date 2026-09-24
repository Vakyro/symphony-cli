# Gate de ProcessKit (P01.S7, STACK §60)

- **Fecha:** 2026-09-24 · **Agente:** claude-code/opus-5.5
- **Crate:** `processkit 3.3.4` con features `limits` + `stats`.
- **Spike:** `spikes/spike-process`. Carga: árboles de `node` (1 raíz → 3 hijos → 2 nietos cada uno = 10 procesos).
- **Dónde:** laptop de Leo (Windows 11) ×3 corridas + workflow [`spike-processkit`](../../.github/workflows/spike-processkit.yml) en GitHub Actions (ubuntu, windows, macos). Salida cruda: [`test-processkit.raw.md`](test-processkit.raw.md).

## Resultados

| Criterio de STACK §60 | Windows (Job Object) | Linux runner (process group, sin cgroup delegado) | macOS (process group) |
|---|---|---|---|
| **Kill del árbol sin huérfanos** (`kill_all`) | ✅ 10 → 0 en ~0.5 s | ✅ 10 → 0 | ✅ 10 → 0 |
| **Drop del grupo a través de un wrapper** (`cmd /c` / `sh -c`) | ✅ 11 → 0 | ✅ 11 → 0 | ✅ 10 → 0 |
| **Streaming estable** (200k líneas, en orden) | ✅ ~1.1 s | ✅ ~0.3 s | ✅ ~0.3 s |
| **Overhead de spawn** vs `tokio::process` | ✅ −11 % a +21 % (ruido; ~70–80 ms por `node`) | ✅ ±15 % | ✅ +8 % |
| **Límite de memoria** (256 MB, el hijo pide 1 GB) | ✅ bloquea en ~0.2 s (pico 271 MB) | ⚠️ `Unenforceable` (sin permiso para cgroup) | ⚠️ `Unsupported` |
| **Límite de CPU** (0.5 núcleos, carga de 4 hilos) | ✅ 0.51–0.56 núcleos efectivos (sin límite: 3.8–3.9) | ⚠️ `Unenforceable` | ⚠️ `Unsupported` |
| **Límite de procesos** (4, el árbol quiere 10) | ✅ el árbol no pasa de 4 | ⚠️ `Unenforceable` | ⚠️ `Unsupported` |
| **Suspend / resume** | ✅ CPU congelado (+0 ns) y se reanuda | ⚠️ no falla, pero sin estadísticas de CPU para comprobarlo | ⚠️ ídem |

(Las filas ❌ de la primera corrida en CI eran errores de medición del spike, no de ProcessKit: en Linux, `sysinfo` también lista hilos y zombies, y el wrapper `sh -c` lleva el marcador. Están corregidos.)

## Conclusiones

1. **Pasa el gate en lo esencial.** Kill del árbol sin huérfanos, streaming y overhead están bien en los 3 OS. En Windows, que es la máquina de Leo y el caso más difícil, **todos** los límites funcionan con Job Objects y son predecibles (0.5 núcleos → 0.51–0.56).
2. **Los límites en Linux dependen de cgroup v2 delegado.** En un runner de GitHub no hay delegación y ProcessKit lo **informa con un error tipado** (`ResourceLimit { reason: Unenforceable }`), sin degradarse en silencio. En un escritorio Linux con systemd, la delegación del usuario normalmente existe. Falta verificarlo en una máquina real: pendiente para P08.
3. **macOS no tiene límites duros**, como ya anticipaba STACK §7.2. Symphony tiene que limitar ahí por scheduling: menos concurrencia, `suspend`, prioridad.
4. **Detalles a tener en cuenta en P04:**
   - `LimitEvidence.memory` devuelve `Unknown` aunque el límite sí bloqueó. Para saber que un proceso murió por memoria hay que inferirlo (exit code / stderr), no confiar en la evidencia.
   - Con `max_processes`, el hijo que no puede hacer spawn puede morir. En la prueba quedó 1 proceso de 4 permitidos. El límite se cumple, pero el efecto sobre la herramienta del agente depende de cómo maneje el error.
   - Las features `limits` y `stats` están desactivadas por defecto. `ProcessGroup::output_string` es del trait `ProcessRunner` (hay que importarlo).

**Decisión propuesta:** ProcessKit detrás de `ProcessSupervisor` (ADR-0002).
