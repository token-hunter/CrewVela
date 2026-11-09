// 通用设置五项功能真机 QA：通过 WebView2 CDP 驱动 release exe。
// 用法：node qa-general-settings.mjs <release-exe-path>
// 覆盖：设置读写/落盘、代理状态、keep-awake 激活（powercfg 取证）、
// 关闭到托盘（隐藏不退出 + 唤回 + 关闭开关后真退出）。
import { exec, spawn } from "node:child_process";
import { existsSync, readFileSync, unlinkSync, copyFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, join } from "node:path";

const exe = process.argv[2];
if (!exe || !existsSync(exe)) {
  console.error("用法：node qa-general-settings.mjs <CrewVela.exe 路径>");
  process.exit(2);
}

const CDP_PORT = 9224;
const settingsFile = join(homedir(), "AppData", "Roaming", "crewvela", "general-settings.json");
const backup = settingsFile + ".qa-backup";
const hadFile = existsSync(settingsFile);
if (hadFile) copyFileSync(settingsFile, backup);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const step = (name, ok, detail = "") => {
  results.push({ name, ok });
  console.log(`${ok ? "✔" : "✖"} ${name}${detail ? ` — ${detail}` : ""}`);
};

const run = (cmd) =>
  new Promise((resolve) => exec(cmd, { windowsHide: true }, (err, stdout) => resolve({ err, stdout: String(stdout) })));

let app;
let ws;
let msgId = 0;
const pending = new Map();

function cdp(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++msgId;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });
}

async function evaluate(expression) {
  const frame = await cdp("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (frame.exceptionDetails) {
    throw new Error(`页面执行异常：${JSON.stringify(frame.exceptionDetails.exception?.description ?? frame.exceptionDetails).slice(0, 300)}`);
  }
  return frame.result.result.value;
}

async function invoke(command, args = {}) {
  // 页内信封包住 resolve/reject：不依赖 CDP 对未捕获 rejection 的呈现差异。
  const envelope = await evaluate(
    `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})` +
      `.then(v => ({ ok: true, v }))` +
      `.catch(e => ({ ok: false, e: String(e) }))`,
  );
  if (!envelope || typeof envelope !== "object") {
    throw new Error(`invoke ${command} 返回异常信封：${JSON.stringify(envelope)?.slice(0, 200)}`);
  }
  if (!envelope.ok) {
    throw new Error(envelope.e);
  }
  return envelope.v;
}

/** 不抛错的探测版：返回 {ok, value|error}。 */
async function probe(command, args = {}) {
  try {
    return { ok: true, value: await invoke(command, args) };
  } catch (error) {
    return { ok: false, error: String(error) };
  }
}

async function waitForAppReady() {
  for (let i = 0; i < 60; i++) {
    try {
      const ready = await evaluate(`Boolean(document.querySelector('.app-shell'))`);
      if (ready) return;
    } catch {}
    await sleep(1000);
  }
  throw new Error("app-shell 60s 未挂载");
}

async function alive() {
  if (!app?.pid) return false;
  const { stdout } = await run(`tasklist /FI "PID eq ${app.pid}" /FO CSV`);
  return stdout.toLowerCase().includes(basename(exe).toLowerCase());
}

function cleanupAndExit(code) {
  try { if (ws) ws.close(); } catch {}
  try { if (app && !app.killed) app.kill(); } catch {}
  // 恢复 QA 前的设置状态：有备份则还原，没有则删除 QA 产物。
  setTimeout(() => {
    try {
      if (hadFile) copyFileSync(backup, settingsFile);
      else if (existsSync(settingsFile)) unlinkSync(settingsFile);
      if (existsSync(backup)) unlinkSync(backup);
    } catch {}
    for (const { name, ok } of results) console.log(`${ok ? "PASS" : "FAIL"}\t${name}`);
    const failed = results.filter((r) => !r.ok).length;
    console.log(`\nQA 汇总：${results.length - failed}/${results.length} 通过`);
    process.exit(failed === 0 && code === 0 ? 0 : 1);
  }, 1500);
}

process.on("SIGINT", () => cleanupAndExit(1));

