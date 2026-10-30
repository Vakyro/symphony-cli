# ADR-0023 · Marcadores de color del chat

- **Estado:** ACEPTADO
- **Fecha:** 2026-10-29
- **Autor:** agente (codex/gpt-5.x) · **Aprobado por:** Leo, opción B de P17.S8
- **Fase/paso:** P17.S8

## Contexto

P17.S8 necesita que el chat sea más legible sin convertir la TUI en un renderizador completo de Markdown. P17.S1 ya define el tema morado apagado y sus roles de color. La decisión de Leo es que el agente indique explícitamente qué partes merecen color: el texto normal del asistente permanece blanco y la TUI solo interpreta marcadores conocidos.

## Decisión

Se adopta un protocolo lineal, explícito y pequeño. El agente puede envolver un fragmento en un span con esta sintaxis exacta:

```text
[[symphony:tipo]]texto[[/symphony:tipo]]
```

`tipo` debe ser uno de `title`, `bullet`, `highlight`, `warning`, `error` o `table`. El nombre de cierre debe coincidir exactamente con el de apertura. Los spans pueden aparecer dentro de una línea, pero no pueden anidarse. Para marcar una línea completa, el agente puede usar el mismo span alrededor de toda la línea. Una tabla se marca como un único span que contiene todas sus líneas.

Ejemplo:

```text
[[symphony:title]]Resultado[[/symphony:title]]
[[symphony:bullet]]Primera observación[[/symphony:bullet]]
[[symphony:warning]]Falta configurar el proveedor[[/symphony:warning]]
[[symphony:table]]Modelo | Estado
opus   | listo[[/symphony:table]]
```

Reglas de interpretación:

- La TUI elimina únicamente los delimitadores válidos y colorea el texto que contienen con el color asociado al tipo en el tema morado de P17.S1.
- Los seis tipos tienen un color propio dentro del tema; la asignación concreta de tonos pertenece al tema, no al protocolo ni al agente.
- Un marcador desconocido, mal cerrado o anidado se muestra literalmente, incluidos sus delimitadores. El texto sin marcadores se muestra tal cual y en blanco.
- No se interpreta Markdown: no se convierten encabezados, listas, énfasis, tablas ni escapes que no estén dentro de un marcador válido.
- Los marcadores se conservan en el transcript exactamente como fueron recibidos. La vista puede derivar una representación coloreada al renderizar, pero no sustituye el texto persistido.
- El protocolo no cambia el contenido semántico ni permite que el agente elija colores arbitrarios; solo el tipo.

## Alternativas descartadas

### Opción A: analizar todo Markdown

Usar `pulldown-cmark` para colorear títulos, listas, énfasis y tablas daría una apariencia más automática, pero añade una dependencia y un parser completo para una necesidad visual acotada. También convertiría la sintaxis Markdown del proveedor en parte del contrato de la TUI. Se descarta en favor de no añadir dependencias y mantener el comportamiento determinista.

### Heurística de la TUI

Inferir títulos, advertencias o tablas por prefijos, mayúsculas, palabras clave o separadores haría que el mismo texto cambiara de aspecto según el contenido. Las heurísticas fallan con texto legítimo y trasladan al cliente la decisión que corresponde al agente. Se descarta: la TUI solo colorea lo marcado.

## Consecuencias

- El agente debe emitir los marcadores cuando quiera una distinción visual; los prompts o contratos del harness deberán documentar estos seis tipos.
- El protocolo es fácil de guardar, transportar y depurar, y no requiere una dependencia nueva ni un parser de Markdown completo.
- El transcript conserva una representación fiel del intercambio, incluidos los marcadores, por lo que exportaciones y futuras vistas pueden reutilizarla.
- Un agente que no use marcadores seguirá funcionando: su respuesta se verá en blanco y sin transformación.
- Los errores de sintaxis son visibles en lugar de corregirse silenciosamente; esto facilita detectar agentes que no respetan el protocolo, a costa de que un marcador inválido pueda aparecer en pantalla.
- La asignación de tonos queda acoplada al tema de P17.S1. Cambiar el tema no requiere cambiar las respuestas ni el contrato del agente.
