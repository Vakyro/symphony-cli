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