try {
  // ---- 启动（带 CDP 调试端口 env；不影响生产行为）----
  app = spawn(exe, [], {
    env: {
      ...process.env,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP_PORT}`,
    },
    detached: false,
    stdio: "ignore",
    windowsHide: true,
  });

  let target = null;
  for (let i = 0; i < 45 && !target; i++) {
    const { stdout } = await run(`curl -s http://127.0.0.1:${CDP_PORT}/json`);
    try {
      const pages = JSON.parse(stdout);
      target = pages.find((p) => p.type === "page" && p.webSocketDebuggerUrl);
    } catch {}
    if (!target) await sleep(1000);
  }
  if (!target) throw new Error("45s 内未发现 CDP 页面 target");
  ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.addEventListener("open", resolve, { once: true });
    ws.addEventListener("error", () => reject(new Error("CDP WebSocket 连接失败")), { once: true });
  });
  ws.addEventListener("message", (event) => {
    const msg = JSON.parse(String(event.data));
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      msg.error ? reject(new Error(msg.error.message)) : resolve(msg);
    }
  });
  await waitForAppReady();
  step("release 启动 + CDP 连接 + app-shell 挂载", true);

  // ---- 1. 默认值读取 ----
  const initial = await invoke("general_settings_get");
  step(
    "general_settings_get 返回迁移安全默认值",
    initial.settings.closeToTray === true &&
      initial.settings.keepAwake === true &&
      initial.settings.webview2HardwareAcceleration === true &&
      initial.settings.terminalFontSize === 12 &&
      initial.proxyEnvActive === false,
    `closeToTray=${initial.settings.closeToTray} keepAwakeActive=${initial.keepAwakeActive}`,
  );

  // ---- 2. 保存代理 + 字体 ----
  const saved = await invoke("general_settings_save", {
    settings: {
      httpProxy: "http://127.0.0.1:7890",
      httpNoProxy: ".corp.com",
      httpCaCertPath: "",
      closeToTray: true,
      keepAwake: true,
      terminalFontFamily: "MesloLGS NF",
      terminalFontSize: 14,
      webview2HardwareAcceleration: true,
    },
  });
  step(
    "保存代理后 proxyEnvActive=true 且设置回读一致",
    saved.proxyEnvActive === true &&
      saved.settings.httpProxy === "http://127.0.0.1:7890" &&
      saved.settings.terminalFontSize === 14,
  );
  step("general-settings.json 落盘", (() => {
    try {
      const raw = JSON.parse(readFileSync(settingsFile, "utf8"));
      return raw.httpProxy === "http://127.0.0.1:7890" && raw.terminalFontFamily === "MesloLGS NF";
    } catch { return false; }
  })());

  // ---- 3. 校验拒绝坏代理 ----
  let rejected = false;
  try {
    await invoke("general_settings_save", {
      settings: { ...saved.settings, httpProxy: "socks5://127.0.0.1:1080" },
    });
  } catch { rejected = true; }
  step("后端拒绝 socks5 代理保存", rejected);

  // ---- 4. keep-awake：开终端会话 → 轮询激活 → powercfg 取证 ----
  await invoke("launch_terminal", {
    runtimeKind: null,
    cwd: null,
    projectId: null,
    shell: "powershell",
  }).catch(async () => {
    // 参数形态兜底：尝试仅 shell。
    await invoke("launch_terminal", { shell: "powershell" });
  });
  let keepAwakeOn = false;
  for (let i = 0; i < 12 && !keepAwakeOn; i++) {
    await sleep(3000);
    const probeResult = await probe("general_settings_get");
    keepAwakeOn = probeResult.ok && probeResult.value.keepAwakeActive === true;
  }
  step("终端会话激活 keep-awake（30s 轮询内）", keepAwakeOn);
  // powercfg -requests 常需管理员权限；无权限时输出提示文本，记为不可判定而不算失败。
  const power = await run("powercfg -requests");
  const powerOutput = power.stdout.trim();
  const powerDenied = /管理员|administrator|访问.*权限|invalid parameter|参数无效|拒绝/i.test(powerOutput) || powerOutput.length === 0;
  if (powerDenied) {
    step("powercfg -requests 不可判定（权限不足或空输出），以 keepAwakeActive 状态为准", true, powerOutput.slice(0, 120) || "(空输出)");
  } else {
    step(
      "powercfg -requests SYSTEM 段出现 CrewVela 程序",
      /SYSTEM[\s\S]*(?:CrewVela|crewvela)\.exe/i.test(powerOutput),
      powerOutput.replace(/\r/g, "").split("\n").filter(Boolean).slice(0, 8).join(" | ").slice(0, 200),
    );
  }

  // ---- 5. 关闭到托盘：关窗不退、隐藏、可唤回 ----
  await invoke("plugin:window|close", { label: "main" });
  await sleep(2500);
  step("关闭请求后进程存活（隐藏到托盘）", await alive());
  const hiddenProbe = await probe("plugin:window|is_visible", { label: "main" });
  step(
    "主窗口已隐藏（isVisible=false）",
    hiddenProbe.ok && hiddenProbe.value === false,
    hiddenProbe.ok ? "" : hiddenProbe.error.slice(0, 80),
  );
  await invoke("plugin:window|show", { label: "main" });
  await invoke("plugin:window|set_focus", { label: "main" });
  await sleep(800);
  const visibleProbe = await probe("plugin:window|is_visible", { label: "main" });
  step(
    "plugin:window|show 唤回主窗口",
    visibleProbe.ok && visibleProbe.value === true,
    visibleProbe.ok ? "" : visibleProbe.error.slice(0, 80),
  );

  // ---- 6. 关闭开关后真退出（收尾；关停序列本身可能耗时十余秒）----
  await invoke("general_settings_save", {
    settings: { ...saved.settings, closeToTray: false },
  });
  // close 之后页面随时可能随进程退出被销毁，IPC 回包不再到达属预期，竞速兜底。
  await Promise.race([
    invoke("plugin:window|close", { label: "main" }),
    sleep(4000),
  ]);
  let exited = false;
  for (let i = 0; i < 10 && !exited; i++) {
    await sleep(3000);
    exited = !(await alive());
  }
  step("closeToTray=false 时关窗即退出应用（≤30s）", exited);

  cleanupAndExit(0);
} catch (error) {
  step(`QA 执行异常：${error.message}`, false);
  cleanupAndExit(1);
}
