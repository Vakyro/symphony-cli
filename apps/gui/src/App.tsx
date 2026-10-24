import { createMemo, For, onCleanup, onMount, Show } from "solid-js";
import { transport } from "./ipc";
import {
  connected,
  markConnected,
  projectRoot,
  setProjectRoot,
  pushProject,
  setTheme,
  setView,
  startBus,
  theme,
  view,
  type Theme,
} from "./state";
import { Notice } from "./ui";
import { VIEWS, viewById } from "./views";

const THEMES: Theme[] = ["auto", "light", "dark"];

export default function App() {
  const groups = createMemo(() => {
    const out: { name: string; items: typeof VIEWS }[] = [];
    for (const v of VIEWS) {
      let g = out.find((x) => x.name === v.group);
      if (!g) out.push((g = { name: v.group, items: [] }));
      g.items.push(v);
    }
    return out;
  });
  const current = createMemo(() => viewById(view()));

  onMount(() => {
    const stop = startBus();
    onCleanup(stop);
    // Como `symphony`: si el daemon del proyecto no está, se arranca.
    void (projectRoot().trim() ? pushProject(projectRoot()) : Promise.resolve())
      .then(() => transport().ensureDaemon())
      .then(markConnected)
      .catch(() => undefined);
  });

  createMemo(() => {
    const t = theme();
    if (typeof document !== "undefined") {
      if (t === "auto") document.documentElement.removeAttribute("data-theme");
      else document.documentElement.setAttribute("data-theme", t);
    }
  });

  return (
    <div class="shell">
      <nav class="side" aria-label="Vistas">
        <div class="brand">Symphony</div>
        <For each={groups()}>
          {(g) => (
            <section>
              <h3>{g.name}</h3>
              <For each={g.items}>
                {(v) => (
                  <button
                    class="nav"
                    classList={{ active: v.id === current().id }}
                    aria-current={v.id === current().id ? "page" : undefined}
                    onClick={() => setView(v.id)}
                  >
                    <span class="nav-id">{v.id}</span>
                    {v.title}
                  </button>
                )}
              </For>
            </section>
          )}
        </For>
      </nav>
      <div class="main">
        <header class="top">
          <h1>
            <span class="nav-id">{current().id}</span> {current().title}
          </h1>
          <label class="root">
            Proyecto
            <input
              value={projectRoot()}
              placeholder="(el del daemon)"
              onChange={(e) => setProjectRoot(e.currentTarget.value)}
            />
          </label>
          <div class="seg" role="group" aria-label="Tema">
            <For each={THEMES}>
              {(t) => (
                <button classList={{ active: theme() === t }} onClick={() => setTheme(t)}>
                  {t}
                </button>
              )}
            </For>
          </div>
        </header>
        <Show when={!connected()}>
          <Notice kind="warn">
            Sin conexión con el daemon.{" "}
            <button class="link" onClick={() => void transport().ensureDaemon().then(markConnected)}>
              Arrancarlo
            </button>
          </Notice>
        </Show>
        <main>{current().render()}</main>
      </div>
    </div>
  );
}
