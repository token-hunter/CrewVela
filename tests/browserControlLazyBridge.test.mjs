import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { BrowserBridge, requestPanel } from "../src-tauri/resources/browser-control-bridge.mjs";

// 真实子进程交换逐行 JSON-RPC；roots/list 故意使用与代理请求相同的字符串 ID。
const engineSource = `
import { createInterface } from 'node:readline';
import { appendFileSync } from 'node:fs';
const args = process.argv.slice(2);
const log = args[0];
const endpoint = args.includes('--cdp-endpoint') ? args[args.indexOf('--cdp-endpoint') + 1] : 'external';
appendFileSync(log, JSON.stringify({pid: process.pid, endpoint}) + '\\n');
let initialization;
let initialized = false;
let waiting;
const send = message => process.stdout.write(JSON.stringify(message) + '\\n');
createInterface({input: process.stdin}).on('line', line => {
  const m = JSON.parse(line);
  if (!m.method) {
    if (m.id === 'crewvela-proxy-1' && initialization) {
      send({jsonrpc:'2.0',id:initialization.id,result:{protocolVersion:initialization.params.protocolVersion,capabilities:{tools:{}},serverInfo:{name:'fixture',version:'1'}}});
      initialization = null;
    }
    return;
  }
  if (m.method === 'notifications/cancelled') {
    send({jsonrpc:'2.0',method:'fixture/cancellation',params:{requestId:m.params.requestId}});
    if (waiting?.id === m.params.requestId) {
      // 规范允许取消后不再回复原请求；代理必须自行释放等待。
      waiting = null;
    }
    return;
  }
  if (m.method === 'fixture/exit') { process.exit(9); }
  if ((m.method === 'tools/list' && m.params?.wait) || (m.method === 'tools/call' && m.params?.arguments?.wait)) {
    waiting = m;
    send({jsonrpc:'2.0',method:'fixture/waiting',params:{requestId:m.id}});
    return;
  }
  if (m.method === 'initialize') {
    initialization = m;
    send({jsonrpc:'2.0',id:'crewvela-proxy-1',method:'roots/list',params:{}});
  } else if (m.method === 'notifications/initialized') initialized = true;
  else if (m.method === 'tools/list') send({jsonrpc:'2.0',id:m.id,result:{tools:[{name:'browser_navigate',inputSchema:{type:'object'}}]}});
  else if (m.method === 'tools/call') {
    if (!initialized) send({jsonrpc:'2.0',id:m.id,error:{code:-32603,message:'not initialized'}});
    else send({jsonrpc:'2.0',id:m.id,result:{content:[{type:'text',text:JSON.stringify({endpoint,name:m.params.name,args:m.params.arguments})}]}});
  }
});
`;

async function fixture(t, ensurePanel) {
  const directory = mkdtempSync(path.join(tmpdir(), "cc-lazy-bridge-"));
  const engine = path.join(directory, "engine.mjs");
  const log = path.join(directory, "engines.jsonl");
  writeFileSync(engine, engineSource);
  const messages = [];
  let bridge;
  bridge = new BrowserBridge({ engineCommand: process.execPath, engineArgs: [engine, log], browserArgs: ["--browser", "chrome", "--isolated"] }, {
    ensurePanel,
    write(message) {
      if (message.method === "roots/list") bridge.receive({ jsonrpc: "2.0", id: message.id, result: { roots: [] } });
      else messages.push(message);
    },
  });
  t.after(async () => { await bridge.close(); rmSync(directory, { recursive: true, force: true }); });
  const call = async (id, method, params) => {
    await bridge.receive({ jsonrpc: "2.0", id, method, params });
    const result = messages.find((message) => message.id === id);
    assert.ok(result, `missing response ${id}`);
    assert.equal(result.error, undefined, JSON.stringify(result.error));
    return result.result;
  };
  await call("crewvela-proxy-1", "initialize", { protocolVersion: "2024-11-05", capabilities: { roots: { listChanged: true } }, clientInfo: { name: "test", version: "1" } });
  bridge.receive({ jsonrpc: "2.0", method: "notifications/initialized" });
  return { bridge, call, messages, engines: () => readFileSync(log, "utf8").trim().split("\n").map(JSON.parse) };
}

