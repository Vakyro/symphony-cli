// Estado de la GUI: solo lo de presentación (vista actual, tema, proyecto elegido). El estado
// del trabajo vive en el daemon y se pide con `useCall`.

import { createEffect, createResource, createSignal, onCleanup } from "solid-js";
import { call, transport, type Failure } from "./ipc";

function stored(key: string, fallback: string): string {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}

function persist(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* sin almacenamiento: no pasa nada */
  }
}

export type Theme = "auto" | "light" | "dark";

const THEMES: Theme[] = ["auto", "light", "dark"];
const savedTheme = stored("theme", "auto") as Theme;
const [theme, setThemeSignal] = createSignal<Theme>(THEMES.includes(savedTheme) ? savedTheme : "auto");
export { theme };
export function setTheme(t: Theme) {
  setThemeSignal(t);
  persist("theme", t);
}

const [view, setViewSignal] = createSignal(stored("view", "04"));
export { view };
export function setView(id: string) {
  setViewSignal(id);
  persist("view", id);
}

/** Raíz del proyecto sobre el que se pregunta al daemon (vacía = la del daemon). */
const [projectRoot, setRootSignal] = createSignal(stored("root", ""));
export { projectRoot };
export function setProjectRoot(r: string) {
  setRootSignal(r);
  persist("root", r);
  // Cada proyecto tiene su daemon: se apunta el puente y se arranca el de esa carpeta.
  void pushProject(r)
    .then(() => (r.trim() ? transport().ensureDaemon() : undefined))
    .then(markConnected)
    .catch(() => undefined);
}

/** Cada proyecto tiene su daemon: le dice al puente a cuál hablar (sin puente, p. ej. en las pruebas, no hace nada). */
export async function pushProject(r: string): Promise<void> {
  try {
    await call("gui.set_project", { root: r.trim() });
  } catch {
    /* sin puente o carpeta inválida: las vistas mostrarán el error del daemon */
  }
}

/** Agente seleccionado (vistas 06–12). */
const [agentId, setAgentSignal] = createSignal(stored("agent", ""));
export { agentId };
export function setAgent(id: string) {
  setAgentSignal(id);
  persist("agent", id);
}

const [connected, setConnected] = createSignal(true);
export { connected };

// Cada evento del bus invalida las vistas abiertas (el daemon es la fuente de verdad).
// Un agente en marcha emite muchos eventos por segundo: se agrupan en un solo refresco.
const BUS_DEBOUNCE_MS = 300;
const [tick, setTick] = createSignal(0);
export function startBus(): () => void {
  const t = transport();
  let pending: ReturnType<typeof setTimeout> | undefined;
  const offEvent = t.onEvent(() => {
    if (pending === undefined)
      pending = setTimeout(() => {
        pending = undefined;
        setTick((n) => n + 1);
      }, BUS_DEBOUNCE_MS);
  });
  const offDown = t.onDisconnected(() => setConnected(false));
  return () => {
    offEvent();
    offDown();
    clearTimeout(pending);
  };
}
export const busTick = tick;
export function markConnected() {
  setConnected(true);
}

export interface CallResult<T> {
  data: () => T | undefined;
  error: () => Failure | undefined;
  loading: () => boolean;
  refetch: () => void;
}

/** Pide `method` y se refresca con cada evento del bus; `null` desde `params` = no pedir. */
export function useCall<T = unknown>(
  method: string,
  params: () => Record<string, unknown> | null,
  opts: { live?: boolean } = { live: true },
): CallResult<T> {
  const source = () => {
    const p = params();
    return p === null ? false : { p, tick: opts.live === false ? 0 : tick() };
  };
  const [res, { refetch }] = createResource(source, async (s) => {
    try {
      const out = (await call(method, s.p)) as T;
      markConnected();
      return { ok: out } as { ok: T; err?: undefined } | { ok?: undefined; err: Failure };
    } catch (e) {
      return { err: e as Failure };
    }
  });
  // Un valor viejo se queda en pantalla mientras llega el nuevo (sin parpadeo).
  let last: T | undefined;
  createEffect(() => {
    const r = res();
    if (r && "ok" in r) last = r.ok;
  });
  // Respaldo por si se pierde algún evento; no aplica a lo que no es en vivo ni a lo que no se pide.
  const timer = setInterval(() => {
    if (opts.live !== false && params() !== null) void refetch();
  }, 15000);
  onCleanup(() => clearInterval(timer));
  return {
    data: () => {
      const r = res();
      return r && "ok" in r ? r.ok : last;
    },
    error: () => res()?.err,
    loading: () => res.loading,
    refetch: () => void refetch(),
  };
}

/** Parámetros con la raíz del proyecto, si hay una elegida. */
export function withRoot(extra: Record<string, unknown> = {}): Record<string, unknown> {
  const r = projectRoot().trim();
  return r ? { project_root: r, ...extra } : { ...extra };
}
