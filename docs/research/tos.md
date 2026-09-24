# Términos de uso: uso automatizado de los CLIs con suscripción

- **Fecha:** 2026-09-24 · **Agente:** claude-code/opus-5.5 · **Paso:** P01.S2
- **Estado:** ✅ **Leo aceptó los riesgos para uso personal** (2026-09-24). La distribución a terceros se reevalúa antes de v0.1 (P07).

Esto no es asesoría legal. Es un resumen de las fuentes públicas a la fecha, con citas, para que Leo decida.

## Anthropic: Claude Code con plan Pro/Max

**Fuentes:** [Legal and compliance (Claude Code docs)](https://code.claude.com/docs/en/legal-and-compliance), [Use Claude Code with your Pro or Max plan](https://support.claude.com/en/articles/11145838-use-claude-code-with-your-pro-or-max-plan), [Use the Claude Agent SDK with your Claude plan](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan), [The New Stack: pausa del cambio de facturación](https://thenewstack.io/anthropic-pauses-claude-agent-sdk-subscription-change/).

Lo que dicen:
1. "OAuth authentication is intended exclusively for purchasers of Claude Free, Pro, Max, Team, and Enterprise subscription plans and is designed to support **ordinary use of Claude Code** and other native Anthropic applications."
2. "Anthropic does not permit third-party developers to offer Claude.ai login into their own applications, or to **route requests through Free, Pro, or Max plan credentials on behalf of their users**. Moreover, developers may not collect, store, or intermediate Claude.ai credentials or session tokens."
3. "Advertised usage limits for Pro and Max plans assume ordinary, individual usage of Claude Code and the Agent SDK."
4. Facturación: Anthropic anunció que desde el 15 de junio de 2026 el uso de `claude -p`, del Agent SDK y de apps de terceros saldría de un crédito mensual aparte, a precio de API. **Lo pausó** el día en que iba a entrar en vigor. Hoy, `claude -p` y el SDK siguen contando contra los límites normales de la suscripción, y Anthropic dijo que avisará antes de cualquier cambio.

Cómo encaja Symphony:
- ✅ Symphony **no** toca credenciales: lanza el binario oficial `claude`, que hace su propio login (regla PLAN §2.9). Eso cumple el punto 2 ("sign-in must complete through Anthropic's own flow").
- ✅ Leo usa **su propia** suscripción en **su propia** máquina: no hay "on behalf of their users".
- ⚠️ **Riesgo abierto:** si Symphony se distribuye y otros lo usan con su propia suscripción, cada uno sigue autenticándose con su CLI. Pero Anthropic podría considerar que un orquestador de terceros no es "ordinary use". El texto no lo prohíbe de forma explícita para un CLI oficial invocado localmente.
- ⚠️ **Riesgo de costo:** si Anthropic retoma el crédito aparte, el modo headless (`-p`) de Symphony saldría de ese crédito a precio de API. Esto afecta la decisión de ADR-0005: un modo interactivo o attach podría seguir contando como uso normal. Hay que vigilarlo.

## OpenAI: Codex CLI con plan ChatGPT

**Fuentes:** [Non-interactive mode](https://developers.openai.com/codex/noninteractive), [Codex CLI](https://learn.chatgpt.com/docs/codex/cli), [Using Codex with your ChatGPT plan](https://help.openai.com/en/articles/11369540-using-codex-with-your-chatgpt-plan) (no se pudo leer: la página tiene un bloqueo anti-bot; **Leo debería revisarla**).

Lo que dicen:
1. La documentación oficial presenta `codex exec` para "scripts and pipelines" y para "repeatable workflows", y describe cómo correrlo en CI **con la cuenta ChatGPT** ("users who need ChatGPT/Codex rate limits instead of API key usage"). Recomienda API key como opción por defecto para automatización.
2. "Treat `~/.codex/auth.json` like a password… Do not use this workflow for public or open-source repositories."

Cómo encaja Symphony:
- ✅ El uso automatizado de `codex exec` con la cuenta ChatGPT está documentado como caso soportado.
- ✅ Symphony no lee ni mueve `auth.json`.
- ⚠️ Falta revisar los Terms of Use de OpenAI y la página de ayuda del plan (bloqueada para el agente).

## Pregunta para Leo

¿Aceptas estos riesgos para uso **personal** (tu máquina, tus suscripciones)? La distribución pública y el uso por terceros se vuelven a evaluar antes de v0.1 (P07).
