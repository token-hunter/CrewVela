// Focused React test with mock IPC; no native WebView is created.
import assert from "node:assert/strict";
import { chromium } from "playwright";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1200, height: 900 } });
  const errors = [];
  page.on("pageerror", error => { errors.push(error.message); console.error(error.message); });
  await page.addInitScript(() => {
    let next = 1;
    const callbacks = new Map(), listeners = new Map();
    window.__qaCalls = [];
    window.__qaViews = {};
    window.__qaEmit = (name, payload) => { for (const item of listeners.values()) if (item.event === name) callbacks.get(item.handler)({ payload }); };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_, id) => listeners.delete(id) };
    window.__TAURI_INTERNALS__ = {
      transformCallback: callback => { const id = next++; callbacks.set(id, callback); return id; },
      invoke: async (command, args = {}) => {
        window.__qaCalls.push({ command, args });
        if (command === "plugin:event|listen") { const id = next++; listeners.set(id, args); return id; }
        if (command === "plugin:event|unlisten") return;
        const id = args.panelId ?? "default";
        if (command === "browser_panel_open") {
          if (window.__qaHold === id) await new Promise(resolve => { window.__qaRelease = resolve; });
          window.__qaViews[id] ??= { url: args.url };
          window.__qaViews[id].visible = true;
          return;
        }
        if (command === "browser_panel_close") { delete window.__qaViews[id]; return; }
        if (command === "browser_panel_current_url") return window.__qaViews[id].url;
        if (command === "browser_panel_set_bounds") {
          if (window.__qaFailBounds === id) { window.__qaFailBounds = null; throw new Error("temporary bounds failure"); }
          return;
        }
        if (command === "browser_panel_set_visible") { if (window.__qaViews[id]) window.__qaViews[id].visible = args.visible; return; }
        if (command === "browser_panel_navigate") { window.__qaViews[id].url = args.url; return; }
        if (command === "browser_panel_history") return;
        throw new Error(`Unexpected IPC ${command}`);
      },
    };
  });
  const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
  const moduleSource = await (await fetch(`${base}/src/components/BrowserPanel.tsx`)).text();
  const reactUrl = moduleSource.match(/"([^" ]*\/react\.js[^" ]*)"/)[1];
  const domSource = await (await fetch(`${base}/src/main.tsx`)).text();
  const domUrl = domSource.match(/"([^" ]*\/react-dom_client\.js[^" ]*)"/)[1];
  await page.route("**/__browser-instances-qa", route => route.fulfill({ contentType: "text/html", body: `<!doctype html><html><head><style>.slot{width:900px;height:650px}.browser-host{width:900px;height:500px}</style></head><body><div id="root"></div><script type="module">import RefreshRuntime from '/@react-refresh';RefreshRuntime.injectIntoGlobalHook(window);window.$RefreshReg$=()=>{};window.$RefreshSig$=()=>type=>type;window.__vite_plugin_react_preamble_installed__=true;</script><script type="module">import React from '${reactUrl}';import ReactDOM from '${domUrl}';import {BrowserPanel} from '/src/components/BrowserPanel.tsx';const root=ReactDOM.createRoot(document.getElementById('root'));window.__qaRender=(active='a',closed=[])=>root.render(React.createElement(React.Fragment,null,...['a','b','c'].filter(id=>!closed.includes(id)).map(id=>React.createElement('div',{key:id,className:'slot',hidden:id!==active,'data-id':id},React.createElement(BrowserPanel,{panelId:id,active:id===active})))));window.__qaRender();</script></body></html>` }));
  await page.goto(`${process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420"}/__browser-instances-qa`);
  const input = id => page.locator(`[data-id="${id}"]`).getByRole("textbox", { name: "地址栏" });
  const navigate = async (id, url) => { await input(id).fill(url); await input(id).press("Enter"); await page.waitForFunction(id => !!window.__qaViews[id], id); };
  await page.evaluate(() => { window.__qaFailBounds = "a"; });
  await navigate("a", "https://example.com/a");
  await page.waitForFunction(() => window.__qaCalls.filter(x => x.command === "browser_panel_set_bounds" && x.args.panelId === "a").length === 2);
  await input("a").fill("discard this draft");
  await input("a").press("Escape");
  assert.equal(await input("a").inputValue(), "https://example.com/a");
  await page.evaluate(() => window.__qaRender("b"));
  await navigate("b", "https://example.com/b");
  await page.waitForFunction(() => window.__qaViews.a.visible === false && window.__qaViews.b.visible === true);
  await page.evaluate(() => window.__qaEmit("browser-panel-navigated", { panelId: "a", url: "https://example.com/a/next" }));
  assert.equal(await input("b").inputValue(), "https://example.com/b", "foreign navigation must not overwrite b");
  await page.evaluate(() => window.__qaRender("a"));
  assert.equal(await input("a").inputValue(), "https://example.com/a/next");
  await page.evaluate(() => { window.__qaHold = "c"; window.__qaRender("c"); });
  await input("c").fill("https://example.com/c");
  await input("c").press("Enter");
  await page.waitForFunction(() => typeof window.__qaRelease === "function");
  await page.evaluate(() => window.__qaRender("b", ["c"]));
  await page.waitForFunction(() => !document.querySelector('[data-id="c"]'));
  await page.evaluate(() => { window.__qaHold = null; window.__qaRelease(); });
  await page.waitForFunction(() => window.__qaCalls.some(x => x.command === "browser_panel_close" && x.args.panelId === "c"));
  assert.equal(await page.evaluate(() => !!window.__qaViews.c), false, "late open must release the closed instance");
  assert.equal(await page.evaluate(() => window.__qaViews.b.url), "https://example.com/b", "closing c must preserve b");
  assert.deepEqual(errors, []);
  console.log("PASS: mock IPC instance IDs, visibility, event isolation and closing during async creation");
} finally { await browser.close(); }
