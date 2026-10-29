# P13.S5 · Auditoría del CLI, el harness y el broker MCP

Revisión estática de `crates/cli/src/*.rs`. Los escenarios describen entradas posibles a las herramientas MCP; no requieren ejecutar proveedores reales. Las rutas absolutas y el acceso fuera del worktree son deliberados según ADR-0019, por lo que no se consideran defectos.

| Archivo:línea | Severidad | Qué pasa | Escenario concreto que lo dispara | Arreglo propuesto |
|---|---|---|---|---|
| `crates/cli/src/harness.rs:103-108,136-139` | alta | `write` y `edit` reemplazan el archivo directamente. Un error de escritura tras truncarlo puede destruir el contenido previo; `edit` tampoco comprueba que nadie lo haya modificado entre lectura y escritura. | Disco lleno durante `write` o `edit`, o dos herramientas que editan el mismo archivo a la vez. | Escribir en un temporal del mismo directorio, sincronizar y renombrar de forma atómica; en `edit`, detectar cambios concurrentes antes de reemplazar. |
| `crates/cli/src/harness.rs:58-75` | media | `read` recorre el archivo entero aunque solo se pidan unas líneas y `BufRead::lines` reserva toda la línea antes de aplicar `cut_line`. | `read` de la primera línea de un archivo de muchos GB, o de una sola línea minificada enorme. | Dejar de leer al completar la página y leer cada línea con un límite de bytes; anunciar si quedan más líneas sin recorrer todo el archivo. |
| `crates/cli/src/harness.rs:198-205,209-219` | media | `grep` guarda en `files` todas las rutas antes de buscar y lee cada archivo completo; `max_results` limita lo devuelto, pero sigue contando coincidencias de todos los archivos. | `grep` en un árbol con millones de archivos pequeños o con una expresión que aparece en cada línea. | Recorrer archivos en flujo, limitar recursos del recorrido y detenerse al alcanzar el máximo de resultados; indicar que el total es parcial. |
| `crates/cli/src/harness.rs:167-183` | media | `glob` acumula y ordena todas las rutas antes de truncar a 500. El límite de respuesta no limita RAM ni tiempo de búsqueda. | `glob` sobre un árbol con millones de archivos que coinciden. | Acotar el recorrido y la colección; si se necesita orden global, paginar o usar un presupuesto explícito. |
| `crates/cli/src/harness.rs:245-252` | media | El máximo de resultados de `grep` es controlado por la llamada (`max.unwrap_or(...).max(1)`) y no tiene cota superior. | Solicitar `max_results` muy grande en un archivo con muchas líneas coincidentes; la respuesta y `hits` crecen sin límite práctico. | Aplicar un máximo fijo antes de reservar o devolver coincidencias. |
| `crates/cli/src/harness.rs:338-350` | media | Tras un shell que termina, la salida de cada flujo espera hasta dos segundos; cuando vence ese plazo se descarta todo lo leído por la tarea. | `sh -c 'proceso_en_segundo_plano & echo resultado'`: el hijo conserva stdout abierto más de dos segundos y el resultado se pierde. | Mantener una copia compartida de los bytes retenidos y devolverla al vencer la espera, o cerrar los flujos restantes conservando lo ya leído. |
| `crates/cli/src/hook.rs:12-39` | baja | El hook lee como máximo `MAX_PAYLOAD` bytes y luego intenta parsear JSON; un evento mayor se trunca y se descarta sin informar al daemon. | Un hook con una salida de herramienta mayor que el límite deja de registrar el evento. | Detectar explícitamente el exceso y enviar un evento de truncamiento o registrar un aviso acotado. |

## Seguimiento (P13.S5)

| Hallazgo (archivo:línea original) | Severidad | Estado | Test o razón |
|---|---|---|---|
| `crates/cli/src/harness.rs:103-108,136-139` | alta | corregido | `write` y `edit` ahora usan `write_atomic` (temporal en la misma carpeta, `sync_all`, `rename` encima): harness.rs:47-71, llamado en write:144 y edit:193. Un fallo a mitad deja intacta la versión anterior. Test: `replacing_a_file_does_not_truncate_the_previous_version` (harness.rs:593). |
| `crates/cli/src/harness.rs:58-75` | media | corregido | `read` lee línea a línea con `BufReader::read_line` y se detiene al completar la página; usa `fill_buf` para saber si quedan más sin recorrer el resto: harness.rs:82-135. Test: `read_does_not_parse_beyond_the_requested_page` (harness.rs:704). |
| `crates/cli/src/harness.rs:198-205,209-219` | media | corregido | `grep` recorre archivos en flujo (`Box<dyn Iterator>`) y se detiene al alcanzar el máximo (`break 'archivos`); indica búsqueda parcial con `partial`: harness.rs:253-319. Test: `grep_reports_a_partial_search_at_the_requested_limit` (harness.rs:715). |
| `crates/cli/src/harness.rs:167-183` | media | corregido | `glob` corta el recorrido al llegar a `GLOB_RESULTS` y marca `partial`; solo ordena lo recogido: harness.rs:224-250. Test: `glob_reports_a_partial_search_at_its_limit` (harness.rs:750). |
| `crates/cli/src/harness.rs:245-252` | media | corregido | Cota dura `GREP_MAX = 1000` (harness.rs:24-25) aplicada con `clamp(1, GREP_MAX)` en grep:266. Test: `grep_caps_the_requested_number_of_results` (harness.rs:729). |
| `crates/cli/src/harness.rs:338-350` | media | corregido | Salida del shell retenida en búfer compartido (`Captured`/`drain`/`grab`): al vencer la espera se devuelve lo ya leído en vez de descartarlo: harness.rs:466-533. Tests: `shell_keeps_output_when_a_descendant_holds_the_pipe_open` (harness.rs:764) y `a_huge_output_keeps_only_head_and_tail_in_memory` (harness.rs:690). |
| `crates/cli/src/hook.rs:12-39` | baja | diferido | `emit()` trunca con `stdin().take(MAX_PAYLOAD)` (hook.rs:32-38) y descarta sin informar. Reportar el exceso tocaría el contrato "sin salida / nunca rompe al CLI" de `emit()` (hook.rs:7-8), así que no es un cambio seguro de 1-2 líneas; requiere revisar el contrato del hook antes de tocarlo. |

### Lo que quedó fuera

Cada hallazgo se verificó contra el código actual; los tests citados viven en
`crates/cli/src/harness.rs` (`mod tests`).

- **Cambios concurrentes en `edit`** (parte del hallazgo alto): diferido. `std` no ofrece
  compare-and-swap y la ventana entre leer y renombrar es de microsegundos dentro de la misma
  llamada; el riesgo residual es «gana la última escritura».
- **Tope de bytes por línea en `read`** (parte del segundo hallazgo): diferido. Exige un lector
  acotado y una semántica de recorte distinta de la de `cut_line`.
- **Orden de `grep`/`glob`**: ya no es alfabético global; se asume para poder cortar el recorrido.
- **`hook.rs:12-39`**: diferido. Detectar el exceso es trivial, pero informarlo obliga a añadir un
  evento al protocolo IPC y el módulo tiene por contrato salida silenciosa y exit 0; es una
  decisión de protocolo, no un arreglo local.
