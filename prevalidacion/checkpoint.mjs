// Arma un prompt de handoff SOLO desde: objetivo, git (diff + archivos nuevos),
// último comando + resultado, y cola del transcript (TODOs + último mensaje).
// Uso: node checkpoint.mjs <claude|codex> <transcript.jsonl> <repo> <task.md>  > handoff.md
import { readFileSync } from "node:fs";
import { execSync } from "node:child_process";

const [, , kind, transcript, repo, taskFile] = process.argv;
const lines = readFileSync(transcript, "utf8").split("\n").filter(Boolean).map((l) => {
  try { return JSON.parse(l); } catch { return null; }
}).filter(Boolean);

let todos = null, lastText = null, lastCmd = null, lastCmdOut = null;

if (kind === "claude") {
  const pending = new Map(); // tool_use_id -> command
  for (const e of lines) {
    const content = e.message?.content;
    if (!Array.isArray(content)) continue;
    for (const c of content) {
      if (c.type === "text" && e.type === "assistant") lastText = c.text;
      if (c.type === "tool_use" && c.name === "TodoWrite") todos = c.input.todos;
      if (c.type === "tool_use" && c.name === "Bash") pending.set(c.id, c.input.command);
      if (c.type === "tool_result" && pending.has(c.tool_use_id)) {
        lastCmd = pending.get(c.tool_use_id);
        lastCmdOut = typeof c.content === "string" ? c.content : JSON.stringify(c.content);
      }
    }
  }
} else {
  // codex exec --json: eventos item.* con item.type agent_message | command_execution | todo_list
  for (const e of lines) {
    const it = e.item;
    if (!it) continue;
    if (it.type === "agent_message") lastText = it.text;
    if (it.type === "todo_list") todos = it.items.map((t) => ({ content: t.text, status: t.completed ? "completed" : "pending" }));
    if (it.type === "command_execution" && e.type === "item.completed") {
      lastCmd = it.command;
      lastCmdOut = `${it.aggregated_output ?? ""}\n(exit ${it.exit_code})`;
    }
  }
}

const sh = (c) => execSync(c, { cwd: repo, encoding: "utf8", maxBuffer: 1 << 24 });
const status = sh("git status --porcelain");
const diff = sh("git diff");
const untracked = status.split("\n").filter((l) => l.startsWith("??")).map((l) => l.slice(3).trim());
const newFiles = untracked.flatMap((p) => {
  if (p.endsWith("/")) return sh(`git ls-files --others --exclude-standard "${p}"`).split("\n").filter(Boolean);
  return [p];
}).map((p) => `--- ${p} (archivo nuevo)\n${readFileSync(`${repo}/${p}`, "utf8")}`).join("\n\n");

const clip = (s, n) => (s && s.length > n ? s.slice(0, n) + "\n…(recortado)" : s);
const todoTxt = todos ? todos.map((t) => `- [${t.status === "completed" ? "x" : t.status === "in_progress" ? "~" : " "}] ${t.content}`).join("\n") : "(sin lista de TODOs en el transcript)";

process.stdout.write(`Retomas una tarea que otro agente de código dejó a medias (su proceso murió sin aviso). Ya estás en su worktree. No empieces de cero: revisa lo que ya existe y continúa desde donde quedó.

## Objetivo original
${readFileSync(taskFile, "utf8").trim()}

## Plan del agente anterior (del transcript; [x] hecho, [~] en curso, [ ] pendiente)
${todoTxt}

## Último mensaje del agente anterior
${clip(lastText, 1500) ?? "(ninguno)"}

## Último comando que corrió y su resultado
$ ${lastCmd ?? "(ninguno)"}
${clip(lastCmdOut, 2500) ?? ""}

## Estado de git
${status || "(limpio)"}

## Diff de archivos modificados
${diff || "(ninguno)"}

## Archivos nuevos (todavía sin commit)
${newFiles || "(ninguno)"}

Termina la tarea. Al final, todos los tests (\`npm test\`) deben pasar. No hagas commit.
`);
