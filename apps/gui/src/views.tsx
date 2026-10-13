// Las 36 vistas de FLOW §18 (00–35). Cada una pide al daemon lo mismo que su equivalente de la
// TUI y lo pinta; las acciones llaman al mismo método IPC. Las vistas prioritarias (Home, Agent,
// Providers, System, Recovery, Tasks, Context) tienen composición propia; el resto muestra la
// respuesta del daemon con el renderizador genérico.

import { createSignal, For, Show, type JSX } from "solid-js";
import { call, type Failure } from "./ipc";
import { agentId, setAgent, setView, useCall, withRoot } from "./state";
import { Auto, Badge, Cell, Notice, Panel, Table, type Json } from "./ui";

type Obj = { [k: string]: Json };

// --- piezas comunes -------------------------------------------------------------------------

/** Una consulta al daemon con su estado de carga/error y un renderizador. */
function Query<T>(props: {
  title: string;
  method: string;
  params: () => Record<string, unknown> | null;
  children: (data: T) => JSX.Element;
  empty?: string;
}) {
  const q = useCall<T>(props.method, props.params);
  return (
    <Panel
      title={props.title}
      aside={<button class="ghost" onClick={() => q.refetch()}>Actualizar</button>}
    >
      <Show when={q.error()}>{(e) => <Notice kind="bad">{e().message}</Notice>}</Show>
      <Show when={q.data() !== undefined} fallback={<Show when={!q.error()}><p class="dim">Cargando…</p></Show>}>
        {props.children(q.data() as T)}
      </Show>
    </Panel>
  );
}

function Generic(props: { title: string; method: string; params: () => Record<string, unknown> | null }) {
  return (
    <Query<unknown> title={props.title} method={props.method} params={props.params}>
      {(d) => <Auto value={d} />}
    </Query>
  );
}

/** Para las vistas que dependen del agente elegido. */
function NeedsAgent(props: { children: (id: string) => JSX.Element }) {
  return (
    <Show
      when={agentId()}
      fallback={
        <Notice>
          Elige un agente en <button class="link" onClick={() => setView("04")}>Home</button> para ver esta vista.
        </Notice>
      }
    >
      {props.children(agentId())}
    </Show>
  );
}

function useAction() {
  const [busy, setBusy] = createSignal(false);
  const [result, setResult] = createSignal<{ ok: boolean; text: string } | undefined>();
  const run = async (method: string, params: Record<string, unknown>, okText = "Hecho.") => {
    setBusy(true);
    try {
      await call(method, params);
      setResult({ ok: true, text: okText });
      return true;
    } catch (e) {
      setResult({ ok: false, text: (e as Failure).message });
      return false;
    } finally {
      setBusy(false);
    }
  };
  const note = () => {
    const r = result();
    return r ? <Notice kind={r.ok ? "info" : "bad"}>{r.text}</Notice> : null;
  };
  return { busy, run, note };
}

// --- 04 Home --------------------------------------------------------------------------------

interface AgentRow extends Obj {
  agent_id: string;
}

function Home() {
  return (
    <div class="grid">
      <Query<{ agents: AgentRow[] }> title="Agentes" method="agent.list" params={() => withRoot({ all: true })}>
        {(d) => (
          <Show when={d.agents.length > 0} fallback={<p class="dim">Todavía no hay agentes. Crea uno en «05 New Agent».</p>}>
            <Table
              rows={d.agents}
              onPick={(r) => {
                setAgent(String(r.agent_id));
                setView("06");
              }}
            />
          </Show>
        )}
      </Query>
      <Query<{ providers: Obj[] }> title="Proveedores" method="providers.list" params={() => ({})}>
        {(d) => (
          <ul class="plain providers">
            <For each={d.providers}>
              {(p) => (
                <li>
                  <strong>{String(p.display_name)}</strong>
                  <Badge value={String((p.health as Obj)?.state ?? p.setup_state)} />
                  <span class="dim">{String(p.models)} modelos</span>
                </li>
              )}
            </For>
          </ul>
        )}
      </Query>
      <SystemStrip />
    </div>
  );
}

