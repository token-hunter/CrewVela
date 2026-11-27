import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1442";
const browser = await chromium.launch({ channel: "chrome" });
try {
  const page = await browser.newPage({ viewport: { width: 1080, height: 900 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html`);
  const request = page.locator('[data-message-id="m1"]');
  const toggle = request.getByRole("button", { name: "本轮用量", exact: true });
  await toggle.waitFor();
  assert.equal(await toggle.getAttribute("aria-expanded"), "false");
  assert.equal(await page.evaluate(() => window.conversationMock.calls.filter(c => c.command === "ma_conversation_round_metrics").length), 0);
  await page.evaluate(() => {
    const tauri = window.__TAURI_INTERNALS__;
    const original = tauri.invoke;
    window.metricCalls = [];
    tauri.invoke = (command, args, ...rest) => {
      if (command !== "ma_conversation_round_metrics") return original.call(tauri, command, args, ...rest);
      window.metricCalls.push(args);
      if (window.metricCalls.length === 2) return Promise.reject("fixture offline");
      const metrics = { starts: 1, repeatDispatches: 0, tokens: { inputTotal: 127, cacheRead: 0, output: null } };
      return Promise.resolve({ totals: metrics, tasks: [{ taskId: "t", memberId: "member", title: "群聊回复", member: "开发成员", metrics }],
        unstartedMembers: ["旁观成员"] });
    };
  });
  await toggle.click();
  const details = request.getByRole("region", { name: "本轮用量" });
  const field = key => details.locator("dl").first().locator("div").filter({ has: page.getByText(key, { exact: true }) }).locator("dd");
  await details.getByText("127", { exact: true }).first().waitFor();
  assert.equal(await field("缓存读取 token").textContent(), "0");
  assert.equal(await field("输出 token").textContent(), "未知");
  assert.deepEqual(await page.evaluate(() => window.metricCalls), [{ projectId: "A", roundId: "round-1" }]);
  await details.getByText("按任务查看", { exact: true }).click();
  await details.getByText("开发成员 · 群聊回复", { exact: true }).waitFor();
  await details.getByText("本轮未启动的成员 · 旁观成员", { exact: true }).waitFor();
  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  await details.getByRole("button", { name: "刷新用量" }).click();
  await details.getByRole("alert").getByText(/fixture offline/).waitFor();
  await details.getByRole("button", { name: "刷新用量" }).click();
  await details.getByText("127", { exact: true }).first().waitFor();
  await toggle.focus();
  await page.keyboard.press("Enter");
  assert.equal(await toggle.getAttribute("aria-expanded"), "false");
  assert.equal(await request.getByRole("region", { name: "本轮用量" }).count(), 0);
  assert.deepEqual(errors, []);
  console.log("PASS round metrics on the real conversation panel, task detail, unknown/zero, refresh, keyboard and narrow viewport");
} finally {
  await browser.close();
}