test("initialize、tools/list、未使用时close与tabs/list不打开；首次真实调用回放握手并绑定面板", { timeout: 15000 }, async (t) => {
  let activations = 0;
  const panel = { mode: "panel", endpoint: "http://127.0.0.1:54321" };
  const f = await fixture(t, async () => { activations++; return panel; });
  await f.call(2, "tools/list", {});
  await f.call(3, "tools/call", { name: "browser_close", arguments: {} });
  await f.call(4, "tools/call", { name: "browser_tabs", arguments: { action: "list" } });
  assert.equal(activations, 0);
  assert.equal(f.engines().length, 1);
  const navigation = await f.call(5, "tools/call", { name: "browser_navigate", arguments: { url: "https://example.com" } });
  assert.deepEqual(JSON.parse(navigation.content[0].text), { endpoint: panel.endpoint, name: "browser_navigate", args: { url: "https://example.com" } });
  assert.equal(activations, 1);
  assert.equal(f.engines().length, 2);
  // 隐藏后下一次操作仍请求激活，不重启或丢失同一个 engine。
  await f.call("snapshot", "tools/call", { name: "browser_snapshot", arguments: {} });
  assert.equal(activations, 2);
  assert.equal(f.engines().length, 2);
  for (const entry of f.engines().slice(0, 1)) assert.throws(() => process.kill(entry.pid, 0), /ESRCH/);
});

test("并发真实调用共享一次激活，原始字符串与数字响应ID分别保留", { timeout: 15000 }, async (t) => {
  let activations = 0;
  const f = await fixture(t, async () => { activations++; return { mode: "panel", endpoint: "http://127.0.0.1:54322" }; });
  await Promise.all([
    f.call(7, "tools/call", { name: "browser_snapshot", arguments: {} }),
    f.call("7", "tools/call", { name: "browser_navigate", arguments: { url: "https://example.org" } }),
  ]);
  assert.equal(activations, 1);
  assert.equal(f.messages.filter((message) => message.id === 7).length, 1);
  assert.equal(f.messages.filter((message) => message.id === "7").length, 1);
});

