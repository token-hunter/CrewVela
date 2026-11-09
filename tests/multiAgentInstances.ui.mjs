import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import { chromium } from "playwright";
const source = readFileSync("src/components/MultiAgentControl.tsx", "utf8");
const fixture = vm.runInNewContext("(" + source.match(/const MULTI_AGENT_UI_PREVIEW_SNAPSHOT: MultiAgentSnapshot = (\{[\s\S]*?\n\});/)[1] + ")");
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const errors = []; page.on("pageerror", e => errors.push(e.message));
  await page.addInitScript(snapshot => {
    let next = 1; const callbacks = new Map(), listeners = new Map();
    window.__qaSnapshot = snapshot; window.__qaCalls = [];
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_, id) => listeners.delete(id) };
    window.__TAURI_INTERNALS__ = {
      transformCallback: callback => { const id = next++; callbacks.set(id, callback); return id; },
      invoke: async (cmd, args = {}) => {
        window.__qaCalls.push({ cmd, args });
        if (cmd === "plugin:event|listen") { const id = next++; listeners.set(id, args); return id; }
        if (cmd === "plugin:event|unlisten") return;
        if (cmd === "get_runtime_preferences") return { disabledRuntimes: [] };
        if (cmd === "ma_probe_runtimes") return [];
        if (cmd === "ma_list_projects") return [window.__qaSnapshot.project];
        if (cmd === "ma_snapshot") return structuredClone(window.__qaSnapshot);
        if (cmd === "ma_approve_task") { window.__qaSnapshot.tasks.find(x => x.id === args.taskId).status = "done"; return; }
        if (cmd === "ma_start_project") return { projectId: window.__qaSnapshot.project.id, started: true };
        throw new Error(`Unexpected IPC ${cmd}`);
      },
    };
  }, fixture);
  const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
  const reactUrl = (await (await fetch(`${base}/src/components/MultiAgentControl.tsx`)).text()).match(/"([^" ]*\/react\.js[^" ]*)"/)[1];
  const domUrl = (await (await fetch(`${base}/src/main.tsx`)).text()).match(/"([^" ]*\/react-dom_client\.js[^" ]*)"/)[1];
  await page.route("**/__multi-instances-qa", route => route.fulfill({ contentType: "text/html", body: `<!doctype html><div id="root"></div><script type="module">import RefreshRuntime from '/@react-refresh';RefreshRuntime.injectIntoGlobalHook(window);window.$RefreshReg$=()=>{};window.$RefreshSig$=()=>type=>type;window.__vite_plugin_react_preamble_installed__=true;</script><script type="module">import React from '${reactUrl}';import ReactDOM from '${domUrl}';import {MultiAgentControl} from '/src/components/MultiAgentControl.tsx';const root=ReactDOM.createRoot(document.getElementById('root'));window.__qaRender=(active='a',ids=['a'])=>root.render(React.createElement(React.Fragment,null,...ids.map(id=>React.createElement('div',{key:id,hidden:id!==active,'data-id':id},React.createElement(MultiAgentControl,{active:id===active,focusedProjectId:'ui-preview-project',voiceWakeEnabled:false})))));window.__qaRender();</script>` }));
  await page.clock.install();
  await page.goto(`${base}/__multi-instances-qa`);
  const mode = id => page.locator(`[data-id="${id}"]`).getByRole("button", { name: "协作自动确认模式" });
  await mode("a").click();
  await page.evaluate(() => window.__qaRender("b", ["a", "b"]));
  await page.waitForFunction(() => document.querySelector('[data-id="b"] button[aria-label="协作自动确认模式"]')?.getAttribute("aria-pressed") === "true");
  await mode("b").click();
  await page.waitForFunction(() => [...document.querySelectorAll('button[aria-label="协作自动确认模式"]')].every(x => x.getAttribute("aria-pressed") === "false"));
  await page.evaluate(() => {
    const task = window.__qaSnapshot.tasks[1]; task.status = "in_review";
    window.__qaSnapshot.project.status = "running";
    window.__qaSnapshot.runs = [{ id: "ready-run", taskId: task.id, attempt: 1, status: "succeeded" }];
  });
  await page.clock.runFor(2500);
  assert.equal(await page.evaluate(() => window.__qaCalls.filter(x => x.cmd === "ma_approve_task").length), 0, "manual in b must prevent hidden a from approving");
  await mode("b").click();
  await page.waitForFunction(() => window.__qaCalls.filter(x => x.cmd === "ma_approve_task").length === 1);
  await page.clock.runFor(2500);
  assert.equal(await page.evaluate(() => window.__qaCalls.filter(x => x.cmd === "ma_approve_task").length), 1, "only active panel approves once");
  await page.evaluate(() => { window.__qaSnapshot.tasks[1].status = "in_review"; window.__qaSnapshot.runs[0].id = "second-run"; window.__qaRender(null, ["a", "b"]); });
  await page.clock.runFor(2500);
  assert.equal(await page.evaluate(() => window.__qaCalls.filter(x => x.cmd === "ma_approve_task").length), 1, "hidden panels must not approve");
  assert.deepEqual(errors, []);
  console.log("PASS: real MultiAgentControl with mock IPC; two instances share Auto off, only active instance approves, hidden instances never approve");
} finally { await browser.close(); }
