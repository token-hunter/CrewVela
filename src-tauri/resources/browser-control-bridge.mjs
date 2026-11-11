// 浏览器控制桥接：初始化/工具发现只代理协议，不打开浏览器。
// 实际工具调用前请求 CrewVela 激活面板，再用 --cdp-endpoint 接管；
// 无法连接面板时保留外部配置，交给引擎在实际操作时启动浏览器。
//
// 调用形态：node bridge.mjs <engineCommand> <engineArgs...> -- <browserArgs...>
// 面板端口文件默认取本脚本旁的 panel-cdp.json（Rust 在创建面板 webview 时写入），
// 可用 CREWVELA_BROWSER_CONTROL_CONFIG 覆盖（测试用）。
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import path from "node:path";
import { readFileSync, realpathSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";

export function parseBridgeArgs(argv) {
  const separator = argv.indexOf("--");
  if (separator <= 0) return null;
  const engineCommand = argv[0];
  const engineArgs = argv.slice(1, separator);
  const browserArgs = argv.slice(separator + 1);
  if (!engineCommand) return null;
  return { engineCommand, engineArgs, browserArgs };
}

const LAUNCH_ONLY_FLAGS_WITH_VALUE = new Set(["--browser", "--executable-path"]);
const LAUNCH_ONLY_VALUE_PREFIXES = ["--browser=", "--executable-path="];
const LAUNCH_ONLY_FLAGS_ALONE = new Set(["--isolated", "--headless"]);

/** 接管模式下无意义甚至冲突的启动参数：--isolated 会强制 newContext 脱离面板
 * 页面，--browser/--executable-path 属于“启动哪个浏览器”的语义。 */
export function stripLaunchOnlyArgs(args) {
  const kept = [];
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (LAUNCH_ONLY_FLAGS_WITH_VALUE.has(arg)) {
      // "--browser msedge"（值分体）与 "--browser=msedge"（值连体）两种形态。
      if (!arg.includes("=")) i++;
      continue;
    }
    if (LAUNCH_ONLY_VALUE_PREFIXES.some((prefix) => arg.startsWith(prefix))) continue;
    if (LAUNCH_ONLY_FLAGS_ALONE.has(arg)) continue;
    kept.push(arg);
  }
  return kept;
}

/** 读取端口文件并探活。任何失败都退回外部模式。
 * - 单次探测 800ms（兼容端口已 listen 但 HTTP 未就绪的窗口）。
 * - waitMs > 0 时（面板优先自动救场场景），「连接失败/超时」按冷启动
 *   窗口处理：退避重试直到 waitMs 耗尽——WebView2 独立环境首次创建需要
 *   数秒，立刻兜底会导致 Agent 弹外部浏览器。404（端口被非 CDP 服务占用）
 *   与文件缺失/坏格式仍立即外部：等待不可能改变结果。 */
export async function resolveMode(configFile, { waitMs = 0 } = {}) {
  const deadline = Date.now() + waitMs;
  for (;;) {
    let port;
    try {
      const raw = readFileSync(configFile, "utf8");
      ({ port } = JSON.parse(raw));
    } catch {
      return { mode: "external" };
    }
    if (!Number.isInteger(port) || port <= 0 || port > 65535) {
      return { mode: "external" };
    }
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/version`, {
        signal: AbortSignal.timeout(800),
      });
      if (!response.ok) return { mode: "external" };
      return { mode: "panel", endpoint: `http://127.0.0.1:${port}` };
    } catch (error) {
      if (Date.now() >= deadline) return { mode: "external" };
      await new Promise((resolve) =>
        setTimeout(resolve, Math.min(600, Math.max(50, deadline - Date.now()))),
      );
    }
  }
}

function isFile(file) {
  try { return statSync(file).isFile(); } catch { return false; }
}

