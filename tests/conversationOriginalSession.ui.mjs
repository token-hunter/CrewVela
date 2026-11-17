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
  assert.deepEqual(errors, []);
  console.log("PASS: exact conversation run history, unavailable history fallback, cross-workspace terminal, and detached route");
} finally {
  await browser.close();
}
