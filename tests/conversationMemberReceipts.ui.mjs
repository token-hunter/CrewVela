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
  await request.waitFor();
  await page.evaluate(() => {
    const tauri = window.__TAURI_INTERNALS__;
    const invoke = tauri.invoke;
    window.privateDelivery = "queued";
    tauri.invoke = async (command, args, ...rest) => {
      const response = await invoke.call(tauri, command, args, ...rest);
      if (command !== "ma_conversation_snapshot" || args.projectId !== "A") return response;
      return {
        ...response,
        memberReceipts: { m1: [
          { messageId: "hidden-status-id", senderId: "claude", agentId: "kimi", runId: null,
            purpose: "status", status: "inbox", arguments: { body: "hidden status payload" }, error: null },
          { messageId: "directed-question-id", senderId: "claude", agentId: "codex",
            runId: window.privateDelivery === "queued" ? null : "private-question-run",
            purpose: "question", status: window.privateDelivery, arguments: { body: "private question payload" },
            error: window.privateDelivery === "failed" ? "private runtime detail" : null },
        ] },
      };
    };
  });
  const progress = request.getByRole("list", { name: "成员协作进度" });
  await progress.getByText("已排队", { exact: true }).waitFor();
  await page.locator('[data-message-id="m2"]').getByText(/我来整理验收项/).waitFor();
  assert.equal(await progress.getByRole("listitem").count(), 1, "the passive status stays in details");
  const details = request.locator(".conversation-member-receipts");
  await details.locator("pre").first().waitFor({ state: "hidden" });
  await details.getByText("查看活动", { exact: true }).click();
  await details.getByText(/hidden-status-id/).waitFor();
  await details.getByText(/directed-question-id/).waitFor();
  await details.getByText("查看活动", { exact: true }).click();

  await page.evaluate(() => { window.privateDelivery = "completed"; });
  await progress.getByText("已回复", { exact: true }).waitFor();
  await page.evaluate(() => { window.privateDelivery = "failed"; });
  const runRecord = progress.getByRole("button", { name: /失败.*查看运行记录/ });
  await runRecord.waitFor();
  await runRecord.click();
  assert.equal(await page.evaluate(() => window.conversationMock.calls.filter(c => c.command === "fixture_open_session").at(-1)?.args.original.runId), "private-question-run");
  await details.getByText("查看活动", { exact: true }).click();
  await details.getByText("private runtime detail", { exact: true }).waitFor();
  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  assert.deepEqual(errors, []);
  console.log("PASS conversation receipt details, silent status, queued and completed delivery, and exact failure run");
} finally {
  await browser.close();
}