async function waitForMessage(messages, predicate) {
  const deadline = Date.now() + 3000;
  while (Date.now() < deadline) {
    const message = messages.find(predicate);
    if (message) return message;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  assert.fail("fixture notification timed out");
}

test("取消映射正在执行的requestId；队列中同值字符串ID取消不打开面板", { timeout: 15000 }, async (t) => {
  let activations = 0;
  const f = await fixture(t, async () => { activations++; return { mode: "panel", endpoint: "http://127.0.0.1:54324" }; });
  const active = f.bridge.receive({ jsonrpc: "2.0", id: 21, method: "tools/list", params: { wait: true } });
  const waiting = await waitForMessage(f.messages, (message) => message.method === "fixture/waiting");
  const queued = f.bridge.receive({ jsonrpc: "2.0", id: "21", method: "tools/call", params: { name: "browser_navigate", arguments: { url: "https://example.org" } } });
  f.bridge.receive({ jsonrpc: "2.0", method: "notifications/cancelled", params: { requestId: "21", reason: "queued cancellation" } });
  f.bridge.receive({ jsonrpc: "2.0", method: "notifications/cancelled", params: { requestId: 21, reason: "active cancellation" } });
  await Promise.all([active, queued]);
  assert.equal(f.messages.find((message) => message.id === 21).error.code, -32800);
  assert.equal(f.messages.find((message) => message.id === "21").error.code, -32800);
  assert.equal((await f.call(22, "tools/list", {})).tools[0].name, "browser_navigate");
  assert.equal(activations, 0);
  assert.equal(f.engines().length, 1);
  const notifications = f.messages.filter((message) => message.method === "fixture/cancellation");
  assert.equal(notifications.length, 1, "queued cancellation must not be sent to engine");
  assert.equal(notifications[0].params.requestId, waiting.params.requestId);
  assert.notEqual(notifications[0].params.requestId, 21);
});

test("已绑定面板的工具调用仍可直接取消，不追加激活或重启", { timeout: 15000 }, async (t) => {
  let activations = 0;
  const f = await fixture(t, async () => { activations++; return { mode: "panel", endpoint: "http://127.0.0.1:54325" }; });
  const active = f.bridge.receive({ jsonrpc: "2.0", id: "tool", method: "tools/call", params: { name: "browser_wait_for", arguments: { wait: true } } });
  const waiting = await waitForMessage(f.messages, (message) => message.method === "fixture/waiting");
  f.bridge.receive({ jsonrpc: "2.0", method: "notifications/cancelled", params: { requestId: "tool" } });
  await active;
  await f.call("after-cancellation", "tools/list", {});
  await waitForMessage(f.messages, (message) => message.method === "fixture/cancellation");
  assert.equal(f.messages.find((message) => message.id === "tool").error.code, -32800);
  assert.equal(f.messages.find((message) => message.method === "fixture/cancellation").params.requestId, waiting.params.requestId);
  assert.equal(activations, 1);
  assert.equal(f.engines().length, 2);
});

test("引擎退出释放当前等待，后续请求立即报错不挂起", { timeout: 10000 }, async (t) => {
  const f = await fixture(t, async () => ({ mode: "external" }));
  await f.bridge.receive({ jsonrpc: "2.0", id: "exit", method: "fixture/exit", params: {} });
  assert.equal(f.messages.find((message) => message.id === "exit").error.code, -32603);
  assert.equal(f.bridge.child, null);
  let timeout;
  try {
    await Promise.race([
      f.bridge.receive({ jsonrpc: "2.0", id: "after-exit", method: "tools/list", params: {} }),
      new Promise((_, reject) => { timeout = setTimeout(() => reject(new Error("request hung after engine exit")), 1000); }),
    ]);
  } finally { clearTimeout(timeout); }
  assert.equal(f.messages.find((message) => message.id === "after-exit").error.code, -32603);
  assert.match(f.messages.find((message) => message.id === "after-exit").error.message, /不可用/);
});

test("设置不优先面板或服务失败时保持外部配置；close清理engine", { timeout: 15000 }, async (t) => {
  const f = await fixture(t, async () => ({ mode: "external" }));
  const result = await f.call(9, "tools/call", { name: "browser_navigate", arguments: { url: "https://example.org" } });
  assert.equal(JSON.parse(result.content[0].text).endpoint, "external");
  assert.equal(f.engines().length, 1);
  const pid = f.engines()[0].pid;
  await f.bridge.close();
  assert.throws(() => process.kill(pid, 0), /ESRCH/);
});

test("请求服务带随机token，仅接受loopback面板响应；超时与无效端点安全回退", { timeout: 10000 }, async (t) => {
  const directory = mkdtempSync(path.join(tmpdir(), "cc-panel-request-"));
  const config = path.join(directory, "panel-request.json");
  const token = "12345678-1234-4321-abcd-123456789012";
  let mode = "panel";
  let authorization;
  const server = createServer((request, response) => {
    authorization = request.headers.authorization;
    assert.equal(request.method, "POST");
    assert.equal(request.url, "/ensure");
    if (mode === "timeout") return;
    response.end(JSON.stringify(mode === "external" ? { mode } : { mode: "panel", endpoint: mode === "invalid" ? "https://evil.example" : "http://127.0.0.1:54323" }));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => { server.closeAllConnections(); server.close(); rmSync(directory, { recursive: true, force: true }); });
  writeFileSync(config, JSON.stringify({ port: server.address().port, token }));
  assert.deepEqual(await requestPanel(config), { mode: "panel", endpoint: "http://127.0.0.1:54323" });
  assert.equal(authorization, `Bearer ${token}`);
  mode = "external";
  assert.deepEqual(await requestPanel(config), { mode: "external" });
  mode = "invalid";
  assert.deepEqual(await requestPanel(config), { mode: "external" });
  mode = "timeout";
  assert.deepEqual(await requestPanel(config, { timeoutMs: 100 }), { mode: "external" });
});

test("真实bridge入口stdin关闭后退出并清理子engine", { timeout: 10000 }, async (t) => {
  const directory = mkdtempSync(path.join(tmpdir(), "cc-bridge-entry-"));
  const engine = path.join(directory, "engine.mjs");
  const log = path.join(directory, "engines.jsonl");
  writeFileSync(engine, engineSource);
  const child = spawn(process.execPath, ["src-tauri/resources/browser-control-bridge.mjs", process.execPath, engine, log, "--", "--isolated"], { stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
  let stderr = "";
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  t.after(() => { if (child.exitCode === null) child.kill(); rmSync(directory, { recursive: true, force: true }); });
  const exited = new Promise((resolve) => child.once("exit", (code) => resolve(code)));
  const response = new Promise((resolve) => {
    createInterface({ input: child.stdout }).on("line", (line) => {
      const message = JSON.parse(line);
      if (message.method === "roots/list") child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id: message.id, result: { roots: [] } })}\n`);
      else if (message.id === 1) resolve(message);
    });
  });
  child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: "entry-test", version: "1" } } })}\n`);
  assert.equal((await response).error, undefined);
  const pid = JSON.parse(readFileSync(log, "utf8").trim()).pid;
  child.stdin.end();
  assert.equal(await exited, 0, stderr);
  assert.throws(() => process.kill(pid, 0), /ESRCH/);
});
