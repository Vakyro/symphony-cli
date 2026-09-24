--- corrida 1
| Mecanismo | — | JobObject |
| kill_all del árbol | ✅ | 10 vivos → 0 tras kill (534.2039ms) |
| drop del grupo vía cmd /c | ✅ | 11 vivos → 0 tras drop |
| streaming 200k líneas | ✅ | 200000/200000 en orden=true en 1.1218851s |
| overhead de spawn | ✅ | processkit 79.687756ms vs tokio 80.50453ms por proceso (-1 %) |
| max_memory 256 MB (pide 1 GB) | ✅ | bloqueado en 246.0223ms; pico Some(271) MB; evidencia LimitEvidence { memory: Unknown, processes: NotTripped, cpu: NotTripped } |
| cpu_quota None | ✅ | 3.80 núcleos efectivos (CPU 15.859375s / pared 4.1726204s) |
| cpu_quota Some(0.5) | ✅ | 0.56 núcleos efectivos (CPU 2.390625s / pared 4.302314s) |
| max_processes 4 (el árbol quiere 10) | ✅ | 1 procesos vivos |
| suspend/resume | ✅ | CPU suspendido +0ns, tras resume +812.5ms |
--- corrida 2
| Mecanismo | — | JobObject |
| kill_all del árbol | ✅ | 10 vivos → 0 tras kill (528.1764ms) |
| drop del grupo vía cmd /c | ✅ | 11 vivos → 0 tras drop |
| streaming 200k líneas | ✅ | 200000/200000 en orden=true en 1.1077004s |
| overhead de spawn | ✅ | processkit 79.551893ms vs tokio 65.809223ms por proceso (+21 %) |
| max_memory 256 MB (pide 1 GB) | ✅ | bloqueado en 269.2191ms; pico Some(271) MB; evidencia LimitEvidence { memory: Unknown, processes: NotTripped, cpu: NotTripped } |
| cpu_quota None | ✅ | 3.87 núcleos efectivos (CPU 16.3125s / pared 4.2158016s) |
| cpu_quota Some(0.5) | ✅ | 0.51 núcleos efectivos (CPU 2.359375s / pared 4.5813208s) |
| max_processes 4 (el árbol quiere 10) | ✅ | 1 procesos vivos |
| suspend/resume | ✅ | CPU suspendido +0ns, tras resume +796.875ms |

--- corrida 3 (Windows local, tras corregir el conteo)
| Mecanismo | — | JobObject |
| kill_all del árbol | ✅ | 10 vivos → 0 tras kill (540.8061ms) |
| drop del grupo vía cmd /c | ✅ | 11 vivos → 0 tras drop |
| streaming 200k líneas | ✅ | 200000/200000 en orden=true en 1.0933302s |
| overhead de spawn | ✅ | processkit 91.658816ms vs tokio 75.49554ms por proceso (+21 %) |
| max_memory 256 MB (pide 1 GB) | ✅ | bloqueado en 245.6658ms; pico Some(271) MB; evidencia LimitEvidence { memory: Unknown, processes: NotTripped, cpu: NotTripped } |
| cpu_quota None | ✅ | 3.82 núcleos efectivos (CPU 15.890625s / pared 4.1581935s) |
| cpu_quota Some(0.5) | ✅ | 0.56 núcleos efectivos (CPU 2.453125s / pared 4.4108028s) |
| max_processes 4 (el árbol quiere 10) | ✅ | 1 procesos vivos |
| suspend/resume | ✅ | CPU suspendido +0ns, tras resume +812.5ms |

--- CI gate (macos-latest) (107794668975)
| Prueba | Resultado | Detalle |
| Mecanismo | — | ProcessGroup |
| kill_all del árbol | ✅ | 10 vivos → 0 tras kill (510.787834ms) |
| drop del grupo vía cmd /c | ✅ | 10 vivos → 0 tras drop |
| streaming 200k líneas | ✅ | 200000/200000 en orden=true en 248.169416ms |
| overhead de spawn | ✅ | processkit 30.409248ms vs tokio 35.892597ms por proceso (-15 %) |
| max_memory 256 MB | ⚠️ no disponible | memory limit is not supported on this platform: resource limits require a cgroup or Job Object; unavailable on this target |
| cpu_quota None | ✅ | 0.00 núcleos efectivos (CPU 0ns / pared 4.096144875s) |
| cpu_quota Some(0.5) | ⚠️ no disponible | CPU limit is not supported on this platform: resource limits require a cgroup or Job Object; unavailable on this target |
| max_processes 4 | ⚠️ no disponible | process-count limit is not supported on this platform: resource limits require a cgroup or Job Object; unavailable on this target |
| suspend/resume | ⚠️ sin stats de CPU | suspend y resume no fallaron |

--- CI gate (windows-latest) (107794669097)
| Prueba | Resultado | Detalle |
| Mecanismo | — | JobObject |
| kill_all del árbol | ✅ | 10 vivos → 0 tras kill (506.495ms) |
| drop del grupo vía cmd /c | ✅ | 11 vivos → 0 tras drop |
| streaming 200k líneas | ✅ | 200000/200000 en orden=true en 492.8879ms |
| overhead de spawn | ✅ | processkit 40.554206ms vs tokio 39.624126ms por proceso (+2 %) |
| max_memory 256 MB (pide 1 GB) | ✅ | bloqueado en 935.7015ms; pico Some(271) MB; evidencia LimitEvidence { memory: Unknown, processes: NotTripped, cpu: NotTripped } |
| cpu_quota None | ✅ | 3.09 núcleos efectivos (CPU 15.125s / pared 4.8936602s) |
| cpu_quota Some(0.5) | ✅ | 0.52 núcleos efectivos (CPU 2.203125s / pared 4.204003s) |
| max_processes 4 (el árbol quiere 10) | ✅ | 1 procesos vivos |
| suspend/resume | ✅ | CPU suspendido +0ns, tras resume +796.875ms |

--- CI gate (ubuntu-latest) (107794669185)
| Prueba | Resultado | Detalle |
| Mecanismo | — | ProcessGroup |
| kill_all del árbol | ✅ | 10 vivos → 0 tras kill (513.686011ms) |
| drop del grupo vía cmd /c | ❌ | 11 vivos → 0 tras drop |
| streaming 200k líneas | ✅ | 200000/200000 en orden=true en 306.741866ms |
| overhead de spawn | ✅ | processkit 22.927714ms vs tokio 22.5384ms por proceso (+2 %) |
| max_memory 256 MB | ⚠️ no disponible | memory limit could not be enforced: Permission denied (os error 13) |
| cpu_quota None | ✅ | 0.00 núcleos efectivos (CPU 0ns / pared 4.085573279s) |
| cpu_quota Some(0.5) | ⚠️ no disponible | CPU limit could not be enforced: Permission denied (os error 13) |
| max_processes 4 | ⚠️ no disponible | process-count limit could not be enforced: Permission denied (os error 13) |
| suspend/resume | ⚠️ sin stats de CPU | suspend y resume no fallaron |
