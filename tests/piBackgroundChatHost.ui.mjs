import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL ?? "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  page.setDefaultTimeout(10000);
  const errors = [];
  page.on("pageerror", error => errors.push(error.stack ?? error.message));
  await page.goto(new URL("/tests/fixtures/app-session-async-ui.html?bulk-actions=stopped", base).href);
  await page.locator(".ews-card").waitFor();
  const members = await page.evaluate(() => window.appSessionMock.bulkSessions().slice(0, 2).map((record, i) => ({
    ...record, title: ["Pi 审查", "Pi 联调"][i], multiAgentProjectId: "group", multiAgentAgentId: record.id,
    agentRuntimeKind: "pi", providerId: "multi-agent-runtime", runtimeCommandHost: true,
    backgroundChatHost: true, ptyId: i + 1, status: "running", updatedAt: Date.now(),
  })));
  const snapshot = records => page.evaluate(next => window.appSessionMock.setSessions(next), records);
  const filter = page.getByRole("button", { name: /^会话状态/ });
  const runningCount = filter.locator(".filter-count");
  const waitCount = (locator, count) => locator.filter({ hasText: new RegExp(`^${count}$`) }).waitFor();
  await snapshot(members);
  // The physical poller mounts even when the workspace has no user-facing terminals.
  await page.locator(".main-terminal-host[hidden]").waitFor({ state: "attached" });
  await waitCount(runningCount, 0);
  await page.waitForFunction(() => !document.querySelector(".main-terminal-host .pane"));
  assert.equal(await page.locator(".main-terminal-host .pane").count(), 0);
  assert.equal(await page.locator(".ews-card").isVisible(), true);

  await filter.click();
  await waitCount(page.locator('[data-filter="stopped"] .filter-count'), 0);
  await page.locator('[data-filter="all"]').click();
  await page.locator(".session-item").filter({ hasText: "Pi 审查" }).click();
  await page.locator(".pane-maximized").getByText("Pi 审查", { exact: true }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.appSessionMock.calls), []);
  await page.getByRole("button", { name: "还原会话", exact: true }).click();
  await page.locator(".ews-card").waitFor();

  // Completion keeps the host reusable. A later formal run can reuse the exact PTY.
  const failed = { ...members[1], status: "error", lastError: "fixture Pi failed" };
  await snapshot([{ ...members[0], status: "completed" }, failed]);
  await page.locator(".session-item").filter({ hasText: "Pi 审查" }).getByText("执行完毕", { exact: true }).waitFor();
  await page.locator(".session-item").filter({ hasText: "Pi 联调" }).getByText("执行失败", { exact: true }).waitFor();
  await filter.click();
  await page.locator('[data-filter="running"]').click();
  await snapshot([
    { ...members[0], backgroundChatHost: false, status: "completed" },
    failed,
    { ...members[0], id: "direct", title: "独立 Pi", providerId: "local-runtime-pi", multiAgentAgentId: null,
      multiAgentProjectId: null, runtimeCommandHost: false, backgroundChatHost: false, ptyId: 3 },
    { ...members[0], id: "other-engine", title: "其他命令宿主", agentRuntimeKind: "opencode",
      backgroundChatHost: false, ptyId: 4 },
  ]);
  await waitCount(runningCount, 3);
  for (const title of ["Pi 审查", "独立 Pi", "其他命令宿主"]) {
    await page.locator(".main-terminal-host .pane").getByText(title, { exact: true }).waitFor();
  }
  assert.equal(await page.locator(".main-terminal-host .pane").filter({ hasText: "Pi 联调" }).count(), 0);
  await filter.click();
  await page.locator('[data-filter="all"]').click();
  await page.getByRole("button", { name: "Pi 联调更多操作", exact: true }).click();
  await page.getByRole("menuitem", { name: "停止", exact: true }).click();
  await page.waitForFunction(() => window.appSessionMock.bulkCalls.some(call => call.command === "stop_session" && call.sessionId === "two"));
  await filter.click();
  await page.locator('[data-filter="stopped"]').click();
  await page.locator(".session-item").getByText("Pi 联调", { exact: true }).waitFor();
  assert.deepEqual(errors, []);
  console.log("PASS: Pi chat stays in the background, manual terminal and stop work, and foreground sessions and reuse remain visible");
} finally {
  await browser.close();
}