function windowsNpmEntry(wrapper) {
  const directory = path.dirname(wrapper);
  const source = readFileSync(wrapper, "utf8");
  // Resolve the known npm shim layout. User browser paths never pass through
  // cmd.exe; an unrecognized wrapper must not silently fall back to a shell.
  for (const name of ["npx-cli", "npm-cli"]) {
    const marker = new RegExp(`node_modules[\\\\/]npm[\\\\/]bin[\\\\/]${name}\\.js`, "i");
    if (!marker.test(source)) continue;
    const entry = path.join(directory, "node_modules", "npm", "bin", `${name}.js`);
    if (!isFile(entry)) throw new Error(`npm 包装器缺少 JavaScript 入口：${entry}`);
    const localNode = path.join(directory, "node.exe");
    return { command: isFile(localNode) ? localNode : process.execPath, entry };
  }
  throw new Error(`不支持安全启动此 Windows 包装器：${wrapper}`);
}

export function composeSpawn({ engineCommand, engineArgs, browserArgs, mode, endpoint, platform = process.platform }) {
  const args =
    mode === "panel"
      ? [...engineArgs, "--cdp-endpoint", endpoint, ...stripLaunchOnlyArgs(browserArgs)]
      : [...engineArgs, ...browserArgs];
  if (platform === "win32" && /\.(cmd|bat)$/i.test(engineCommand)) {
    const { command, entry } = windowsNpmEntry(engineCommand);
    return { command, args: [entry, ...args], useShell: false };
  }
  return { command: engineCommand, args, useShell: false };
}

export function needsPanel(message) {
  if (message?.method !== "tools/call") return false;
  const name = message.params?.name;
  if (typeof name !== "string" || !name.startsWith("browser_")) return false;
  if (name === "browser_close" || name === "browser_install") return false;
  if (name === "browser_tabs" && ["list", "close"].includes(message.params?.arguments?.action)) return false;
  return true;
}

/** 服务返回的端点也限制在 loopback，不能被配置引导连接远端调试接口。 */
function panelEndpoint(value) {
  if (typeof value !== "string") return false;
  try {
    const url = new URL(value);
    return url.protocol === "http:" && url.hostname === "127.0.0.1" &&
      Boolean(url.port) && url.pathname === "/" && !url.username && !url.password && !url.search && !url.hash;
  } catch { return false; }
}

export async function requestPanel(configFile, { timeoutMs = 16000 } = {}) {
  try {
    const { port, token } = JSON.parse(readFileSync(configFile, "utf8"));
    if (!Number.isInteger(port) || port < 1 || port > 65535 || typeof token !== "string" || !/^[a-zA-Z0-9-]{32,}$/.test(token)) {
      throw new Error("面板请求配置无效");
    }
    const response = await fetch(`http://127.0.0.1:${port}/ensure`, {
      method: "POST", headers: { Authorization: `Bearer ${token}` },
      signal: AbortSignal.timeout(timeoutMs), redirect: "error",
    });
    if (!response.ok) throw new Error(`面板请求 HTTP ${response.status}`);
    const result = await response.json();
    if (result.mode === "panel" && panelEndpoint(result.endpoint)) return result;
    if (result.mode !== "external") throw new Error("面板请求响应无效");
  } catch (error) {
    console.error(`[browser-control-bridge] ${error.message}，使用外部浏览器`);
  }
  return { mode: "external" };
}

async function killProcessTree(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise((resolve) => child.once("exit", resolve));
  if (process.platform === "win32" && child.pid) {
    const killer = spawn("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
      stdio: "ignore", windowsHide: true,
    });
    await new Promise((resolve) => { killer.once("exit", resolve); killer.once("error", resolve); });
  } else child.kill("SIGTERM");
  const stopped = await Promise.race([exited.then(() => true), new Promise((resolve) => setTimeout(() => resolve(false), 1500))]);
  if (!stopped) child.kill("SIGKILL");
}

