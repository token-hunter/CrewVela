import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome" });
try {
  const page = await browser.newPage({ viewport: { width: 880, height: 900 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  const open = query => page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?codex-login=1&native=1&${query}`);
  const login = page.getByRole("button", { name: "登录 Codex", exact: true });
  const retries = () => page.evaluate(() => window.conversationMock.calls.filter(call => call.command === "ma_conversation_retry_codex_members"));
  const loginPending = () => page.waitForFunction(() => window.conversationMock.pending.some(call => call.command === "ma_conversation_codex_login"));
  const releaseLogin = error => page.evaluate(error => window.conversationMock.release("ma_conversation_codex_login", true, error), error);

  await open("login-running=1&hold-login=1");
  await login.click();
  await loginPending();
  assert.equal(await login.isDisabled(), true);
  await releaseLogin();
  await page.getByText("已登录，等待本轮结束后重试失败成员").waitFor();
  assert.equal((await retries()).length, 0);
  await page.evaluate(() => { window.conversationMock.chats.A.rounds[0].status = "failed"; });
  await page.getByText("Codex 补发成功").waitFor();
  assert.equal(await page.getByText("Claude 已回复", { exact: true }).count(), 1);
  assert.deepEqual((await retries()).map(call => call.args.input), [{
    projectId: "A", expectedConversationId: "conversation-A", roundId: "round-1",
    turns: [{ messageId: "m3", runId: "run-codex" }],
  }]);
  assert.equal(await login.count(), 0);
  assert.equal(await page.evaluate(() => window.conversationMock.calls.filter(call => call.command === "ma_conversation_send").length), 0);

  await open("hold-login=1");
  await login.click();
  await loginPending();
  await releaseLogin("Codex 登录未完成");
  await page.getByRole("alert").filter({ hasText: "Codex 登录未完成" }).waitFor();
  assert.equal((await retries()).length, 0);
  assert.equal(await login.isEnabled(), true);
  await login.click();
  await loginPending();
  await page.getByRole("combobox", { name: "协作项目" }).selectOption("B");
  await releaseLogin();
  await page.getByRole("combobox", { name: "协作项目" }).selectOption("A");
  await login.waitFor();
  assert.equal((await retries()).length, 0);
  await login.click();
  await loginPending();
  await releaseLogin();
  await page.getByText("Codex 补发成功").waitFor();
  assert.equal((await retries()).length, 1);

  const editor = page.getByRole("combobox", { name: "给团队发消息", exact: true });
  const prompt = "生成分工：完成一个单位换算器";
  await open("login-plan=1&empty=1&natural=1&hold-login=1");
  await editor.fill(prompt);
  await page.getByRole("button", { name: "发送消息", exact: true }).click();
  await login.click();
  await loginPending();
  await editor.fill("改为只讨论需求");
  await releaseLogin();
  await login.waitFor({ state: "detached" });
  assert.equal(await page.evaluate(() => window.conversationMock.calls.some(c => c.command === "ma_conversation_send")), false);
  await open("login-plan=1&empty=1&natural=1");
  await editor.fill(prompt);
  await page.getByRole("button", { name: "发送消息", exact: true }).click();
  await login.click();
  await page.waitForFunction(() => window.conversationMock.calls.some(c => c.command === "ma_conversation_send"));
  const sent = await page.evaluate(() => window.conversationMock.calls.filter(c => c.command === "ma_conversation_send"));
  assert.equal(sent.length, 1);
  assert.equal(sent[0].args.input.body, prompt);
  assert.equal(sent[0].args.input.mode, "execution_plan");

  await open("login-plan=apply&plan=pending");
  await page.getByRole("button", { name: "确认并开始执行", exact: true }).click();
  await login.click();
  await page.waitForFunction(() => window.conversationMock.chats.A.executionPlans[0].status === "applied");
  const applies = await page.evaluate(() => window.conversationMock.calls.filter(c => c.command === "ma_conversation_execute_plan"));
  assert.equal(applies.length, 2);
  assert.deepEqual(applies[0].args.input, applies[1].args.input);
  assert.deepEqual(errors, []);
  console.log("PASS Codex login: selective retry, cancellation, navigation, edited draft, planning, and idempotent plan approval");
} finally {
  await browser.close();
}
