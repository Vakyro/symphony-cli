# ADR-0022: Datos de uso entre proyectos en status global

- **Estado:** Aceptado
- **Fecha:** 2026-10-29
- **Enmienda:** ADR-0018

## Contexto

La pantalla de status global necesita mostrar información de uso de varios proyectos sin convertir el directorio global en una base de datos compartida ni introducir telemetría.

## Decisión

La pantalla status global leerá, en modo solo lectura, los datos de uso disponibles en cada directorio `.symphony` de proyecto. La lista de proyectos se obtiene de `~/.symphony`; cada proyecto conserva sus propios datos y su propia autoridad.

Se pueden mostrar agregados y estados por proyecto, pero no se mezclan ni se consolidan los datos en una única base de datos de proyecto. No se modifican, migran ni borran archivos durante esta lectura.

## Qué se lee

- La lista de proyectos registrada en `~/.symphony`.
- Los datos locales de uso y estado que cada proyecto expone dentro de su `.symphony`.

## Qué no se mezcla

- No se combinan historiales, eventos, muestras ni contadores entre proyectos.
- No se atribuyen datos de un proyecto a otro.
- No se crea un almacén global derivado con los datos de uso.

## Privacidad

Esta funcionalidad no añade telemetría ni llamadas de red. La lectura permanece local, limitada a los proyectos enumerados por `~/.symphony`, y no comparte datos con proveedores ni con terceros.

## Consecuencias

- La pantalla puede presentar una visión global sin cambiar la propiedad local de los datos.
- Un proyecto ausente, inaccesible o incompleto se representa como tal; no se inventan datos ni se rellenan con datos de otro proyecto.
- Las futuras ampliaciones deben conservar lectura local, solo lectura y separación entre proyectos.
