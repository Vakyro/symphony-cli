// Piezas de presentación genéricas: tablas, pares clave-valor, insignias. Pintan lo que el
// daemon devuelve tal cual; no interpretan ni deciden nada.

import { For, Show, type JSX } from "solid-js";

export type Json = null | boolean | number | string | Json[] | { [k: string]: Json };

const STATE_TONE: Record<string, string> = {
  RUNNING: "ok",
  READY: "ok",
  COMPLETED: "ok",
  DONE: "ok",
  PASS: "ok",
  HEALTHY: "ok",
  AVAILABLE: "ok",
  TESTING: "info",
  CREATED: "info",
  WAITING_PROVIDER: "warn",
  WAITING_RESOURCE: "warn",
  WAITING_DEPENDENCY: "warn",
  PAUSED: "warn",
  DEGRADED: "warn",
  BLOCKED: "bad",
  FAILED: "bad",
  FAIL: "bad",
  CANCELLED: "muted",
  UNKNOWN: "muted",
};

export function tone(value: string): string {
  return STATE_TONE[value.toUpperCase()] ?? "muted";
}

export function Badge(props: { value: string }) {
  return <span class={`badge badge-${tone(props.value)}`}>{props.value}</span>;
}

function isObject(v: unknown): v is { [k: string]: Json } {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function isScalar(v: unknown): boolean {
  return v === null || ["string", "number", "boolean"].includes(typeof v);
}

/** Un valor corto en una celda. Los estados salen como insignia. */
export function Cell(props: { k?: string; v: unknown }) {
  const v = props.v;
  if (v === null || v === undefined) return <span class="dim">—</span>;
  if (typeof v === "boolean") return <span>{v ? "sí" : "no"}</span>;
  if (typeof v === "number") return <span class="num">{Number.isInteger(v) ? v : v.toFixed(2)}</span>;
  if (typeof v === "string") {
    if (props.k && /state|status|health|outcome/.test(props.k) && /^[A-Z_]+$/.test(v)) {
      return <Badge value={v} />;
    }
    return <span title={v.length > 60 ? v : undefined}>{v.length > 80 ? `${v.slice(0, 79)}…` : v}</span>;
  }
  if (Array.isArray(v)) return <span class="dim">{v.length} elementos</span>;
  return <span class="dim">{`{${Object.keys(v as object).length}}`}</span>;
}

export function Table(props: { rows: { [k: string]: Json }[]; max?: number; onPick?: (row: { [k: string]: Json }) => void }) {
  const cols = () => {
    const seen: string[] = [];
    for (const r of props.rows) {
      for (const [k, v] of Object.entries(r)) {
        if (isScalar(v) && !seen.includes(k)) seen.push(k);
      }
    }
    return seen.slice(0, props.max ?? 9);
  };
  return (
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <For each={cols()}>{(c) => <th>{c.replaceAll("_", " ")}</th>}</For>
          </tr>
        </thead>
        <tbody>
          <For each={props.rows}>
            {(r) => (
              <tr class={props.onPick ? "pick" : undefined} onClick={() => props.onPick?.(r)}>
                <For each={cols()}>
                  {(c) => (
                    <td>
                      <Cell k={c} v={r[c]} />
                    </td>
                  )}
                </For>
              </tr>
            )}
          </For>
        </tbody>
      </table>
    </div>
  );
}

export function KV(props: { data: { [k: string]: Json } }) {
  return (
    <dl class="kv">
      <For each={Object.entries(props.data)}>
        {([k, v]) => (
          <>
            <dt>{k.replaceAll("_", " ")}</dt>
            <dd>
              <Show when={isScalar(v)} fallback={<Auto value={v} />}>
                <Cell k={k} v={v} />
              </Show>
            </dd>
          </>
        )}
      </For>
    </dl>
  );
}

/** Pinta cualquier JSON: lista de objetos → tabla, objeto → pares, resto → texto. */
export function Auto(props: { value: unknown; onPick?: (row: { [k: string]: Json }) => void }): JSX.Element {
  const v = props.value as Json;
  if (Array.isArray(v)) {
    if (v.length === 0) return <p class="dim">Sin elementos.</p>;
    if (v.every(isObject)) return <Table rows={v as { [k: string]: Json }[]} onPick={props.onPick} />;
    return (
      <ul class="plain">
        <For each={v}>{(x) => <li><Cell v={x} /></li>}</For>
      </ul>
    );
  }
  if (isObject(v)) {
    const entries = Object.entries(v);
    // Un objeto cuyo único contenido es una lista (`{agents: [...]}`) se muestra como esa lista.
    if (entries.length === 1 && Array.isArray(entries[0][1])) {
      return <Auto value={entries[0][1]} onPick={props.onPick} />;
    }
    return <KV data={v} />;
  }
  return <Cell v={v} />;
}

export function Panel(props: { title: string; aside?: JSX.Element; children: JSX.Element }) {
  return (
    <section class="panel">
      <header>
        <h2>{props.title}</h2>
        {props.aside}
      </header>
      {props.children}
    </section>
  );
}

export function Notice(props: { kind?: "info" | "warn" | "bad"; children: JSX.Element }) {
  return <p class={`notice notice-${props.kind ?? "info"}`}>{props.children}</p>;
}
