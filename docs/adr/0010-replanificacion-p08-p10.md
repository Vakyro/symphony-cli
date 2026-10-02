# ADR-0010 · Replanificación de P08–P10 (cierre de P07.S10)

- **Estado:** ACEPTADO
- **Fecha:** 2026-10-01
- **Autor:** claude-code/sonnet-5.5 · **Aprobado por:** Leo (2026-10-01: «haz todo esto que diferiste», P07.S10, P08, P09 y P10)
- **Fase/paso:** P07.S10 (replanificación) · P10 · P09 · P08

## Contexto
P07.S10 pedía revisar P08–P16 con evidencia de uso. Se hizo por partes: ADR-0006 (el chat, P07.5), ADR-0007 (gate por criterio de contenido), ADR-0008 (P11 antes de P08–P10). P11 está cerrada con cinco proveedores. Leo pidió ahora hacer lo que quedó diferido: la revisión de P07.S10 y las fases P08, P09 y P10.

Evidencia acumulada (`docs/research/uso-v0.1.md`, bitácoras de P07.5 y P11, `cli-p11.md`):
- El valor está en el chat con continuidad entre proveedores (ADR-0006); el cambio de proveedor funciona con cinco CLIs reales (matriz de handoff, P11.S5).
- El coste de cambiar de proveedor lo domina el arranque del CLI de destino (~18k–27k tokens), no el handoff.
- Hoy el failover es el básico de P06 (`repo::next_executor`); la salud de proveedores se deduce de los errores parseados sin estado propio; no hay cuota por proveedor ni routing por profiles.
- Cada adapter reporta cosas distintas: cuota KNOWN solo en Claude y Codex; Copilot informa «solicitudes premium»; Kimi y Antigravity no informan cuota; Kimi no da tokens; solo Claude, Codex y Antigravity dan uso de contexto.
- Leo levanta con esta instrucción el gate de tiempo de ADR-0007 §3 (no empezar el scheduler multiagente sin haber usado el producto un tiempo): **la decisión es suya y es explícita**. El riesgo (construir P08 con poco uso del multiagente) se asume y se mitiga con el paso de diseño y el benchmark-gate de P08.

## Opciones consideradas
1. **Hacer P08, P09 y P10 completas, en el orden del PLAN (P08 → P09 → P10).** Respeta el PLAN pero deja el failover y la salud de proveedores, lo que más sirve al chat, para el final.
2. **Mantener el orden de ADR-0008: P10 → P09 → P08, cada una completa (elegida).** Cada fase cierra con su tag y su revisión; el valor para el chat llega primero.
3. **Recortar P09 y P10 como sugería ADR-0006.** Ya no aplica: Leo pidió el alcance completo.

## Decisión
1. **Orden de ejecución: P10 → P09 → P08**, cada una en su rama `phase/pNN-*`, con revisión de código, CI en los tres sistemas, merge `--no-ff` y tag `pNN-done`. Los tags públicos de versión (`v0.5.0` tras P10) se crean con ese cierre.
2. **Alcance: el de cada fase en el PLAN**, con estos ajustes por la evidencia:
   - **P10:** una cuenta por proveedor (`provider_accounts` con la cuenta «default» autodetectada; sin gestión de login: PLAN §2.9). `quota_certainty`: `KNOWN` donde el CLI la informa (Claude, Codex), `ESTIMATED` donde la reserva viene de la config o de las «solicitudes premium» (Copilot), `UNKNOWN` en el resto. Profiles iniciales `@code`, `@cheap` y `@fast` con pesos en `weights_json`. Los textos de falla por cuota de Kimi, Antigravity y Copilot siguen siendo sintéticos hasta que aparezcan en vivo.
   - **P09:** se hace completa, en este orden de valor: migración 003 → ctx:// → compresores → handoff assembler v2 → evaluación del handoff (P09.S10, que decide los defaults) → AST (tree-sitter) → watcher → hechos → broker MCP → skills/MCP compartidos. La evaluación de S10 manda: si `balanced` pierde efectividad frente a `raw`, se ajustan los defaults y se documenta.
   - **P08:** el destino de integración es la rama del chat (ADR-0006: `main ← symphony/chat ← agentes`); chat → `main` es un merge aparte con review agent. **La decisión de «phase + conditions» frente a los 12 estados** se toma en el primer paso de P08 con los datos del scheduler (ADR-0006 §8): se mantienen los estados exclusivos salvo que el scheduler demuestre que no alcanzan. Suspend/resume por inactividad (idea de AX) entra solo si el benchmark lo pide.
3. **Diseño primero.** El primer paso de cada fase empieza con un párrafo de diseño en su bitácora (qué tablas, qué contratos, qué se prueba) antes de escribir código. Si un paso exige cambiar el core, ADR primero (como ADR-0009).
4. **Pruebas.** CI nunca gasta cuota. Las pruebas live (L3) las dispara quien corre el paso con `SYMPHONY_LIVE=1` y se anotan en la bitácora; Leo autorizó las de P11 y las de estas fases durante esta sesión («tienes permiso de todo»).
5. **Lo que no cambia:** P12–P16 siguen provisionales; el MVP de proveedores son los cinco ya integrados.

## Consecuencias
- PLAN: P08–P10 dejan de ser «provisionales»; se actualizan el mapa de fases, el registro de cambios y los prerrequisitos. P12–P16 conservan la marca.
- STATUS: P07.S10 queda ✅ (cerrada por este ADR); el gate de tiempo de ADR-0007 §3 queda levantado.
- Riesgo: P08 (multiagente) se construye con poco uso real del multiagente. Mitigación: diseño explícito al inicio, el benchmark principal (P08.S9) como gate con la opinión de Leo sobre la fluidez de la máquina, y no mergear a `main` sin política explícita.
- Riesgo: el alcance es muy grande. Se trabaja por pasos pequeños con commit por paso, y cada fase cierra por separado; una fase puede quedar con pendientes documentados sin bloquear a la siguiente.
