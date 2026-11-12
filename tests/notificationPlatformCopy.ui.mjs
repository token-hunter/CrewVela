// Run against Vite: node tests/notificationPlatformCopy.ui.mjs
// 通知设置的说明文案必须跟随平台：macOS 不能出现 "Windows 通知"。
import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  for (const [platform, expected, forbidden] of [
    ["MacIntel", /发送系统通知/, /Windows/],
    ["Win32", /发送 Windows 通知/, /系统通知/],
  ]) {
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    await page.addInitScript((value) => Object.defineProperty(navigator, "platform", { get: () => value }), platform);
    const pageErrors = [];
    page.on("pageerror", (error) => pageErrors.push(error.message));
    await page.goto(`${base}/?ui-preview=1`);
    await page.getByRole("button", { name: "设置", exact: true }).click();
    await page.locator(".settings-nav-item").filter({ hasText: "通知" }).first().click();
    const row = page.locator(".settings-dialog").getByText("为后台任务和需要人工处理的事件", { exact: false });
    await row.waitFor();
    const text = await row.innerText();
    assert.match(text, expected, `${platform}: ${text}`);
    assert.doesNotMatch(text, forbidden, `${platform}: ${text}`);
    assert.deepEqual(pageErrors.filter((message) => !/transformCallback|__TAURI/.test(message)), []);
    await page.close();
  }
  console.log("PASS: notification settings copy follows the platform (macOS: 系统通知, Windows: Windows 通知)");
} finally {
  await browser.close();
}
