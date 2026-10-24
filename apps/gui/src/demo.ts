// Datos de ejemplo para revisar el diseño en un navegador sin daemon (`pnpm dev`, solo en
// desarrollo; no entra al bundle de producción). Las formas son las que devuelve el daemon.

import type { Transport } from "./ipc";

const provider = (id: string, name: string, state: string, models: string[]) => ({
  id,
  display_name: name,
  enabled: true,
  setup_state: "READY",
  cli_version: "1.0.0",
  models: models.length,
  model_names: models,
  agents_active: id === "anthropic" ? 2 : 0,
  health: { state, certainty: "OBSERVED", reserve: 0.2, remaining: null },
  usage_7d: { reported_tokens: 183_000, estimated_tokens: 12_000 },
});

const DATA: Record<string, unknown> = {
  "agent.list": {
    agents: [
      { agent_id: "01J9A", number: 1, state: "RUNNING", task_code: "T-1", task_title: "Arreglar el login", provider_id: "anthropic", model_id: "claude/sonnet" },
      { agent_id: "01J9B", number: 2, state: "WAITING_PROVIDER", task_code: "T-2", task_title: "Migrar la tabla de sesiones", provider_id: "openai", model_id: "codex/gpt" },
      { agent_id: "01J9C", number: 3, state: "TESTING", task_code: "T-3", task_title: "Documentar la API", provider_id: "anthropic", model_id: "claude/haiku" },
      { agent_id: "01J9D", number: 4, state: "COMPLETED", task_code: "T-4", task_title: "Limpiar dependencias", provider_id: "google", model_id: "google/gemini-flash" },
    ],
  },
  "providers.list": {
    providers: [
      provider("anthropic", "Claude", "HEALTHY", ["Claude Opus", "Claude Sonnet", "Claude Haiku"]),
      provider("openai", "Codex", "DEGRADED", ["GPT"]),
      provider("google", "Antigravity", "UNKNOWN", ["Gemini Flash", "Gemini Pro"]),
    ],
  },
  "system.get": {
    profile: "BALANCED",
    cpu_pct: 41,
    ram_pct: 58,
    cpu_ceiling_pct: 70,
    ram_ceiling_pct: 70,
    heavy_slots_total: 1,
    heavy_slots_used: 1,
    running: [{ agent: "01J9A", op: "cargo test", class: 3 }],
    waiting: [{ agent: "01J9C", op: "pnpm build", class: 3 }],
  },
  "models.list": {
    models: [
      { id: "claude/sonnet", display_name: "Claude Sonnet", provider: "Claude", provider_id: "anthropic", available: true, health: "HEALTHY" },
      { id: "claude/haiku", display_name: "Claude Haiku", provider: "Claude", provider_id: "anthropic", available: true, health: "HEALTHY" },
      { id: "codex/gpt", display_name: "GPT", provider: "Codex", provider_id: "openai", available: true, health: "DEGRADED" },
    ],
  },
  "profiles.list": {
    profiles: [
      { id: "@code", description: "Best available for implementation" },
      { id: "@fast", description: "Prioritize response speed" },
      { id: "@conserve", description: "Preserve scarce quota" },
    ],
  },
  "recovery.list": {
    items: [{ id: "r1", kind: "AGENT_STALE", severity: "HIGH", agent_id: "01J9B", summary: "El agente 2 no latió en 10 min" }],
  },
  "task.list": {
    tasks: [
      { id: "T-1", title: "Arreglar el login", state: "RUNNING", agent: "01J9A" },
      { id: "T-2", title: "Migrar la tabla de sesiones", state: "WAITING", agent: "01J9B" },
      { id: "T-5", title: "Publicar guía", state: "BACKLOG", agent: null },
    ],
  },
  "usage.get": { days: 7, usage: [{ provider: "Claude", reported_tokens: 183000, estimated_tokens: 12000 }] },
  "context.stats": { chunks: 1204, objects: 311, retrievals: { total: 52, misses: 3, tokens_returned: 18400 }, handoffs: [], compression: [] },
  "project.status": { name: "symphony-cli", branch: "main", initialized: true, is_repo: true, root: "C:/proyectos/symphony-cli", recovery_open: 1 },
  "plugin.list": { plugins: [], problems: [], enabled_but_missing: [] },
  "agent.inspect": { agent_id: "01J9A", state: "RUNNING", model_id: "claude/sonnet", branch: "symphony/t-1", worktree: "C:/.symphony/worktrees/01J9A" },
  "chat.get": { agent_id: "01J9CHAT" },
  "agent.logs": { lines: [{ at: "10:02", who: "user", text: "Revisa el login" }, { at: "10:03", who: "agent", text: "Entendido, empiezo por auth.rs" }] },
};

export function demoTransport(): Transport {
  return {
    async call(method) {
      await new Promise((r) => setTimeout(r, 60));
      if (method in DATA) return DATA[method];
      return {};
    },
    onEvent: () => () => undefined,
    onDisconnected: () => () => undefined,
    ensureDaemon: async () => undefined,
  };
}
