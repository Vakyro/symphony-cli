# Spec de Symphony (solo lectura)

Documentos originales. No se editan: los cambios de decisión van en `docs/adr/`.

| Alias | Archivo | Para qué sirve |
|---|---|---|
| **IDEA** | `idea.md` | Visión, principios, arquitectura, supuestos abiertos, roadmap de producto |
| **FLOW** | `Symphony_CLI_User_Flow_and_Views.html` | Flujo de usuario, 35 vistas, estados, ramas, journeys A–E, reglas UX |
| **STACK** | `Symphony_CLI_Ideal_Technology_Stack.md` | Tecnología, crates y prohibiciones |
| **DB** | `symphony_database.md` | 42 tablas, tipos, enums, índices, relaciones y fases |
| **ER** | `symphony_er_diagram.html` | Diagrama entidad-relación. **No es fuente de verdad** (lo es DB) |
| **STACKDIAG** | `symphony_stack_diagram.html` | Cómo se conectan componentes y tecnologías |
| **SKILLS** | `Catalogo-Skills-ClaudeCode.pdf` | Skills disponibles en la máquina de Leo |
| **PLAN** | [`../../PLAN.md`](../../PLAN.md) | Orden de trabajo y protocolo |

## Precedencia (PLAN §1.2)

1. ADRs aceptados en `docs/adr/`
2. PLAN (orden y protocolo)
3. DB (esquema, tipos, enums, nombres)
4. STACK (tecnología, crates, prohibiciones)
5. FLOW (comportamiento visible)
6. IDEA (principios y visión)

Una contradicción nunca se resuelve en silencio: va a "Desviaciones del spec" en la bitácora de fase y, si importa, a un ADR.
