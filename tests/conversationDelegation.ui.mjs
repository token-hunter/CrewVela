import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { chromium } from "playwright";

const url = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1461";
const out = "output/playwright";
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ channel: "chrome" });
const errors = [];
async function show(width, variant, height = 1058) {
  const page = await browser.newPage({ viewport: { width, height } });
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(`${url}/tests/fixtures/collaboration-conversation-ui.html?delegation=${variant}`);
  await page.getByRole("button", { name: "查看委派", exact: true }).waitFor();
  return page;
}

try {
  const page = await show(1487, "resumed");
  const link = page.getByRole("button", { name: "查看委派", exact: true });
  const editor = page.getByRole("combobox", { name: "给团队发消息", exact: true });
  await editor.fill("保留原草稿");
  await link.click();
  const drawer = page.getByRole("complementary", { name: "委派详情" });
  await drawer.waitFor();
  const bounds = await drawer.boundingBox();
  assert.ok(bounds.x > 798 && bounds.x < 832);
  assert.ok(bounds.width > 655 && bounds.width < 690);
  assert.equal(await editor.inputValue(), "保留原草稿");
  await drawer.getByText("后端已续跑", { exact: true }).waitFor();
  await drawer.getByRole("list", { name: "委派进度" }).getByText("验收通过并自动续跑").waitFor();
  const readCalls = () => page.evaluate(() => window.conversationMock.calls.filter(call => call.command === "ma_conversation_design_delivery"));
  assert.deepEqual(await readCalls(), []);
  await editor.fill("");
  await page.mouse.move(1000, 800);
  await page.screenshot({ path: `${out}/conversation-delegation-design-2.png` });
  await drawer.getByRole("button", { name: "查看交付", exact: true }).click();
  await drawer.getByRole("heading", { name: "登录接口契约", exact: true }).waitFor();
  await drawer.getByRole("heading", { name: "验收依据", exact: true }).waitFor();
  assert.deepEqual((await readCalls()).map(call => call.args), [{ projectId: "A", conversationId: "conversation-A", delegationId: "login-design" }]);
  await drawer.getByRole("button", { name: "收起交付", exact: true }).press("Escape");
  await drawer.waitFor({ state: "hidden" });
  await page.waitForFunction(() => document.activeElement?.textContent?.includes("查看委派"));
  await link.click();
  await page.getByRole("tab", { name: "任务执行", exact: true }).click();
  await drawer.waitFor({ state: "hidden" });
  await page.getByRole("tab", { name: "团队聊天", exact: true }).click();
  assert.equal(await link.isVisible(), true);

  const narrow = await show(365, "pending&theme=light", 770);
  await narrow.getByRole("button", { name: "查看委派", exact: true }).click();
  const waiting = narrow.getByRole("complementary", { name: "委派详情" });
  await waiting.getByText("等待设计交付", { exact: true }).waitFor();
  assert.equal(await waiting.getByRole("button", { name: "查看交付", exact: true }).count(), 0);
  assert.equal(await narrow.getByRole("log").isVisible(), false);
  assert.ok(await waiting.evaluate(node => node.scrollWidth <= node.clientWidth + 1));
  await narrow.screenshot({ path: `${out}/conversation-delegation-narrow.png` });
  await waiting.getByRole("button", { name: "关闭委派详情" }).click();
  await narrow.getByRole("log").waitFor();
  assert.deepEqual(errors, []);
  console.log("PASS delegation panel: 55/45 split, timeline, scoped delivery, preserved draft, keyboard dismissal, narrow and pending states");
} finally {
  await browser.close();
}
