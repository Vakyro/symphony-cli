# P16 · Validación final y v1.0.0

| Campo | Valor |
|---|---|
| Estado | EN CURSO |
| Rama | phase/p16-validacion (sale de `phase/p13-endurecimiento` antes de su merge; se rebasa sobre `main` al cerrar P13) |
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

### P16.S3 · Benchmark final — ⏳
- El harness E2E de STACK §25.2 quedó completo en P13.S4 (`crates/daemon/tests/benchmark.rs`).
  Falta correrlo con la máquina sin otros agentes y compararlo con la línea base de P08, y el
  veredicto de Leo sobre la usabilidad.

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
