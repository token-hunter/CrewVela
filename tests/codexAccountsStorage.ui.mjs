import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  for (const failingMethod of ["getItem", "setItem", "removeItem"]) {
    const page = await browser.newPage();
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.addInitScript(method => {
      const original = Storage.prototype[method];
      Storage.prototype[method] = function(key, ...args) {
        if (key === "crewvela:codex-restart-required") throw new DOMException("Storage unavailable", "SecurityError");
        return original.call(this, key, ...args);
      };
    }, failingMethod);
    await page.goto(`${base}/tests/fixtures/codex-accounts-ui.html`);
    await page.locator(".cid-account-row").nth(4).waitFor({ timeout: 5000 });
    if (failingMethod === "getItem") {
      await page.getByRole("alert").filter({ hasText: "读取重启提醒失败" }).waitFor();
    }
    await page.locator(".cid-account-row").last().getByRole("button", { name: "切换", exact: true }).click();
    await page.locator(".cid-message.success").filter({ hasText: "账户已切换" }).waitFor();
    assert.equal(await page.evaluate(() => window.__accountSwitchCount()), 1);
    if (failingMethod === "setItem") {
      await page.getByRole("alert").filter({ hasText: "保存重启提醒失败" }).waitFor();
    }
    if (failingMethod === "removeItem") {
      page.on("dialog", dialog => dialog.accept());
      await page.getByRole("button", { name: "重启 Codex", exact: true }).click();
      await page.getByRole("alert").filter({ hasText: "保存重启提醒失败" }).waitFor();
      await page.locator(".cid-message.success").filter({ hasText: "Codex 已重新打开" }).waitFor();
    }
    assert.equal(await page.locator(".cid-message.error").count(), 0);
    assert.deepEqual(errors, []);
    await page.close();
    console.log(`PASS: ${failingMethod} failure keeps account operations usable and reports storage separately (Mock IPC)`);
  }
} finally { await browser.close(); }
