// Run against Vite: node tests/workspaceSessionRestore.ui.mjs
import assert from "node:assert/strict";
import { chromium } from "playwright";

const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  page.setDefaultTimeout(8000);
  page.setDefaultNavigationTimeout(8000);
  await page.addInitScript(() => {
    localStorage.setItem("crewvela-workspace-tabs", JSON.stringify({
      tabs: [
        { id: "workspace-zcode", title: "工作区 1" },
        { id: "workspace-other", title: "工作区 2" },
        { id: "workspace-empty", title: "工作区 3" },
      ],
      activeTabId: "workspace-zcode",
      assignments: {
        "preview-harness-session": "workspace-zcode",
        "preview-kimi-1": "workspace-other",
      },
    }));
    localStorage.setItem("crewvela-runtime-preferences", JSON.stringify({ disabledRuntimes: [] }));
  });
  await page.goto(`${process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420"}/?ui-preview=1&harness=empty`);

  await page.locator(".native-agent-view:not([hidden]) .harness-chat").waitFor();
  console.log("STEP: preview harness selected");
  await page.getByRole("button", { name: "新建聊天" }).click();
  await page.getByRole("button", { name: "ZCode Desktop", exact: true }).click();
  const zcodeView = page.locator(".native-agent-view:not([hidden]) .zcode-chat");
  await zcodeView.waitFor();
  const zcodeDraft = zcodeView.locator('textarea[aria-label="发送给 ZCode Desktop"]');
  await zcodeDraft.fill("切换工作区后保留草稿");
  console.log("STEP: created ZCode preview chat");

  await page.getByRole("button", { name: "工作区 2", exact: true }).click();
  await page.locator(".session-item").filter({ hasText: "接口检查" }).click({ position: { x: 30, y: 12 } });
  await assertEventually(page, () => document.querySelector('.session-item.active')?.textContent?.includes("接口检查"));
  await page.locator(".main-terminal-host:not([hidden]) .pane.pane-maximized").waitFor();
  assert.equal(await page.locator(".main-terminal-host:not([hidden]) .pane:not(.pane-hidden)").count(), 1, "selected terminal must retain single-pane focus");
  console.log("STEP: selected another workspace session");

  await page.getByRole("button", { name: "工作区 3", exact: true }).click();
  await page.locator(".empty-state").waitFor();
  console.log("STEP: empty workspace stayed empty");

  await page.getByRole("button", { name: "工作区 1", exact: true }).click();
  await zcodeView.waitFor();
  assert.equal(await zcodeDraft.inputValue(), "切换工作区后保留草稿", "restoring a native view must preserve its mounted draft");
  console.log("STEP: restored ZCode after switching back");

  await page.getByRole("button", { name: "工作区 2", exact: true }).click();
  await assertEventually(page, () => document.querySelector('.session-item.active')?.textContent?.includes("接口检查"));
  await page.locator(".main-terminal-host:not([hidden]) .pane.pane-maximized").waitFor();
  assert.equal(await page.locator(".main-terminal-host:not([hidden]) .pane:not(.pane-hidden)").count(), 1, "workspace restoration must retain terminal grid focus");
  console.log("STEP: restored the other workspace's independent selection");

  await page.getByRole("button", { name: "工作区 1", exact: true }).click();
  await page.getByRole("button", { name: "关闭「ZCode Desktop」标签", exact: true }).click();
  await page.locator(".native-agent-view:not([hidden]) .harness-chat").waitFor();
  await page.getByRole("button", { name: "工作区 2", exact: true }).click();
  await page.getByRole("button", { name: "工作区 1", exact: true }).click();
  await page.locator(".native-agent-view:not([hidden]) .harness-chat").waitFor();
  assert.equal(await zcodeView.count(), 0, "closing a native chat tab must not reopen it on workspace return");
  console.log("STEP: manually closed ZCode stayed closed after switching back");

  await page.getByRole("button", { name: "关闭「DeepSeek Harness · ACP 预览」标签", exact: true }).click();
  await page.getByRole("button", { name: "工作区 2", exact: true }).click();
  await page.keyboard.press("Escape");
  await assertEventually(page, () => !document.querySelector(".session-item.active"));
  await page.getByRole("button", { name: "工作区 1", exact: true }).click();
  await page.getByRole("button", { name: "工作区 2", exact: true }).click();
  await assertEventually(page, () => !document.querySelector(".session-item.active"));
  console.log("STEP: explicit Escape clear stayed empty after switching back");

  await page.getByRole("button", { name: "工作区 2", exact: true }).click();
  await page.locator(".session-item").filter({ hasText: "接口检查" }).click({ position: { x: 30, y: 12 } });
  await assertEventually(page, () => document.querySelector('.session-item.active')?.textContent?.includes("接口检查"));
  await page.locator(".main-terminal-host:not([hidden]) .pane.pane-maximized").waitFor();
  await page.getByRole("button", { name: "工作区 3", exact: true }).click();
  await page.locator(".empty-state").waitFor();
  await page.getByRole("button", { name: "关闭页签 工作区 3", exact: true }).click();
  await assertEventually(page, () => document.querySelector('.session-item.active')?.textContent?.includes("接口检查"));
  await page.locator(".main-terminal-host:not([hidden]) .pane.pane-maximized").waitFor();
  assert.equal(await page.locator(".main-terminal-host:not([hidden]) .pane:not(.pane-hidden)").count(), 1, "closing an empty active workspace must restore the neighbor's focused terminal");
  console.log("PASS: ZCode and per-workspace selection restore; empty, closed, escaped, and neighbor states stay correct");
} finally {
  await browser.close();
}

async function assertEventually(page, predicate) {
  await page.waitForFunction(predicate);
  assert.ok(await page.evaluate(predicate));
}
