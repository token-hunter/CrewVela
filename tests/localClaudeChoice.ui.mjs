import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: process.env.CREWVELA_BROWSER_CHANNEL || "chrome" });
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  const url = query => `${base}/tests/fixtures/local-claude-choice-ui.html?ui-preview=1&${query}`;
  for (const providers of ["sample", "none"]) {
    await page.goto(url(`providers=${providers}`));
    await page.getByText("Claude Code", { exact: true }).click();
    await page.getByRole("button", { name: /使用本地 Claude Code 配置/ }).click();
    await page.getByText("本地 Claude Code：选择工作目录", { exact: true }).waitFor();
    assert.equal(await page.locator(".new-session-dialog").count(), 0);
  }
  await page.goto(url("providers=sample"));
  await page.getByText("Claude Code", { exact: true }).click();
  await page.getByRole("button", { name: /^GLM 示例/ }).click();
  await page.getByText("Provider：fixture-glm", { exact: true }).waitFor();
  for (const theme of ["dark", "light"]) {
    await page.goto(url(`theme=${theme}`));
    await page.getByText("Claude Code", { exact: true }).click();
    for (const size of [{ width: 1280, height: 900 }, { width: 687, height: 903 }]) {
      await page.setViewportSize(size);
      assert.equal(await page.getByRole("button", { name: /使用本地 Claude Code 配置/ }).isVisible(), true);
      assert.equal(await page.locator(".new-session-dialog").evaluate(e => e.scrollWidth > e.clientWidth), false);
    }
  }
  // Real App UI-preview path: choosing local config bypasses Provider credentials/count prompts.
  await page.goto(`${base}/?ui-preview=1`);
  await page.getByRole("button", { name: "新建会话", exact: true }).first().click();
  await page.getByText("Claude Code", { exact: true }).click();
  await page.locator(".new-session-dialog").getByText("Claude Code", { exact: true }).click();
  await page.getByRole("button", { name: /使用本地 Claude Code 配置/ }).click();
  await page.locator(".session-item").filter({ hasText: "Claude Code" }).first().waitFor();
  assert.equal(await page.getByRole("heading", { name: /^多开 / }).count(), 0);
  assert.equal(await page.locator(".new-session-dialog").count(), 0);
  await page.locator(".session-project-name").filter({ hasText: "crewvela" }).click();
  await page.locator('[title="在「crewvela」下新建会话"]').click();
  await page.locator(".new-session-dialog").getByText("Claude Code", { exact: true }).click();
  await page.getByRole("button", { name: /使用本地 Claude Code 配置/ }).click();
  await page.locator(".session-item").filter({ hasText: "Claude Code · crewvela" }).waitFor();
  assert.deepEqual(errors, []);
} finally { await browser.close(); }
