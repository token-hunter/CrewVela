import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1468";
const output = process.env.CREWVELA_WORK_QUEUE_SCREENSHOTS;
if (output) mkdirSync(output, { recursive: true });
const browser = await chromium.launch({ channel: process.env.CREWVELA_BROWSER_CHANNEL || "chrome" });
const errors = [];
async function pageFor(query, viewport = { width: 1488, height: 980 }) {
  const page = await browser.newPage({ viewport });
  page.setDefaultTimeout(11000);
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?work-queue=1&project-overview=1&${query}`);
  await page.locator(".collab-work-toggle:enabled").waitFor();
  return page;
}
const card = (page, key) => page.locator(`[data-work-id="${key}"]`);
const calls = (page, command) => page.evaluate(cmd => window.conversationMock.calls.filter(item => item.command === cmd), command);

try {
  const page = await pageFor("");
  const rail = page.getByRole("complementary", { name: "任务队列" });
  await rail.waitFor();
  await card(page, "approval:installer-verify").waitFor();
  assert.equal(await rail.locator('[data-group="attention"] li').count(), 3);
  assert.match(await card(page, "approval:installer-verify").innerText(), /其他话题 · 安装流程/);
  assert.match(await card(page, "task:t-review").innerText(), /等待完成 · 构建检查/);
  assert.equal(await page.getByRole("list", { name: "成员投递回执" }).locator("li").count(), 3);
  if (output) await page.screenshot({ path: `${output}/work-queue-desktop.png` });
  await page.getByRole("list", { name: "成员投递回执" }).getByRole("button", { name: /Codex/ }).click();
  assert.equal((await calls(page, "fixture_open_session")).at(-1).args.original.runId, "release-reply");
  await page.locator(".conversation-switch-trigger").click();
  await page.getByRole("button", { name: "安装流程的会话操作" }).click();
  await page.getByRole("menuitem", { name: "重命名会话" }).click();
  const rename = page.getByRole("dialog", { name: "重命名会话" });
  await rename.getByRole("textbox").fill("安装流程复验");
  await rename.getByRole("button", { name: "保存" }).click();
  await page.getByRole("navigation", { name: "最近话题" }).getByRole("button", { name: "安装流程复验" }).waitFor();

  await card(page, "plan:installer-plan").click();
  await page.waitForFunction(() => document.activeElement?.dataset?.messageId === "old-plan-message");
  const target = await page.locator('[data-message-id="old-plan-message"]').boundingBox();
  const log = await page.getByRole("log").boundingBox();
  assert.ok(target && log && target.y < log.y + log.height && target.y + target.height > log.y);
  assert.equal((await calls(page, "ma_conversation_switch")).at(-1).args.input.conversationId, "installer-topic");
  await page.locator('[data-message-id="old-plan-message"]').getByRole("button", { name: "确认并开始执行" }).click();
  await card(page, "plan:installer-plan").waitFor({ state: "detached" });
  assert.equal((await calls(page, "ma_conversation_execute_plan")).at(-1).args.input.planId, "installer-plan");
  await page.getByRole("tab", { name: "团队聊天", exact: true }).click();
  await page.getByRole("navigation", { name: "最近话题" }).getByRole("button", { name: "发布核对" }).click();
  await page.getByRole("navigation", { name: "最近话题" }).locator('[aria-current="page"]').filter({ hasText: "发布核对" }).waitFor();
  await card(page, "approval:installer-verify").click();
  await page.waitForFunction(() => document.activeElement?.dataset?.approvalId === "installer-verify");
  const approval = page.locator('[data-work-task="installer-task"] [data-approval-id="installer-verify"]');
  await approval.getByRole("button", { name: "允许本次操作" }).click();
  await approval.waitFor({ state: "detached" });
  assert.deepEqual((await calls(page, "ma_resolve_intervention")).map(call => call.args.input.interventionId), ["gate-installer-verify"]);
  assert.equal(await page.locator('[data-work-task="installer-task"] [data-approval-id="installer-read"]').count(), 1);
  await page.getByRole("tab", { name: "团队聊天", exact: true }).click();
  assert.equal(await card(page, "approval:installer-verify").count(), 0);
  await page.locator(".collaboration-project-row").filter({ hasText: "单位换算器" }).click();
  await page.waitForFunction(() => document.querySelector(".collaboration-project-title")?.textContent === "单位换算器");
  await rail.getByText("当前没有需要你处理的事项").waitFor();
  assert.equal(await card(page, "approval:installer-read").count(), 0);

  const narrow = await pageFor("", { width: 390, height: 844 });
  assert.equal(await narrow.locator(".collab-work-rail").count(), 0);
  await narrow.getByRole("button", { name: "展开任务队列" }).click();
  const drawer = narrow.getByRole("dialog", { name: "任务队列" });
  await drawer.getByText("读取安装器签名", { exact: true }).waitFor();
  assert.equal(await narrow.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  if (output) await narrow.screenshot({ path: `${output}/work-queue-narrow.png` });
  await narrow.keyboard.press("Escape");
  await drawer.waitFor({ state: "detached" });
  await narrow.waitForFunction(() => document.activeElement?.classList.contains("collab-work-toggle"));
  await narrow.keyboard.press("Enter");
  await card(narrow, "task:t-review").click();
  await narrow.waitForFunction(() => document.activeElement?.dataset?.workTask === "t-review");

  const locked = await pageFor("locked-topic=1");
  await card(locked, "plan:installer-plan").click();
  await locked.getByRole("dialog", { name: "原始请求" }).getByText("检查安装器签名并验证安装流程。", { exact: true }).waitFor();
  assert.equal((await calls(locked, "ma_conversation_switch")).length, 0);
  const legacy = await browser.newPage({ viewport: { width: 1488, height: 980 } });
  legacy.on("pageerror", error => errors.push(error.message));
  await legacy.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?activity=1&approval=1&project-overview=1`);
  await legacy.locator(".team-chat-panel [data-approval-id='permission-claude']").waitFor();
  await legacy.evaluate(() => { window.conversationMock.activityApprovals[0].approvalType = "legacy_permission"; });
  await legacy.locator(".team-chat-panel [data-approval-id='permission-claude']").waitFor({ state: "detached" });
  await card(legacy, "approval:permission-claude").click();
  await legacy.getByRole("dialog", { name: "原始请求" }).locator('[data-approval-id="permission-claude"]').waitFor();
  assert.deepEqual(errors, []);
  console.log("PASS project-scoped work queue: exact approval, saved plan, history anchor, dependencies, receipts, responsive drawer and original request during an active turn");
} finally {
  await browser.close();
}
