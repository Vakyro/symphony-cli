# P16 · Validación final y v1.0.0

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p16-validacion (rebasada sobre `main` el 2026-10-28, después del merge de P13) |
| Inicio / cierre | 2026-10-27 / — |
| Agentes que trabajaron | claude-code/opus-5.5 (S0) |
| Tag | — |
| Docs usados | ADR-0020, ADR-0021, DB §3.H, PLAN § P16 |

## Pasos

### P16.S0 · Validación por tipo de proyecto — ✅
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-10-27 (noche, dogfooding)
- **Qué se hizo:** ADR-0021 (aceptado por delegación de Leo) y check `files`: en proyectos
  `document` y `media` el tier 1 comprueba que cada archivo cambiado existe y se abre, por
  extensión y sin dependencias nuevas (texto UTF-8, JSON/TOML con lo que ya está en el
  workspace, firmas de PNG/JPEG/GIF/WebP/PDF con `%%EOF`/zip ofimático/MP3/WAV/MP4/WebM/OGG/
  FLAC). Un proyecto así tiene gate aunque no defina `[validation]` (gate en el tier 1). Lo
  desconocido no se juzga; un texto de más de 32 MB tampoco (`ponytail:`).
- **Archivos clave:** `migrations/009_files_check.sql`, `crates/core/src/{enums,config}.rs`,
  `crates/daemon/src/{files_check,validation}.rs`.
- **Cómo se verificó:** tests de `files_check` (firmas, vacíos, PDF truncado, borrados),
  `migration_009_keeps_checks_and_accepts_files`, y L2 con fake-agent:
  `a_document_project_gates_on_files_that_open` (PDF completo → COMPLETED, check PASS) y
  `a_truncated_pdf_fails_the_files_check` (PDF sin `%%EOF` → el agente recibe el motivo y
  la tarea queda BLOCKED). Store y core: 143 tests verdes.
- **Pendiente / notas:** sin archivos que revisar el check es PASS (no SKIPPED, que el merge
  muestra como «un check no pudo correr»); el ADR lo dice así.

### P16.S1 · Checklist de aceptación — ✅ (revisar los 🟡)
- **Agente:** `google/gemini-3.8-flash-medium` y, tras la revisión, `google/gemini-4-argon-medium` · **Fecha:** 2026-10-27
- **Qué se hizo:** `docs/phases/P16-acceptance.md`: 145 elementos (36 vistas, 5 journeys, 10 reglas UX, 11 principios, 44 tablas, 10 consultas, 29 comandos) con su evidencia. 143 ✅, 2 🟡 (vista 31 Settings en la TUI; subcomando `symphony plan`), 1 ⛔ (journeys live).
- **Cómo se verificó:** la primera versión citaba 37 funciones que no existían; se devolvió con la lista. En la segunda, un script comprobó que las 188 referencias `archivo::función` existen (la única que no, `route_explain`, era el método IPC `route.explain` y se corrigió a mano) y que los snapshots citados están en `crates/tui/tests/snapshots/`.

### P16.S2 · Pruebas E2E — 🟡
- La suite L2 con fake-agents (journeys A–E, failover por cuota simulado, forced kill, conflicto
  de merge, recuperación tras reinicio del daemon y soak de 4 h) corre en CI en los tres
  sistemas dentro de `cargo xtask check`. Falta la sesión live de Leo con los proveedores reales.

### P16.S3 · Benchmark final — 🟡 (medido; falta el veredicto de Leo)
- El harness E2E de STACK §25.2 quedó completo en P13.S4 (`crates/daemon/tests/benchmark.rs`).
  **Corrida final del 2026-10-28** (`cargo nextest run -p symphony-daemon --test benchmark
  --no-capture`, 41 s, sin otros agentes, 112 GB libres):

| Escenario | Agentes | Tiempo | RAM daemon | Arranque | Cola p95 | Evento p95 | Evento p99 | Checkpoint p95 |
|---|---|---|---|---|---|---|---|---|
| 1 agente | 1 | 1 827 ms | 31 MB | 625 ms | — | 20 ms | 20 ms | 788 ms |
| 3 agentes / 3 worktrees | 3 | 2 707 ms | 32 MB | 1 522 ms | — | 8 ms | 10 ms | 976 ms |
| 3 agentes + prueba pesada | 3 | 8 572 ms | 33 MB | 1 574 ms | 3 691 ms | 10 ms | 52 ms | 671 ms |
| failover forzado | 1 | 2 783 ms | 34 MB | 684 ms | — | 13 ms | 25 ms | 1 271 ms |
| reinicio del daemon | 2 | 4 850 ms | 34 MB | 23 ms | — | 2 ms | 34 ms | 1 309 ms |
| logs grandes (4 MB) | 1 | 4 530 ms | 57 MB | 689 ms | — | 275 ms | 313 ms | 777 ms |
| 10k eventos | 1 | 419 ms | 38 MB | — | 9 ms | 0,08 ms | 0,08 ms | — |
| 100k eventos | 1 | 4 176 ms | 53 MB | — | 12 ms | 0,06 ms | 0,08 ms | — |

  Handoff 441–823 ms; el failover forzado tarda 207 ms. Todos los topes del propio benchmark se
  cumplen (RAM < 100 MB, ninguna prueba pesada solapada, ningún escenario > 2 min).
- **Contra la línea base de P08** (`docs/phases/P08-multiagente.md`): el daemon bajo carga sigue en
  **32 MB** (idéntico) y el evento p95 sigue entre 8 y 20 ms; 100k eventos se escriben y se leen en
  4,2 s con p99 de 0,08 ms. Lo que creció es lo esperado por lo añadido después de P08 (índice de
  contexto, checkpoints, validación): `logs grandes` es el escenario más caro en RAM (57 MB) por el
  pipeline de salida.
- **Añadido en esta vuelta:** el benchmark declara antes de arrancar que necesita **≥ 5 GB libres**
  (`resources::disk_free`, ahora público) y falla con la cifra y qué hacer, en vez de morir a medias
  como pasó con 1,8 GB el 2026-10-27.
- **Falta:** el veredicto de Leo sobre la usabilidad (⛔): la corrida L3 con los seis proveedores
  reales de P08 no se repitió aquí a propósito (gasta cuota y solo Leo la autoriza).

### P16.S4 · Instalación limpia — 🟡 (falta su primera corrida en CI)
- **Agente:** claude-code/opus-5.5 · **Fecha:** 2026-10-27
- **Qué se hizo:** `.github/workflows/install-check.yml`: tras cada CI verde (o a mano con el
  id del run) baja los artefactos `dist-*` del dry run de P13.S7, los instala fuera del
  checkout (los dos binarios juntos) y prueba `symphony --version`, `symphonyd --version` y
  un Journey A en un proyecto nuevo (`init`, `daemon start`, `status`, `providers`, `spawn`,
  `agents`, `daemon stop`) con un `claude` falso (`fake-agent`) como único proveedor. Los
  instaladores shell/PowerShell de dist bajan de un GitHub Release: se prueban al publicar.
- **Cómo se verificó:** el mismo Journey A local con los binarios de `dist build` de Windows
  (agente #1 creado, `READY`).

### P16.S5 · Documentación as-built — 🟡
- `docs/spec/desviaciones.md`: anexo de lo que cambió respecto al spec y por qué, con sus ADRs.
  Falta README final y CHANGELOG (se hacen con el tag).
