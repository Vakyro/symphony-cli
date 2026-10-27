# ADR-0021 · Check de validación `files` para proyectos que no son código

- **Estado:** ACEPTADO
- **Fecha:** 2026-10-27
- **Autor:** agente (claude-code/opus-5.5) · **Aprobado por:** Leo, por delegación (2026-10-26: «el ADR de P16.S0 se acepta por delegación»). Revisar en la mañana.
- **Fase/paso:** P16.S0

## Contexto
ADR-0020 pidió que un proyecto `document` o `media` valide que «el archivo existe y su formato se abre (comprobación por extensión, sin dependencias nuevas)». P15.5.S5 lo recortó porque un check nuevo es un nombre nuevo en `validation_checks.name` (un `CHECK` de la tabla) y en `crates/core/src/enums.rs`: migración y cambio del núcleo, como pasó con `plugin` (ADR-0015). Hoy un proyecto sin `[validation]` no tiene gate, así que un agente puede integrar un PDF truncado o una imagen vacía.

## Opciones consideradas
1. **Dejarlo a los plugins validators (ADR-0015).** Cada usuario tendría que escribir uno para lo más básico.
2. **Check nuevo `files` en el núcleo, sin dependencias:** por extensión, lee el archivo y comprueba la firma (bytes mágicos) o que el texto se parsee con lo que ya está en el workspace.
3. **Abrir cada formato con su biblioteca** (imágenes, PDF, ofimática). Muchas dependencias nuevas; prohibido sin ADR por cada una y fuera del espíritu de ADR-0020.

## Decisión
Opción 2.

- Nombre nuevo `files` en `validation_checks.name` (migración 009, reconstrucción de la tabla con el patrón de 002–006) y variante en el enum del core.
- Corre en el **tier 1** solo si `[project] kind` es `document` o `media` (en `code` y `general` no cambia nada). No necesita `[validation]`: un proyecto de documentos sin comandos de validación igual tiene este gate.
- Revisa los archivos **cambiados** por el agente (los del diff contra su base), no todo el repo. Un archivo borrado no se revisa.
- Por extensión, sin dependencias nuevas:
  - texto (`md`, `txt`, `csv`, `html`, `svg`, `srt`, `vtt`…): UTF-8 válido; `svg` además contiene `<svg`.
  - `json`: `serde_json`; `toml`: `toml_edit` (ya están en el workspace).
  - binarios por firma: `png`, `jpg/jpeg`, `gif`, `webp`, `pdf` (`%PDF-` y `%%EOF` al final), `docx/xlsx/pptx/odt/zip/epub` (`PK\x03\x04`), `mp3` (`ID3` o sincronía MPEG), `wav` (`RIFF…WAVE`), `mp4/mov/m4a` (`ftyp` en el byte 4), `webm/mkv` (`1A 45 DF A3`), `ogg` (`OggS`).
  - archivo vacío: `FAIL` salvo extensiones de texto.
  - extensión desconocida: no se juzga (cuenta como revisado, sin fallo).
- Resultado: `PASS` si todo se abre; `FAIL` con la lista de archivos y el motivo; sin archivos que revisar, `SKIPPED`.
- Los validators de plugin (ADR-0015) siguen sumándose en el tier 2.

## Consecuencias
- Migración 009 y test de migración 008 → 009 con datos.
- El check es determinista y local (CONSTRAINTS: nada de red ni de ML).
- Lo que esta comprobación no ve (un PDF bien formado pero con el contenido equivocado) sigue siendo trabajo de la revisión de Leo o de un review agent.