/** JSON-RPC 代理：发现工具不使用浏览器，真实操作前才能决定 CDP 接管模式。 */
export class BrowserBridge {
  constructor(parsed, { ensurePanel, write, spawnEngine = spawn } = {}) {
    this.parsed = parsed;
    this.ensurePanel = ensurePanel;
    this.write = write;
    this.spawnEngine = spawnEngine;
    this.pending = new Map();
    this.clientRequests = new Map();
    this.nextId = 0;
    this.initialization = null;
    this.initialized = null;
    this.mode = { mode: "external" };
    this.used = false;
    this.closed = false;
    this.queue = Promise.resolve();
    this.bufferedNotifications = [];
    this.binding = null;
    this.activationBatch = null;
    this.start(this.mode);
  }

  start(mode) {
    if (this.closed) throw new Error("桥接已关闭");
    const { command, args, useShell } = composeSpawn({ ...this.parsed, ...mode });
    const child = this.spawnEngine(command, args, { stdio: ["pipe", "pipe", "inherit"], shell: useShell, windowsHide: true });
    this.child = child;
    const output = createInterface({ input: child.stdout });
    output.on("line", (line) => {
      if (this.child !== child) return;
      let message;
      try { message = JSON.parse(line); }
      catch { console.error("[browser-control-bridge] 引擎输出不是 JSON-RPC"); return; }
      if (message.method) this.write(message); // 引擎请求/通知，宿主响应直接回传。
      else {
        const pending = this.pending.get(message.id);
        if (pending) { this.pending.delete(message.id); clearTimeout(pending.timer); pending.resolve(message); }
      }
    });
    const failed = (error) => {
      if (this.child !== child) return;
      this.child = null;
      for (const pending of this.pending.values()) { clearTimeout(pending.timer); pending.reject(error); }
      this.pending.clear();
    };
    child.on("error", failed);
    child.stdin.on("error", failed);
    child.on("exit", () => failed(new Error("浏览器控制引擎已退出")));
  }

  send(message) {
    if (!this.child || this.closed) throw new Error("浏览器控制引擎不可用");
    this.child.stdin.write(`${JSON.stringify(message)}\n`);
  }

  request(message, timeoutMs = 0, clientRequest = null) {
    // 所有宿主请求重新编号，内部回放 ID 与宿主任意字符串/数字 ID 永不冲突。
    const id = `crewvela-proxy-${++this.nextId}`;
    if (clientRequest) clientRequest.engineId = id;
    return new Promise((resolve, reject) => {
      const timer = timeoutMs ? setTimeout(() => { this.pending.delete(id); reject(new Error("引擎初始化超时")); }, timeoutMs) : null;
      this.pending.set(id, { resolve, reject, timer });
      try { this.send({ ...message, id }); }
      catch (error) { this.pending.delete(id); clearTimeout(timer); reject(error); }
    });
  }

  async bindPanel() {
    const mode = await this.ensurePanel();
    const changed = this.mode.mode !== mode.mode || this.mode.endpoint !== mode.endpoint;
    if (!changed) return;
    const previous = this.child;
    this.child = null;
    await killProcessTree(previous);
    this.mode = mode;
    this.start(mode);
    if (this.initialization) {
      const replay = await this.request(this.initialization, 30000);
      if (replay.error) throw new Error(`引擎初始化失败：${JSON.stringify(replay.error)}`);
      if (this.initialized) this.send(this.initialized);
    }
  }

