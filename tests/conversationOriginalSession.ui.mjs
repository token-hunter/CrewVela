import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1461";
const browser = await chromium.launch({ channel: "chrome" });
const errors = [];
async function openTeam(extra = "") {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  page.setDefaultTimeout(10000);
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(`${base}/tests/fixtures/app-session-async-ui.html?original-run=1${extra}`);
  if (!await page.locator(".right-sidebar-start").isVisible()) {
    await page.getByRole("button", { name: "切换右侧栏", exact: true }).click();
  }
  await page.locator(".right-sidebar-start").getByRole("button", { name: "多智能体", exact: true }).click();
  return page;
}

async function openOriginal(page, id) {
  const message = page.locator(`[data-message-id="${id}"]`);
  await message.getByRole("button", { name: "查看活动", exact: true }).click();
  await message.getByRole("button", { name: "查看成员 A的原会话", exact: true }).click();
}

try {
  const page = await openTeam();
  const history = page.getByRole("dialog", { name: /成员 A · 原会话/ });
  await openOriginal(page, "old");
  await history.getByText("从 Claude 原会话读取的历史正文", { exact: true }).waitFor();
  assert.equal(await page.getByRole("tab", { name: "工作区 A" }).getAttribute("aria-selected"), "true");
  await history.getByRole("button", { name: "关闭", exact: true }).click();

  await openOriginal(page, "unavailable");
  await history.getByText("原会话记录暂不可用。以下为群聊保存的本次对话。", { exact: true }).waitFor();
  await history.getByText("原始用户消息", { exact: true }).waitFor();
  await history.getByText("保存的群聊回复 unavailable", { exact: true }).waitFor();
  await page.keyboard.press("Escape");
  await history.waitFor({ state: "detached" });

  await openOriginal(page, "live");
  await page.locator(".pane.pane-maximized").getByText("Claude 原终端", { exact: true }).waitFor();
  assert.equal(await page.getByRole("tab", { name: "工作区 B" }).getAttribute("aria-selected"), "true");
  assert.equal(await page.getByRole("tab", { name: "团队聊天" }).isVisible(), true);
  assert.deepEqual(await page.evaluate(() => window.appSessionMock.originalRunReads), [
    { projectId: "A", runId: "old" }, { projectId: "A", runId: "unavailable" },
  ]);
  assert.deepEqual(await page.evaluate(() => window.appSessionMock.calls), []);
  await page.close();

  const managed = await openTeam("&command-host=1");
  await openOriginal(managed, "live");
  await managed.getByRole("dialog", { name: /成员 A · 原会话/ }).getByText("原始用户消息", { exact: true }).waitFor();
  assert.equal(await managed.getByRole("tab", { name: "工作区 A" }).getAttribute("aria-selected"), "true");
  assert.deepEqual(await managed.evaluate(() => window.appSessionMock.calls), []);
  await managed.keyboard.press("Escape");
  await managed.evaluate(() => window.appSessionMock.emit("crewvela-conversation-open-session", {
    sessionId: null, original: { projectId: "A", runId: "old", agentName: "成员 A", request: "", reply: "", error: null },
  }));
  await managed.getByRole("dialog", { name: /成员 A · 原会话/ }).getByText("从 Claude 原会话读取的历史正文", { exact: true }).waitFor();
  await managed.keyboard.press("Escape");
  await managed.evaluate(() => window.appSessionMock.emit("crewvela-conversation-open-session", {
    sessionId: null, original: { projectId: "A", runId: "unavailable", source: "managed_task", agentName: "成员 A", request: "本次托管任务", reply: "本次执行摘要", error: null },
  }));
  const managedRecord = managed.getByRole("dialog", { name: /成员 A · 原会话/ });
  await managedRecord.getByText("只读记录（每段最多 200 条）", { exact: true }).waitFor();
  await managedRecord.getByText("原会话记录暂不可用。以下为任务保存的本次记录。", { exact: true }).waitFor();
  assert.deepEqual(await managed.evaluate(() => window.appSessionMock.calls), []);
  await managed.close();

  const pi = await openTeam("&pi-chat-host=1");
  await openOriginal(pi, "live");
  await pi.getByRole("dialog", { name: /成员 A · 原会话/ }).getByText("原始用户消息", { exact: true }).waitFor();
  assert.equal(await pi.getByRole("tab", { name: "工作区 A" }).getAttribute("aria-selected"), "true");
  assert.deepEqual(await pi.evaluate(() => window.appSessionMock.originalRunReads), [{ projectId: "A", runId: "live" }]);
  await pi.keyboard.press("Escape");
  await pi.evaluate(() => window.appSessionMock.emit("multi-agent-workspace-open", { projectId: "A" }));
  const project = pi.locator(".multi-agent-workspace");
  await project.locator(".ma-terminal-awaiting").waitFor();
  assert.equal(await project.locator(".pane").count(), 0);
  await pi.getByRole("button", { name: "关闭通信提示" }).click();
  await project.getByRole("button", { name: /^成员 ▾/ }).click();
  await project.locator(".ma-agent-strip").getByRole("button", { name: /成员 A/ }).click();
  await project.locator(".pane-maximized").getByText("Pi 审查", { exact: true }).waitFor();
  await project.getByRole("button", { name: "显示全部终端" }).click();
  await project.locator(".ma-terminal-awaiting").waitFor();
  assert.deepEqual(await pi.evaluate(() => window.appSessionMock.calls), []);
  assert.deepEqual(errors, []);
  console.log("PASS: exact conversation history, cross-workspace terminal, detached route, and opt-in project view of a background Pi chat host");
} finally {
  await browser.close();
}
