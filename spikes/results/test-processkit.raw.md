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
