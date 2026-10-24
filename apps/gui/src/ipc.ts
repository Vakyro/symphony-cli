// Única frontera con el daemon: el lado Rust (src-tauri) reenvía cada request por IPC.
// Nada de lógica de negocio aquí (FLOW regla 9): la GUI pide, el daemon decide.

export interface Failure {
  code: string;
  message: string;
}

export interface BusEvent {
  topic: string;
  payload: unknown;
}

/** Lo que la GUI necesita del exterior; en tests se reemplaza por uno falso. */
export interface Transport {
  call(method: string, params: Record<string, unknown>): Promise<unknown>;
  onEvent(cb: (e: BusEvent) => void): () => void;
  onDisconnected(cb: (why: string) => void): () => void;
  ensureDaemon(): Promise<void>;
}

function asFailure(e: unknown): Failure {
  if (e && typeof e === "object" && "code" in e && "message" in e) return e as Failure;
  return { code: "error", message: String(e) };
}

export function tauriTransport(): Transport {
  // Import dinámico: en el navegador (vitest) no existe `window.__TAURI_INTERNALS__`.
  const api = () => import("@tauri-apps/api/core");
  const events = () => import("@tauri-apps/api/event");
  const listen = <T>(name: string, cb: (p: T) => void) => {
    let off: (() => void) | undefined;
    let gone = false;
    void events().then((m) =>
      m.listen<T>(name, (e) => cb(e.payload)).then((u) => {
        if (gone) u();
        else off = u;
      }),
    );
    return () => {
      gone = true;
      off?.();
    };
  };
  return {
    async call(method, params) {
      try {
        return await (await api()).invoke("ipc_call", { method, params });
      } catch (e) {
        throw asFailure(e);
      }
    },
    onEvent: (cb) => listen<BusEvent>("symphony://event", cb),
    onDisconnected: (cb) => listen<string>("symphony://disconnected", cb),
    async ensureDaemon() {
      try {
        await (await api()).invoke("daemon_ensure");
      } catch (e) {
        throw asFailure(e);
      }
    },
  };
}

let current: Transport | undefined;

export function setTransport(t: Transport) {
  current = t;
}

export function transport(): Transport {
  current ??= tauriTransport();
  return current;
}

export const call = (method: string, params: Record<string, unknown> = {}) =>
  transport().call(method, params);