function SystemStrip() {
  return (
    <Query<Obj> title="Sistema" method="system.get" params={() => ({})}>
      {(s) => (
        <div class="metrics">
          <Metric label="Perfil" value={String(s.profile)} />
          <Metric label="CPU" value={pct(s.cpu_pct)} sub={`techo ${pct(s.cpu_ceiling_pct)}`} />
          <Metric label="RAM" value={pct(s.ram_pct)} sub={`techo ${pct(s.ram_ceiling_pct)}`} />
          <Metric label="Pesados" value={`${s.heavy_slots_used}/${s.heavy_slots_total}`} />
        </div>
      )}
    </Query>
  );
}

function pct(v: Json | undefined): string {
  return typeof v === "number" ? `${Math.round(v)} %` : "—";
}

function Metric(props: { label: string; value: string; sub?: string }) {
  return (
    <div class="metric">
      <span class="metric-label">{props.label}</span>
      <span class="metric-value">{props.value}</span>
      <Show when={props.sub}><span class="dim">{props.sub}</span></Show>
    </div>
  );
}

// --- 00 Chat --------------------------------------------------------------------------------

function Chat() {
  const act = useAction();
  const [text, setText] = createSignal("");
  const chat = useCall<{ agent_id: string | null }>("chat.get", () => withRoot());
  const logs = useCall<unknown>("agent.logs", () => {
    const id = chat.data()?.agent_id;
    return id ? { agent: id } : null;
  });
  const send = async (id: string) => {
    const t = text().trim();
    if (!t) return;
    if (await act.run("agent.send", { agent: id, text: t }, "Mensaje enviado.")) setText("");
  };
  return (
    <Panel title="Chat (symphony/chat)">
      <Show when={chat.data()?.agent_id} fallback={<Notice>Este proyecto todavía no tiene chat: se crea con el primer mensaje desde la TUI o el CLI.</Notice>}>
        {(id) => (
          <>
            <div class="log"><Auto value={logs.data() ?? []} /></div>
            <form class="composer" onSubmit={(e) => { e.preventDefault(); void send(id()); }}>
              <input value={text()} onInput={(e) => setText(e.currentTarget.value)} placeholder="Escribe un mensaje…" />
              <button disabled={act.busy()}>Enviar</button>
            </form>
            {act.note()}
          </>
        )}
      </Show>
    </Panel>
  );
}

// --- 05 New Agent ---------------------------------------------------------------------------

function NewAgent() {
  const act = useAction();
  const [task, setTask] = createSignal("");
  const [model, setModel] = createSignal("");
  const [profile, setProfile] = createSignal("");
  const models = useCall<{ models: Obj[] }>("models.list", () => ({}), { live: false });
  const profiles = useCall<{ profiles: Obj[] }>("profiles.list", () => ({}), { live: false });
  const submit = async (e: Event) => {
    e.preventDefault();
    if (!task().trim()) return;
    const ok = await act.run(
      "agent.create",
      withRoot({
        title: task().trim(),
        model: model() || null,
        profile: model() ? null : profile() || null,
      }),
      "Agente creado.",
    );
    if (ok) setTask("");
  };
  return (
    <Panel title="Nuevo agente">
      <form class="form" onSubmit={submit}>
        <label>
          Tarea
          <textarea rows="3" value={task()} onInput={(e) => setTask(e.currentTarget.value)} />
        </label>
        <label>
          Modelo exacto
          <select value={model()} onChange={(e) => setModel(e.currentTarget.value)}>
            <option value="">Decidir con un profile</option>
            <For each={models.data()?.models ?? []}>
              {(m) => <option value={String(m.id)}>{String(m.display_name)} · {String(m.provider)}</option>}
            </For>
          </select>
        </label>
        <label>
          Profile
          <select value={profile()} disabled={!!model()} onChange={(e) => setProfile(e.currentTarget.value)}>
            <option value="">(el predeterminado)</option>
            <For each={profiles.data()?.profiles ?? []}>
              {(p) => <option value={String(p.id)}>{String(p.id)} — {String(p.description)}</option>}
            </For>
          </select>
        </label>
        <button disabled={act.busy() || !task().trim()}>Crear agente</button>
        {act.note()}
      </form>
    </Panel>
  );
}

// --- 22/03 Providers ------------------------------------------------------------------------

