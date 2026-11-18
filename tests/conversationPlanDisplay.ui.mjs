import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome" });
try {
  const page = await browser.newPage({ viewport: { width: 860, height: 920 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?empty=1&natural=1&propose=1&planning-states=1`);
  const editor = page.getByRole("combobox", { name: /给团队/ });
  const send = page.getByRole("button", { name: "发送消息", exact: true });
  const commandCalls = command => page.evaluate(command => window.conversationMock.calls.filter(call => call.command === command), command);
  const request = "开发一个贪吃蛇小游戏，先给分工。";
  await editor.fill(request);
  await send.click();
  await page.getByText("正在拟定分工，开发尚未开始", { exact: true }).waitFor();
  const firstReply = page.locator('[data-message-id="streaming-plan"]');
  await firstReply.waitFor();
  assert.equal(await firstReply.getByText(/分工正在返回/).count(), 0);
  assert.equal(await firstReply.locator("details").count(), 0);
  assert.equal((await commandCalls("ma_conversation_execute_plan")).length, 0);

  await page.evaluate(() => window.conversationMock.failPlanning());
  const problem = page.locator(".conversation-plan-progress").filter({ hasText: "分工未通过校验，开发尚未开始" });
  await problem.waitFor();
  assert.match(await problem.innerText(), /项目内相对目录或文件：frontend\//);
  assert.equal(await firstReply.getByText("规划成员已回复").isVisible(), true);
  assert.equal(await firstReply.getByText("已完成", { exact: true }).count(), 0);
  const raw = firstReply.locator(".conversation-raw-plan");
  assert.equal(await raw.locator("pre").isVisible(), false);
  await raw.getByText("查看原始回复", { exact: true }).click();
  assert.equal(await raw.locator("pre").isVisible(), true);
  assert.match(await raw.locator("pre").innerText(), /frontend\//);
  await raw.getByText("查看原始回复", { exact: true }).click();
  mkdirSync("output/playwright", { recursive: true });
  await page.screenshot({ path: "output/playwright/conversation-plan-failure.png" });

  await editor.fill("我另外写的草稿");
  await problem.getByRole("button", { name: "编辑分工请求", exact: true }).click();
  assert.equal(await editor.inputValue(), `${request}\n\n我另外写的草稿`);
  await editor.fill("保留这条独立草稿");
  // Change the picker without altering the original request the retry should use.
  await page.getByRole("button", { name: /选择参与成员/ }).click();
  await page.getByRole("checkbox", { name: "Codex", exact: true }).uncheck();
  await page.keyboard.press("Escape");
  await problem.getByRole("button", { name: "重试生成分工", exact: true }).click();
  const proposal = page.getByRole("region", { name: "待确认分工", exact: true });
  await proposal.waitFor();
  const sends = await commandCalls("ma_conversation_send");
  assert.equal(sends.length, 2);
  assert.notEqual(sends[0].args.input.clientMessageId, sends[1].args.input.clientMessageId);
  assert.deepEqual(sends[1].args.input.participantAgentIds, sends[0].args.input.participantAgentIds);
  assert.equal(sends[1].args.input.body, request);
  assert.equal(await editor.inputValue(), "保留这条独立草稿");
  assert.equal(await problem.getByRole("button", { name: "重试生成分工" }).isDisabled(), true);
  assert.equal(await page.locator('[data-message-id="generated-proposal"] .conversation-raw-plan pre').isVisible(), false);
  assert.equal((await commandCalls("ma_conversation_execute_plan")).length, 0);

  await editor.fill("请给一个普通 JSON 示例");
  await send.click();
  const ordinaryReply = page.locator('[data-message-id="plain-json"]');
  await ordinaryReply.locator(".conversation-markdown pre code").waitFor();
  assert.equal(await ordinaryReply.locator("details").count(), 0);
  assert.equal(await ordinaryReply.getByText("已完成", { exact: true }).isVisible(), true);
  await proposal.getByRole("button", { name: "确认并开始执行", exact: true }).click();
  await page.waitForFunction(() => window.conversationMock.calls.filter(call => call.command === "ma_conversation_execute_plan").length === 1);
  assert.deepEqual(errors, []);
  console.log("PASS: no streaming JSON, actionable failure, accessible raw reply, exact retry, normal JSON and explicit approval");
} finally { await browser.close(); }
