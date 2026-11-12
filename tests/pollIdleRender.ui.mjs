// Run against Vite: node tests/pollIdleRender.ui.mjs
// 常驻轮询（Agent 接力记录 2s、token 用量 5s）内容没变时不能让整棵 App 重渲染；内容一变必须立刻反映到界面。
// IPC 每次都返回全新对象，且 token 用量的键顺序随机（Rust HashMap 的序列化顺序就是如此）。
import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 850 } });
  page.setDefaultTimeout(10000);
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  // 用 React DevTools 全局钩子统计根节点提交次数，不依赖任何应用内部实现。
  await page.addInitScript(() => {
    window.__commits = 0;
    window.__REACT_DEVTOOLS_GLOBAL_HOOK__ = {
      supportsFiber: true, renderers: new Map(), isDisabled: false,
      inject(renderer) { this.renderers.set(1, renderer); return 1; },
      onCommitFiberRoot() { window.__commits += 1; }, onCommitFiberUnmount() {}, onPostCommitFiberRoot() {}, checkDCE() {},
    };
  });
  await page.goto(new URL("/tests/fixtures/app-session-async-ui.html", base).href);
  await page.locator(".session-filter-trigger").click();
  await page.getByRole("option", { name: /全部/ }).click();
  const items = page.locator(".session-item");
  await items.first().waitFor();
  assert.equal(await items.count(), 2);

  // 真实变化之后会有一两次紧随的收尾提交（布局重算），等它们结束再开始统计空闲期。
  const idleCommits = async (ms) => {
    let last = await page.evaluate(() => window.__commits);
    for (let round = 0; round < 10; round += 1) {
      await page.waitForTimeout(700);
      const now = await page.evaluate(() => window.__commits);
      if (now === last) break;
      last = now;
    }
    const before = await page.evaluate(() => window.__commits);
    await page.waitForTimeout(ms);
    return (await page.evaluate(() => window.__commits)) - before;
  };
  const usageText = (title) => page.evaluate((needle) => [...document.querySelectorAll(".session-item")]
    .find((item) => item.textContent.includes(needle))?.querySelector(".session-usage")?.textContent.trim() ?? null, title);

  // 用量变化会显示出来，之后再变也会更新。
  await page.evaluate(() => {
    const zero = { cacheCreationTokens: 0, cacheReadTokens: 0 };
    window.appSessionMock.usages = { one: { inputTokens: 1000, outputTokens: 200, ...zero }, two: { inputTokens: 5, outputTokens: 5, ...zero } };
  });
  await page.waitForFunction(() => [...document.querySelectorAll(".session-usage")].some((node) => node.textContent.trim() === "1.2k tok"), null, { timeout: 9000 });
  assert.equal(await usageText("fixture-one"), "1.2k tok");
  await page.evaluate(() => { window.appSessionMock.usages.one.inputTokens = 4000; });
  await page.waitForFunction(() => [...document.querySelectorAll(".session-usage")].some((node) => node.textContent.trim() === "4.2k tok"), null, { timeout: 9000 });
  assert.equal(await usageText("fixture-one"), "4.2k tok");
  assert.equal(await usageText("fixture-two"), "10 tok");

  // 内容没变：新对象 + 随机键顺序，仍然不允许产生任何一次提交。
  const idleWithUsage = await idleCommits(6000);
  assert.equal(idleWithUsage, 0, `idle polls with unchanged usages re-rendered the app ${idleWithUsage} times`);

  // 接力记录变化会改变侧栏投影（one 被 two 接力取代），撤销后恢复。
  await page.evaluate(() => {
    const source = { id: "one", title: "fixture-one", kind: "terminal", shell: "powershell", claudeSessionId: "", providerId: "local-terminal", providerName: "PowerShell", cwd: "C:\\fixture", status: "stopped", ptyId: null, yolo: false, autoMode: false, permissionMode: null, titleLocked: true, exitCode: 0, lastError: null, createdAt: 0, updatedAt: 0 };
    window.appSessionMock.switchRecords = [{ id: "relay-1", source, sourcePosition: 0, sourcePty: null, targetId: "two", threadId: null, state: "done", historyMode: "handoff", context: "", error: null }];
  });
  await page.waitForFunction(() => document.querySelectorAll(".session-item").length === 1, null, { timeout: 6000 });
  assert.match(await items.first().innerText(), /fixture-two/);
  const idleWithRecord = await idleCommits(6000);
  assert.equal(idleWithRecord, 0, `idle polls with an unchanged relay record re-rendered the app ${idleWithRecord} times`);
  await page.evaluate(() => { window.appSessionMock.switchRecords = []; });
  await page.waitForFunction(() => document.querySelectorAll(".session-item").length === 2, null, { timeout: 6000 });

  assert.deepEqual(pageErrors, []);
  console.log("PASS: unchanged polls cause zero commits; changed usages and relay records still reach the UI");
} finally {
  await browser.close();
}