function Providers(props: { detail?: boolean }) {
  const act = useAction();
  const [pick, setPick] = createSignal("");
  return (
    <>
      <Query<{ providers: Obj[] }> title={props.detail ? "Detalle del proveedor" : "Proveedores"} method="providers.list" params={() => ({})}>
        {(d) => (
          <>
            <Table
              rows={d.providers.map((p) => ({
                proveedor: p.display_name,
                estado: (p.health as Obj)?.state ?? "UNKNOWN",
                setup: p.setup_state,
                habilitado: p.enabled,
                modelos: p.models,
                agentes: p.agents_active,
                cli: p.cli_version,
                id: p.id,
              })) as Obj[]}
              onPick={(r) => setPick(String(r.id))}
            />
            <Show when={props.detail && pick()}>
              <Auto value={d.providers.find((p) => p.id === pick()) ?? null} />
            </Show>
          </>
        )}
      </Query>
      <div class="actions">
        <button disabled={act.busy()} onClick={() => void act.run("providers.refresh", {}, "Proveedores detectados de nuevo.")}>
          Detectar proveedores
        </button>
        {act.note()}
      </div>
    </>
  );
}

// --- 30 Recovery ----------------------------------------------------------------------------

function Recovery() {
  const act = useAction();
  return (
    <Query<{ items: Obj[] }> title="Centro de recuperación" method="recovery.list" params={() => withRoot()}>
      {(d) => (
        <Show when={d.items.length > 0} fallback={<p class="dim">Nada que recuperar.</p>}>
          <For each={d.items}>
            {(it) => (
              <article class="card">
                <Auto value={it} />
                <div class="actions">
                  <For each={["restart", "reclaim", "stop", "dismiss"]}>
                    {(a) => (
                      <button class="ghost" disabled={act.busy()} onClick={() => void act.run("recovery.act", { id: it.id, action: a }, `«${a}» aplicado.`)}>
                        {a}
                      </button>
                    )}
                  </For>
                </div>
              </article>
            )}
          </For>
          {act.note()}
        </Show>
      )}
    </Query>
  );
}

// --- 16/21 y otras con parámetro propio -----------------------------------------------------

function ContextSearch() {
  const [q, setQ] = createSignal("");
  const [sent, setSent] = createSignal("");
  return (
    <>
      <Panel title="Buscar en el contexto del proyecto">
        <form class="composer" onSubmit={(e) => { e.preventDefault(); setSent(q().trim()); }}>
          <input value={q()} onInput={(e) => setQ(e.currentTarget.value)} placeholder="Texto a buscar…" />
          <button>Buscar</button>
        </form>
      </Panel>
      <Show when={sent()}>
        <Generic title={`Resultados de «${sent()}»`} method="context.search" params={() => withRoot({ query: sent() })} />
      </Show>
    </>
  );
}

function Usage() {
  return (
    <div class="grid">
      <Generic title="Uso (7 días)" method="usage.get" params={() => ({ days: 7 })} />
      <Generic title="Estadísticas de contexto" method="context.stats" params={() => ({})} />
    </div>
  );
}

// --- registro -------------------------------------------------------------------------------

export interface ViewDef {
  id: string;
  title: string;
  group: string;
  render: () => JSX.Element;
}

const agentView = (title: string, method: string, extra: Record<string, unknown> = {}) => () => (
  <NeedsAgent>{(id) => <Generic title={title} method={method} params={() => ({ agent: id, ...extra })} />}</NeedsAgent>
);

