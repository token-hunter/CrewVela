// 探针:复现 CrewVela 与 codex-acp 的握手,转储 session/new 返回的 configOptions
import readline from "node:readline";
import { spawn } from "node:child_process";
import { resolve } from "node:path";

const bin = resolve("node_modules/.bin/codex-acp.cmd");
const child = spawn("cmd.exe", ["/d", "/s", "/c", bin], { stdio: ["pipe", "pipe", "pipe"] });
let nextId = 1;
const pending = new Map();
const send = (method, params) => new Promise((resolve, reject) => {
  const id = nextId++;
  pending.set(id, { resolve, reject });
  child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
});
child.stdout.on("data", (chunk) => {
  for (const line of chunk.toString().split("\n")) {
    if (!line.trim()) continue;
    let frame;
    try { frame = JSON.parse(line); } catch { continue; }
    if (frame.method === "session/update" && frame.params?.update?.sessionUpdate === "available_commands_update") {
      console.log("[available_commands_update]");
      for (const command of frame.params.update.availableCommands ?? []) {
        console.log(`  - name=${JSON.stringify(command.name)} hint=${JSON.stringify(command.input?.hint ?? null)}`);
      }
    }
    if (frame.method === "session/update" && frame.params?.update?.sessionUpdate === "config_option_update") {
      console.log("[late config_option_update]");
      for (const option of frame.params.update.configOptions ?? []) {
        console.log(`  - id=${option.id} currentValue=${JSON.stringify(option.currentValue)} leaves=${countLeaves(option.options)}`);
      }
    }
    if (frame.id && pending.has(frame.id)) {
      const { resolve, reject } = pending.get(frame.id);
      pending.delete(frame.id);
      frame.error ? reject(new Error(JSON.stringify(frame.error))) : resolve(frame.result);
    }
  }
});
function countLeaves(items) {
  let count = 0;
  for (const item of items ?? []) {
    if (typeof item.value === "string") count += 1;
    else count += countLeaves(item.options);
  }
  return count;
}
child.stderr.on("data", (d) => process.stderr.write(`[adapter] ${d}`));
const rl = readline.createInterface({ input: process.stdin });
rl.on("line", (line) => child.stdin.write(line + "\n"));
setTimeout(() => { console.error("probe timeout"); child.kill(); process.exit(1); }, 45000).unref();
try {
  const init = await send("initialize", { protocolVersion: 1, clientCapabilities: {}, clientInfo: { name: "probe", version: "0" } });
  console.log("agentInfo:", JSON.stringify(init.agentInfo));
  console.log("capabilities:", JSON.stringify(init.agentCapabilities));
  const loadId = process.argv[2];
  const session = loadId
    ? await send("session/load", { sessionId: loadId, cwd: process.cwd(), mcpServers: [] })
    : await send("session/new", { cwd: process.cwd(), mcpServers: [] });
  console.log("sessionId:", session.sessionId);
  console.log("configOptions:");
  for (const option of session.configOptions ?? []) {
    console.log(`- id=${option.id} currentValue=${JSON.stringify(option.currentValue)} type=${option.type} leaves=${countLeaves(option.options)}`);
    const walk = (items, depth) => {
      for (const item of items ?? []) {
        if (typeof item.value === "string") console.log(`${"  ".repeat(depth + 1)}· ${item.name ?? item.value} => ${item.value}`);
        else { console.log(`${"  ".repeat(depth + 1)}[group] ${item.name ?? ""}`); walk(item.options, depth + 1); }
      }
    };
    walk(option.options, 1);
  }
  console.log("--- 等待 8 秒观察异步 config_option_update ---");
  await new Promise((r) => setTimeout(r, 8000));
  child.kill();
  process.exit(0);
} catch (error) {
  console.error("probe failed:", error.message ?? error);
  child.kill();
  process.exit(1);
}
