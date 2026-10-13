import { cleanup, fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { setTransport, type Transport } from "./ipc";
import { setView, setAgent } from "./state";
import { Auto } from "./ui";
import { VIEWS } from "./views";

type Handler = (params: Record<string, unknown>) => unknown;

function fake(handlers: Record<string, Handler>) {
  const calls: { method: string; params: Record<string, unknown> }[] = [];
  const t: Transport = {
    async call(method, params) {
      calls.push({ method, params });
      const h = handlers[method];
      if (!h) throw { code: "unknown_method", message: `sin handler: ${method}` };
      return h(params);
    },
    onEvent: () => () => undefined,
    onDisconnected: () => () => undefined,
    ensureDaemon: async () => undefined,
  };
  return { t, calls };
}

const SYSTEM = {
  profile: "BALANCED",
  cpu_pct: 41,
  ram_pct: 55,
  cpu_ceiling_pct: 70,
  ram_ceiling_pct: 70,
  heavy_slots_total: 1,
  heavy_slots_used: 0,
};

beforeEach(() => {
  localStorage.clear();
});
afterEach(() => cleanup());

describe("registro de vistas", () => {
  it("tiene las 36 vistas de FLOW §18 (00–35) sin repetir", () => {
    expect(VIEWS.map((v) => v.id)).toEqual(Array.from({ length: 36 }, (_, i) => String(i).padStart(2, "0")));
  });
});

describe("Auto", () => {
  it("pinta una lista de objetos como tabla con insignias de estado", () => {
    render(() => <Auto value={[{ agent_id: "a1", state: "RUNNING", model_id: "claude/sonnet" }]} />);
    expect(screen.getByRole("table")).toBeTruthy();
    expect(screen.getByText("RUNNING").className).toContain("badge-ok");
  });

  it("muestra un objeto de una sola lista como esa lista", () => {
    render(() => <Auto value={{ agents: [] }} />);
    expect(screen.getByText("Sin elementos.")).toBeTruthy();
  });
});

describe("Home", () => {
  it("pide agentes, proveedores y sistema al daemon y lleva al agente al elegirlo", async () => {
    const { t, calls } = fake({
      "agent.list": () => ({ agents: [{ agent_id: "01ABC", number: 1, state: "RUNNING", model_id: "claude/sonnet", task_title: "Arreglar login" }] }),
      "providers.list": () => ({ providers: [{ display_name: "Claude", setup_state: "READY", models: 4, health: { state: "HEALTHY" } }] }),
      "system.get": () => SYSTEM,
      "agent.inspect": () => ({ agent_id: "01ABC", state: "RUNNING" }),
    });
    setTransport(t);
    setView("04");
    render(() => <App />);
    await waitFor(() => expect(screen.getByText("Arreglar login")).toBeTruthy());
    expect(screen.getByText("Claude")).toBeTruthy();
    expect(screen.getByText("41 %")).toBeTruthy();

    fireEvent.click(screen.getByText("Arreglar login"));
    await waitFor(() => expect(calls.some((c) => c.method === "agent.inspect" && c.params.agent === "01ABC")).toBe(true));
  });

  it("muestra el error del daemon en vez de quedarse cargando", async () => {
    const { t } = fake({
      "agent.list": () => {
        throw { code: "db", message: "no se pudo leer la base" };
      },
      "providers.list": () => ({ providers: [] }),
      "system.get": () => SYSTEM,
    });
    setTransport(t);
    setView("04");
    render(() => <App />);
    await waitFor(() => expect(screen.getByText("no se pudo leer la base")).toBeTruthy());
  });
});

describe("New Agent", () => {
  it("crea el agente con la tarea y el profile elegidos (sin lógica propia)", async () => {
    const create = vi.fn(() => ({ agent_id: "x" }));
    const { t } = fake({
      "models.list": () => ({ models: [{ id: "claude/sonnet", display_name: "Claude Sonnet", provider: "Claude" }] }),
      "profiles.list": () => ({ profiles: [{ id: "@code", description: "Best available for implementation" }] }),
      "agent.create": create,
      "agent.list": () => ({ agents: [] }),
    });
    setTransport(t);
    setView("05");
    render(() => <App />);
    const task = await screen.findByLabelText("Tarea");
    fireEvent.input(task, { target: { value: "Documentar el módulo" } });
    fireEvent.click(screen.getByText("Crear agente"));
    await waitFor(() => expect(create).toHaveBeenCalled());
    const first = create.mock.calls[0] as unknown as [Record<string, unknown>];
    expect(first[0]).toMatchObject({ title: "Documentar el módulo", model: null });
    await waitFor(() => expect(screen.getByText("Agente creado.")).toBeTruthy());
  });
});

describe("vistas de agente", () => {
  it("sin agente elegido invitan a elegir uno", async () => {
    setTransport(fake({}).t);
    setAgent("");
    setView("06");
    render(() => <App />);
    expect(await screen.findByText(/Elige un agente en/)).toBeTruthy();
  });
});
