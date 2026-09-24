# ADR-0002 · Capa de procesos: ProcessKit

- **Estado:** ACEPTADO
- **Fecha:** 2026-09-24
- **Autor:** claude-code/opus-5.5 · **Aprobado por:** — (sigue la recomendación de STACK §7.1/§60; el gate pasó)
- **Fase/paso:** P01.S7

## Contexto
STACK §60 pide un gate antes de acoplarse a ProcessKit: kill del árbol sin huérfanos, streaming estable, buen comportamiento en Windows/Linux/macOS, overhead bajo y límites predecibles. Si no los cumple, el plan B es `tokio::process` + `windows-sys` + `nix` + abstracción de PTY.

## Opciones consideradas
1. **ProcessKit 3.3** (features `limits`, `stats`) detrás del trait `ProcessSupervisor` (STACK §7.1).
2. **Plan B:** `tokio::process` + Job Objects propios (`windows-sys`) + `nix`/cgroups propios.

## Decisión
Opción 1. `crates/process` depende de `processkit = { version = "3.3", features = ["limits", "stats"] }` y **solo** ese crate lo conoce. El resto del core usa `ProcessSupervisor`. `pty` se activa cuando ADR-0005 lo requiera.

## Evidencia
`spikes/results/test-processkit.md`:
- Kill del árbol sin huérfanos en los 3 OS (Job Object en Windows, process group en Linux/macOS), también a través de un wrapper `cmd /c` / `sh -c`.
- Streaming de 200k líneas sin pérdidas y en orden en los 3 OS.
- Overhead de spawn igual al de `tokio::process` (dentro del ruido).
- Windows: límites de memoria, CPU (0.5 → 0.51–0.56 núcleos) y procesos efectivos. Suspend/resume funciona.
- Linux sin cgroup delegado y macOS: los límites devuelven errores tipados (`Unenforceable` / `Unsupported`), sin fallar en silencio.

## Consecuencias
- **P04.S3 (`crates/process`):** implementar `ProcessSupervisor` sobre `ProcessGroup` (un grupo por agente), resolviendo el ejecutable nativo en lugar de shims `.cmd` (Test A/B). No confiar en `LimitEvidence.memory`: inferir el OOM por exit code.
- **P08 (scheduler):** la capa 2 (OS) es fuerte en Windows. En Linux depende de cgroup v2 delegado (hay que verificarlo en una máquina real y degradar a scheduling si falta). En macOS solo hay scheduling (concurrencia, `suspend`, prioridad). La TUI tiene que mostrar qué garantías hay en cada OS (`ProcessGroup::mechanism()` + errores de límite).
- El plan B de STACK §60 no se activa. Si en P04 aparece un bloqueo (PTY, por ejemplo), se escribe un ADR nuevo.
