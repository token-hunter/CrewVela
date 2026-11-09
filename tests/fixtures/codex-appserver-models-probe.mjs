// 探针:直连 codex app-server 的 model/list,核对权威模型目录
import { spawn } from "node:child_process";
import { resolve } from "node:path";

const child = spawn("codex", ["app-server"], { stdio: ["pipe", "pipe", "pipe"], shell: true });
let buffer = "";
let nextId = 1;
const pending = new Map();
const send = (method, params) => new Promise((done, fail) => {
  const id = nextId++;
  pending.set(id, { done, fail });
  child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
});
child.stdout.on("data", (chunk) => {
  buffer += chunk.toString();
  let index;
  while ((index = buffer.indexOf("\n")) >= 0) {
    const line = buffer.slice(0, index).trim();
    buffer = buffer.slice(index + 1);
    if (!line) continue;
    let frame;
    try { frame = JSON.parse(line); } catch { continue; }
    if (frame.id && pending.has(frame.id)) {
      const { done, fail } = pending.get(frame.id);
      pending.delete(frame.id);
      frame.error ? fail(new Error(JSON.stringify(frame.error))) : done(frame.result);
    }
  }
});
child.stderr.on("data", (d) => process.stderr.write(`[codex] ${d}`));
setTimeout(() => { console.error("probe timeout"); child.kill(); process.exit(1); }, 30000).unref();
try {
  await send("initialize", { clientInfo: { name: "probe", title: "Probe", version: "0.0.1" } });
  let cursor = null;
  const models = [];
  do {
    // 适配器传 limit:null;对照默认(不传)的差异
    const page = await send("model/list", cursor ? { cursor, limit: null } : { limit: null });
    models.push(...(page.data ?? page.models ?? []));
    cursor = page.nextCursor ?? null;
    console.error(`[page] 返回 ${page.data?.length ?? 0} 个,nextCursor=${page.nextCursor ?? "无"}`);
  } while (cursor);
  console.log(`model/list 共 ${models.length} 个:`);
  for (const model of models) {
    console.log(`- ${model.id} | display=${model.displayName ?? model.display_name ?? "-"} | recommended=${model.recommended ?? "-"}`);
  }
  child.kill();
  process.exit(0);
} catch (error) {
  console.error("probe failed:", error.message ?? error);
  child.kill();
  process.exit(1);
}
