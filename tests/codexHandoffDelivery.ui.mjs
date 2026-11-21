import assert from "node:assert/strict";
import { chromium } from "playwright";
import { createServer } from "vite";

const server = await createServer({ server: { host: "127.0.0.1", port: 0 }, logLevel: "error" });
await server.listen();
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 850 } });
  page.setDefaultTimeout(15000);
  await page.addInitScript(() => {
    let current;
    Object.defineProperty(window, "appSessionMock", { get() { return current; }, set(api) {
    current = api;
    const original = { id: "one", title: "Claude 交接测试来源", kind: "claude", shell: null, claudeSessionId: "test-claude",
      agentRuntimeKind: "claude_code", providerId: "fixture", providerName: "Claude", cwd: "C:\\fixture", status: "stopped",
      ptyId: null, yolo: false, autoMode: false, permissionMode: null, titleLocked: true, exitCode: 0, lastError: null, createdAt: 1, updatedAt: 2 };
    const target = { ...original, id: "two", title: "Codex 交接测试目标", kind: "terminal", agentRuntimeKind: "codex", providerName: "Codex" };
    api.setSessions([original, target]);
    api.switchRecords = [{ id: "test-handoff", source: original, targetId: target.id, sourcePty: null, threadId: null,
      targetRuntime: "codex", preserveSource: true, sourcePosition: 0, state: "queued", historyMode: "handoff",
      context: "fixture", error: null, codexDelivery: "pending" }];
    } });
  });
  await page.goto(new URL("/tests/fixtures/app-session-async-ui.html", server.resolvedUrls.local[0]).href, { waitUntil: "commit" });
  await page.waitForFunction(() => !!window.appSessionMock, undefined, { timeout: 90000 });
  await page.locator(".session-filter-trigger").click();
  await page.getByRole("menuitemradio", { name: /全部/ }).click();
  await page.locator(".session-item").filter({ hasText: "Claude 交接测试来源" }).click();
  await page.getByText("等待 Codex CLI 接收", { exact: true }).waitFor();
  await page.getByRole("button", { name: "交接详情与撤销" }).click();
  const dialog = page.getByRole("dialog", { name: "将当前工作交接给其他 Agent" });
  await dialog.getByText(/交接资料已准备/).waitFor();
  for (const [phase, text] of [["awaiting_receipt", /正在等待 Codex 原生提交回执/], ["confirmed", /Codex 原生会话已收到交接资料/], ["confirmed_reference", /已收到交接包文件位置/]]) {
    await page.evaluate(phase => { window.appSessionMock.switchRecords[0].codexDelivery = phase; }, phase);
    await dialog.getByText(text).waitFor();
  }
  assert.equal(await dialog.getByRole("button", { name: "继续接手会话" }).count(), 0);
  await dialog.getByRole("button", { name: "关闭", exact: true }).last().click();
  await page.getByText("已交接给 Codex CLI", { exact: true }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.appSessionMock.calls), [], "viewing the source must never resume it");
  console.log("PASS: source stays read-only, dialog tracks all four Codex delivery phases");
} finally {
  await browser.close();
  await server.close();
}