export const VIEWS: ViewDef[] = [
  { id: "00", title: "Chat", group: "Inicio", render: Chat },
  { id: "01", title: "Launch / Boot", group: "Inicio", render: () => <Generic title="Proyecto" method="project.status" params={() => withRoot()} /> },
  { id: "02", title: "First-run Setup", group: "Inicio", render: FirstRun },
  { id: "03", title: "Provider Setup", group: "Inicio", render: () => <Providers /> },
  { id: "04", title: "Home / Dashboard", group: "Inicio", render: Home },
  { id: "05", title: "New Agent", group: "Agente", render: NewAgent },
  { id: "06", title: "Agent Overview", group: "Agente", render: agentView("Agente", "agent.inspect") },
  { id: "07", title: "Agent Conversation", group: "Agente", render: agentView("Conversación", "agent.logs") },
  { id: "08", title: "Agent Activity", group: "Agente", render: agentView("Actividad", "agent.activity") },
  { id: "09", title: "Agent Changes", group: "Agente", render: agentView("Cambios", "agent.diff") },
  { id: "10", title: "Agent Validation", group: "Agente", render: agentView("Validación", "merge.get") },
  { id: "11", title: "Agent Context", group: "Agente", render: agentView("Contexto del agente", "context.inspect") },
  { id: "12", title: "Agent History", group: "Agente", render: agentView("Historial", "agent.history") },
  { id: "13", title: "Model Picker", group: "Agente", render: ModelPicker },
  { id: "14", title: "Explain Route", group: "Agente", render: agentView("Por qué este modelo", "route.explain") },
  { id: "15", title: "Task List", group: "Proyecto", render: () => <Generic title="Tareas" method="task.list" params={() => withRoot()} /> },
  { id: "16", title: "Task Detail", group: "Proyecto", render: TaskDetail },
  { id: "17", title: "Milestone Review", group: "Proyecto", render: () => <Generic title="Hitos y tareas" method="task.list" params={() => withRoot()} /> },
  { id: "18", title: "System / Resources", group: "Sistema", render: SystemView },
  { id: "19", title: "Performance Profile", group: "Sistema", render: SystemView },
  { id: "20", title: "Context Overview", group: "Contexto", render: () => <Generic title="Contexto" method="context.stats" params={() => ({})} /> },
  { id: "21", title: "Context Object", group: "Contexto", render: ContextSearch },
  { id: "22", title: "Providers", group: "Sistema", render: () => <Providers /> },
  { id: "23", title: "Provider Detail", group: "Sistema", render: () => <Providers detail /> },
  { id: "24", title: "Failover Event", group: "Agente", render: agentView("Cambios de executor", "agent.history") },
  { id: "25", title: "Changes / Diff", group: "Agente", render: agentView("Diff", "agent.diff") },
  { id: "26", title: "Merge Preflight", group: "Integración", render: agentView("Preflight de merge", "merge.get") },
  { id: "27", title: "Conflict Review", group: "Integración", render: agentView("Conflictos", "merge.get") },
  { id: "28", title: "Validation", group: "Integración", render: agentView("Resultados de validación", "merge.get") },
  { id: "29", title: "Review Agent Result", group: "Integración", render: agentView("Review", "merge.get") },
  { id: "30", title: "Recovery Center", group: "Sistema", render: Recovery },
  { id: "31", title: "Settings", group: "Sistema", render: () => <Generic title="Proyecto y configuración" method="project.status" params={() => withRoot()} /> },
  { id: "32", title: "Shared Skills", group: "Contexto", render: () => <Generic title="Skills compartidas" method="skills.shared" params={() => withRoot()} /> },
  { id: "33", title: "Shared MCP / Tools", group: "Contexto", render: () => <Generic title="Plugins y herramientas" method="plugin.list" params={() => withRoot()} /> },
  { id: "34", title: "Event / Audit Log", group: "Sistema", render: agentView("Eventos", "agent.logs") },
  { id: "35", title: "Usage / Context Stats", group: "Contexto", render: Usage },
];

function FirstRun() {
  const act = useAction();
  return (
    <>
      <Generic title="Proyecto" method="project.status" params={() => withRoot()} />
      <div class="actions">
        <button disabled={act.busy()} onClick={() => void act.run("project.init", withRoot(), "Proyecto inicializado.")}>
          Inicializar Symphony en este proyecto
        </button>
        {act.note()}
      </div>
    </>
  );
}

function ModelPicker() {
  return (
    <div class="grid">
      <Query<{ models: Obj[] }> title="Modelos" method="models.list" params={() => ({})}>
        {(d) => <Table rows={d.models} />}
      </Query>
      <Query<{ profiles: Obj[] }> title="Profiles" method="profiles.list" params={() => ({})}>
        {(d) => <Table rows={d.profiles.map((p) => ({ id: p.id, descripción: p.description })) as Obj[]} />}
      </Query>
    </div>
  );
}

function TaskDetail() {
  const [task, setTask] = createSignal("");
  return (
    <>
      <Query<{ tasks: Obj[] }> title="Tareas" method="task.list" params={() => withRoot()}>
        {(d) => <Table rows={d.tasks} onPick={(r) => setTask(String(r.id ?? r.task_id ?? ""))} />}
      </Query>
      <Show when={task()}>
        <Generic title="Dependencias" method="task.deps" params={() => ({ task: task() })} />
      </Show>
    </>
  );
}

function SystemView() {
  return (
    <>
      <SystemStrip />
      <Generic title="Colas y procesos" method="system.get" params={() => ({})} />
    </>
  );
}

export function viewById(id: string): ViewDef {
  return VIEWS.find((v) => v.id === id) ?? VIEWS[4];
}

export { Cell };
