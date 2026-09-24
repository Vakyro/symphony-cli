// ADR-0005: ¿un claude -p con --input-format stream-json acepta un mensaje a media tarea?
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";

const cwd = process.argv[2];
const t0 = Date.now();
const log = [];
const p = spawn(
  "claude.cmd",
  ["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose",
   "--model", "haiku", "--allowedTools", "Bash", "Write", "--replay-user-messages"],
  { cwd, shell: true, stdio: ["pipe", "pipe", "inherit"] },
);
const send = (text) =>
  p.stdin.write(JSON.stringify({ type: "user", message: { role: "user", content: text } }) + "\n");

let buf = "";
let sentSecond = false;
p.stdout.on("data", (d) => {
  buf += d;
  let i;
  while ((i = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, i);
    buf = buf.slice(i + 1);
    let e;
    try { e = JSON.parse(line); } catch { continue; }
    const s = ((Date.now() - t0) / 1000).toFixed(1);
    if (e.type === "assistant") {
      for (const c of e.message.content) {
        if (c.type === "tool_use") log.push(`${s}s tool_use ${c.name} ${JSON.stringify(c.input).slice(0, 80)}`);
        if (c.type === "text") log.push(`${s}s text ${c.text.slice(0, 100)}`);
      }
      // En cuanto el agente empieza a trabajar, Leo le manda un segundo mensaje.
      if (!sentSecond) {
        sentSecond = true;
        send("Additional instruction from the user: also create the file second.txt containing the word two.");
        log.push(`${s}s >>> segundo mensaje enviado`);
      }
    }
    if (e.type === "user" && e.isReplay) log.push(`${s}s replay (mensaje recibido)`);
    if (e.type === "result") {
      log.push(`${s}s result ${e.subtype} turns=${e.num_turns}: ${String(e.result).slice(0, 120)}`);
      if (sentSecond && e.num_turns !== undefined) p.stdin.end();
    }
  }
});
p.on("exit", (code) => {
  log.push(`exit ${code}`);
  writeFileSync(process.argv[3], log.join("\n") + "\n");
  console.log(log.join("\n"));
});
send("Use Bash to run: sleep 10 && echo done1 . Then create the file first.txt containing the word one. Reply briefly.");
