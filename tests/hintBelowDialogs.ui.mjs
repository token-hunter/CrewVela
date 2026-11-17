// Run against Vite: node tests/hintBelowDialogs.ui.mjs
// 首次使用的「会话通信」提示卡只是引导，不能盖在弹窗之上截走弹窗里的点击。
import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  for (const [width, height] of [[687, 903], [1280, 900]]) {
    const page = await browser.newPage({ viewport: { width, height } });
    const pageErrors = [];
    page.on("pageerror", (error) => pageErrors.push(error.message));
    await page.goto(`${base}/?ui-preview=1`);
    await page.locator(".communication-sidebar-hint .communication-hint").waitFor();
    await page.getByRole("button", { name: "新建会话", exact: true }).first().click();
    await page.getByText("Claude Code", { exact: true }).click();
    await page.locator(".cli-launch-dialog").waitFor();

    const report = await page.evaluate(() => {
      const hint = document.querySelector(".communication-sidebar-hint .communication-hint").getBoundingClientRect();
      const dialog = document.querySelector(".cli-launch-dialog");
      const box = dialog.getBoundingClientRect();
      const overlaps = Math.max(0, Math.min(hint.right, box.right) - Math.max(hint.left, box.left))
        * Math.max(0, Math.min(hint.bottom, box.bottom) - Math.max(hint.top, box.top)) > 0;
      const controls = [...dialog.querySelectorAll("button, strong, label, input, select")];
      let intercepted = 0;
      for (const control of controls) {
        const r = control.getBoundingClientRect();
        if (!r.width || !r.height) continue;
        const top = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
        if (top?.closest(".communication-hint")) intercepted += 1;
      }
      // 提示卡里的点同样不能穿透到对话框上方：对话框内任意点命中的都应是对话框自身或其内容。
      return { overlaps, intercepted, controls: controls.length };
    });
    assert.ok(report.controls > 3, `${width}px: dialog should expose controls: ${JSON.stringify(report)}`);
    assert.equal(report.intercepted, 0, `${width}px: first-run hint must stay below the new-session dialog: ${JSON.stringify(report)}`);
    if (width < 1000) assert.ok(report.overlaps, `${width}px: narrow layout is expected to make hint and dialog overlap, otherwise this test proves nothing`);
    // 真实点击也要能落到对话框里。
    await page.locator(".cli-launch-dialog").getByRole("combobox", { name: "启动 Agent" }).selectOption("codex");
    assert.equal(await page.locator(".cli-launch-dialog").getByRole("combobox", { name: "启动 Agent" }).inputValue(), "codex");
    assert.deepEqual(pageErrors.filter((message) => !/transformCallback|__TAURI/.test(message)), []);
    await page.close();
  }
  console.log("PASS: first-run communication hint never covers the new-session dialog");
} finally {
  await browser.close();
}
