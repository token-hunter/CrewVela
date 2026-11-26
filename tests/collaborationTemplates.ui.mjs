import assert from "node:assert/strict";
import { chromium } from "playwright";

const browser = await chromium.launch({ channel: "chrome" });
const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1432";
const errors = [];
try {
  const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?empty=1&natural=1&native=1`);
  const toggle = page.locator(".collab-work-toggle");
  await toggle.waitFor();
  if (await toggle.getAttribute("aria-expanded") === "false") await toggle.click();
  const shortcuts = page.getByRole("region", { name: "协作模板" });
  await shortcuts.getByRole("button", { name: "审阅 PR" }).waitFor();
  await page.evaluate(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    window.templatePlans = [];
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command !== "create_automation") return invoke(command, args);
      window.templatePlans.push(structuredClone(args.input));
      return Promise.resolve({ ...args.input, id: args.input.action.creationId, enabled: true });
    };
  });
  for (const title of ["审阅 PR", "整理 Issue", "发布前核对"]) {
    await shortcuts.getByRole("button", { name: title }).click();
    const dialog = page.getByRole("dialog", { name: title });
    assert.equal(await dialog.getByRole("button", { name: "试跑一次" }).isEnabled(), false);
    await dialog.getByRole("button", { name: "填入示例" }).click();
    await dialog.getByText("预览实际任务", { exact: true }).click();
    const prompt = await dialog.locator("pre").innerText();
    assert.ok(prompt.startsWith(title));
    assert.ok(prompt.includes(await dialog.locator("textarea").inputValue()));
    assert.deepEqual(await dialog.locator("select option").evaluateAll(options => options.map(o => o.value)), ["claude"]);
    await dialog.getByText("查看示例结果", { exact: true }).click();
    assert.ok((await dialog.locator("details p").innerText()).length > 10);
    await dialog.getByRole("button", { name: "试跑一次" }).click();
    await dialog.getByText("本次试跑已提交，成员的回复会出现在本话题。").waitFor();
    const sent = await page.evaluate(() => window.conversationMock.calls.filter(call => call.command === "ma_conversation_send").at(-1).args);
    assert.equal(sent.input.body, prompt);
    assert.equal(sent.templateTrial, true);
    assert.equal(sent.input.nativeCapabilities, false);
    assert.equal(sent.input.taskRules.tools, "research");
    assert.equal(sent.input.expectedConversationId, "conversation-A");
    assert.deepEqual(sent.input.participantAgentIds, ["claude"]);
    assert.equal(await dialog.getByRole("button", { name: "试跑一次" }).count(), 0);
    if (title === "发布前核对") {
      await dialog.getByRole("button", { name: "设置定时" }).click();
      assert.equal(await page.evaluate(() => window.templatePlans.length), 0);
      await dialog.getByLabel("本机时间").fill("08:25");
      await dialog.getByRole("button", { name: "确认启用定时" }).click();
      await dialog.getByText("计划已启用，可在自动化中查看记录或暂停。").waitFor();
      const [plan] = await page.evaluate(() => window.templatePlans);
      assert.deepEqual(plan.trigger, { kind: "weekly", weekdays: [1, 2, 3, 4, 5], hour: 8, minute: 25 });
      assert.equal(plan.action.body, prompt);
      assert.equal(plan.action.conversationId, "conversation-A");
      assert.equal(plan.action.kind, "conversation_template");
      assert.equal(plan.action.agentId, "claude");
      await dialog.getByRole("button", { name: "完成" }).click();
    } else {
      await dialog.getByRole("button", { name: "返回话题" }).click();
    }
  }
  await page.setViewportSize({ width: 390, height: 844 });
  if (await toggle.getAttribute("aria-expanded") === "false") await toggle.click();
  await shortcuts.getByRole("button", { name: "整理 Issue" }).click();
  const narrow = await page.getByRole("dialog", { name: "整理 Issue" }).boundingBox();
  assert.ok(narrow && narrow.x >= 0 && narrow.x + narrow.width <= 391);
  await page.keyboard.press("Escape");
  assert.equal(await page.getByRole("dialog", { name: "整理 Issue" }).count(), 0);
  assert.deepEqual(errors, []);
  console.log("PASS three template trials, exact preview and scope, explicit scheduling confirmation and compact dialog");
} finally {
  await browser.close();
}