  receive(message) {
    if (this.closed) return;
    // 通知和引擎请求的响应不能排在工具请求之后，否则 cancellation/elicitation 会死锁。
    if (!message.method) {
      this.send(message);
      return;
    }
    if (message.id === undefined) {
      if (message.method === "notifications/cancelled") {
        const request = this.clientRequests.get(message.params?.requestId);
        if (!request) return; // 未知或已结束的宿主 ID 不能误取消内部回放。
        request.cancelled = true;
        if (request.engineId) {
          // MCP 接收者取消后可以不回复原请求；本地释放等待，不能堵住后续请求。
          const pending = this.pending.get(request.engineId);
          if (pending) {
            this.pending.delete(request.engineId);
            clearTimeout(pending.timer);
            pending.reject(new Error("请求已取消"));
          }
          if (this.child) this.send({ ...message, params: { ...message.params, requestId: request.engineId } });
        }
        return; // 尚在队列的请求直接标记，不触发面板或转发不存在的引擎请求。
      }
      if (message.method === "notifications/initialized") this.initialized = message;
      if (this.binding) this.bufferedNotifications.push(message);
      else this.send(message);
      return;
    }
    const clientRequest = { engineId: null, cancelled: false };
    this.clientRequests.set(message.id, clientRequest);
    let batch;
    if (needsPanel(message)) {
      batch = this.activationBatch ??= { promise: null, count: 0 };
      batch.count++;
    }
    this.queue = this.queue.then(async () => {
      if (this.closed) return;
      try {
        if (clientRequest.cancelled) throw new Error("请求已取消");
        if (message.method === "initialize") this.initialization = message;
        if (needsPanel(message)) {
          this.binding = batch.promise ??= this.bindPanel();
          try { await this.binding; }
          finally {
            this.binding = null;
            for (const notification of this.bufferedNotifications.splice(0)) this.send(notification);
          }
          if (clientRequest.cancelled) throw new Error("请求已取消");
          this.used = true;
        }
        // 尚未使用浏览器时，关闭/列出标签不应让 Playwright 懒创建外部浏览器。
        if (!this.used && message.method === "tools/call" &&
            (message.params?.name === "browser_close" ||
             (message.params?.name === "browser_tabs" && ["list", "close"].includes(message.params?.arguments?.action)))) {
          this.write({ jsonrpc: "2.0", id: message.id, result: { content: [{ type: "text", text: "浏览器尚未打开，没有可管理的标签页。" }] } });
          return;
        }
        const response = await this.request(message, 0, clientRequest);
        this.write({ ...response, id: message.id });
      } catch (error) {
        if (!this.closed) this.write({ jsonrpc: "2.0", id: message.id, error: { code: clientRequest.cancelled ? -32800 : -32603, message: error.message } });
      } finally {
        if (this.clientRequests.get(message.id) === clientRequest) this.clientRequests.delete(message.id);
        if (batch && --batch.count === 0 && this.activationBatch === batch) this.activationBatch = null;
      }
    });
    return this.queue;
  }

  async close() {
    this.closed = true;
    for (const pending of this.pending.values()) { clearTimeout(pending.timer); pending.reject(new Error("桥接已关闭")); }
    this.pending.clear();
    await killProcessTree(this.child);
  }
}

function defaultConfigFile() {
  return fileURLToPath(new URL("./panel-cdp.json", import.meta.url));
}

async function main() {
  const parsed = parseBridgeArgs(process.argv.slice(2));
  if (!parsed) {
    console.error("用法: node browser-control-bridge.mjs <engineCommand> [engineArgs...] -- [browserArgs...]");
    process.exitCode = 2;
    return;
  }
  const configFile = process.env.CREWVELA_BROWSER_CONTROL_CONFIG || defaultConfigFile();
  const requestFile = path.join(path.dirname(configFile), "panel-request.json");
  const bridge = new BrowserBridge(parsed, {
    ensurePanel: () => requestPanel(requestFile),
    write: (message) => process.stdout.write(`${JSON.stringify(message)}\n`),
  });
  const input = createInterface({ input: process.stdin });
  input.on("line", (line) => {
    try { bridge.receive(JSON.parse(line)); }
    catch (error) { process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id: null, error: { code: -32700, message: error.message } })}\n`); }
  });
  input.on("close", () => void bridge.close());
  for (const signal of ["SIGINT", "SIGTERM"]) {
    process.on(signal, () => { void bridge.close().finally(() => { process.exitCode = 0; }); input.close(); });
  }
}

function isEntryPoint() {
  if (!process.argv[1]) return false;
  try {
    // Node resolves import.meta.url through symlinks/junctions, while argv[1]
    // retains the migrated CrewVela alias. Compare the actual files on both sides.
    return realpathSync(fileURLToPath(import.meta.url)) === realpathSync(process.argv[1]);
  } catch {
    // Importing this library from a virtual or unavailable entry is not a launch.
    return false;
  }
}
if (isEntryPoint()) await main();
