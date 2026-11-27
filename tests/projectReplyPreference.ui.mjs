import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome" });
try {
  const page = await browser.newPage({ viewport: { width: 420, height: 850 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?empty=1&natural=1`);
  const scope = page.locator(".conversation-send-scope");
  const members = page.locator(".conversation-members-trigger");
  const setting = page.getByRole("combobox", { name: "项目默认答复成员", exact: true });
  const draft = page.getByRole("combobox", { name: "给团队发消息", exact: true });
  const project = page.getByRole("combobox", { name: "协作项目", exact: true });

  async function publish(body, preview) {
    await draft.fill(body);
    await scope.getByText(preview, { exact: true }).waitFor();
    const index = await page.evaluate(() => window.conversationMock.calls.filter(call => call.command === "ma_conversation_send").length);
    await page.getByRole("button", { name: "发送消息", exact: true }).click();
    await page.waitForFunction(index =>
      window.conversationMock.calls.filter(call => call.command === "ma_conversation_send").length > index
      && document.querySelector(".conversation-composer textarea").value === "", index);
    return page.evaluate(index => window.conversationMock.calls.filter(call => call.command === "ma_conversation_send")[index].args.input, index);
  }

  await scope.getByText("Claude Code 答复", { exact: true }).waitFor();
  await members.click();
  await setting.selectOption("kimi");
  await scope.getByText("Kimi Code 答复", { exact: true }).waitFor();
  await page.keyboard.press("Escape");
  const first = await publish("这次有哪些改动？", "Kimi Code 答复");
  assert.deepEqual(first.participantAgentIds, ["claude", "codex", "kimi"]);
  assert.deepEqual(first.chatDispatch, { kind: "assigned", agentId: "kimi" });
  assert.equal(await page.locator(".conversation-message.agent").count(), 1);
  await page.locator(".conversation-message.user").getByText("群内可见", { exact: true }).waitFor();
  await page.reload();
  await scope.getByText("Kimi Code 答复", { exact: true }).waitFor();
  await page.getByRole("button", { name: "讨论记录操作" }).click();
  await page.getByRole("menuitem", { name: "新建会话" }).click();
  await scope.getByText("Kimi Code 答复", { exact: true }).waitFor();
  await project.selectOption("B");
  await scope.getByText("文档 Codex 答复", { exact: true }).waitFor();
  await members.click();
  assert.equal(await setting.inputValue(), "");
  await page.keyboard.press("Escape");
  await project.selectOption("A");
  await scope.getByText("Kimi Code 答复", { exact: true }).waitFor();

  const mention = await publish("@Codex 请你回答", "已 @ 1 位");
  assert.deepEqual(mention.participantAgentIds, ["codex"]);
  const opinions = await publish("大家怎么看？", "全员答复 · 3 位");
  assert.deepEqual(opinions.participantAgentIds, ["claude", "codex", "kimi"]);
  assert.deepEqual(opinions.chatDispatch, { kind: "parallel" });
  const exchange = await publish("请大家互相讨论方案", "成员互聊 · 3 位");
  assert.equal(exchange.intent, "exchange");
  assert.equal(exchange.chatDispatch, undefined);
  await page.getByRole("button", { name: "发给 Codex，恢复项目默认" }).click();
  await scope.getByText("Kimi Code 答复", { exact: true }).waitFor();
  await page.evaluate(() => window.conversationMock.snapshot("A").agents.find(member => member.id === "kimi").enabled = false);
  await scope.getByText("Claude Code 答复", { exact: true }).waitFor();
  await members.click();
  await page.getByText("默认成员当前不可用，由 Claude Code 代答。", { exact: true }).waitFor();
  assert.equal(await setting.inputValue(), "kimi");
  await page.keyboard.press("Escape");
  const fallback = await publish("这个问题现在由谁答复？", "Claude Code 答复");
  assert.deepEqual(fallback.participantAgentIds, ["claude", "codex"]);
  assert.deepEqual(fallback.chatDispatch, { kind: "assigned", agentId: "claude" });
  await page.evaluate(() => window.conversationMock.snapshot("A").agents.find(member => member.id === "kimi").enabled = true);
  await scope.getByText("Kimi Code 答复", { exact: true }).waitFor();
  await members.click();
  await setting.selectOption("");
  await scope.getByText("Claude Code 答复", { exact: true }).waitFor();
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true);
  await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?empty=1&busy-chat=1`);
  await page.locator(".conversation-composer textarea").fill("排队后由谁来答？");
  await page.getByRole("button", { name: "加入发送队列" }).click();
  const queue = page.getByRole("region", { name: "消息队列" });
  await queue.getByText("发给 Claude Code", { exact: true }).waitFor();
  await members.click();
  await setting.selectOption("kimi");
  await scope.getByText("Kimi Code 答复", { exact: true }).waitFor();
  await page.keyboard.press("Escape");
  await queue.getByRole("button", { name: "排队消息操作" }).click();
  await queue.getByRole("button", { name: "编辑消息" }).click();
  await queue.getByRole("textbox", { name: "编辑排队消息" }).fill("补充一下刚才的问题");
  await queue.getByRole("button", { name: "保存" }).click();
  await queue.getByRole("textbox", { name: "编辑排队消息" }).waitFor({ state: "hidden" });
  const edited = await page.evaluate(() =>
    window.conversationMock.calls.findLast(call => call.command === "ma_conversation_change_queued" && call.args.input).args.input);
  assert.deepEqual(edited.participantAgentIds, ["claude", "codex", "kimi"]);
  assert.deepEqual(edited.chatDispatch, { kind: "assigned", agentId: "claude" });
  assert.deepEqual(errors, []);
  console.log("PASS project default replies: project isolation, reload, topics, explicit and group routing, unavailable member, queued recipient retained");
} finally {
  await browser.close();
}
