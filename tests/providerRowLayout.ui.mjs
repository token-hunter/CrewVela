// Run against Vite: node tests/providerRowLayout.ui.mjs
// 模型设置的服务行：操作按钮不能越出卡片，「编辑」不能被挤成竖排。
import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  for (const width of [820, 900, 1001, 1024, 1060, 1090, 1100, 1280, 1600]) {
    const page = await browser.newPage({ viewport: { width, height: 800 } });
    const pageErrors = [];
    page.on("pageerror", (error) => pageErrors.push(error.message));
    await page.goto(`${base}/?ui-preview=1`);
    await page.getByRole("button", { name: "设置", exact: true }).click();
    await page.locator(".settings-nav-item").filter({ hasText: "模型设置" }).click();
    await page.locator(".settings-provider-row").first().waitFor();
    const rows = await page.locator(".settings-provider-row").evaluateAll((elements) => elements.map((row) => {
      const box = row.getBoundingClientRect();
      const actions = row.querySelector(".settings-provider-actions").getBoundingClientRect();
      const label = row.querySelector(".settings-provider-edit span");
      const labelStyle = getComputedStyle(label);
      return {
        name: row.querySelector("strong")?.textContent ?? "?",
        overflowRight: Math.round(actions.right - box.right),
        overflowLeft: Math.round(box.left - actions.left),
        labelLines: Math.round(label.getBoundingClientRect().height / (parseFloat(labelStyle.lineHeight) || parseFloat(labelStyle.fontSize) * 1.4)),
      };
    }));
    assert.ok(rows.length >= 4, `${width}px: expected built-in provider rows`);
    for (const row of rows) {
      assert.ok(row.overflowRight <= 1 && row.overflowLeft <= 1, `${width}px: actions of ${row.name} overflow the card: ${JSON.stringify(row)}`);
      assert.equal(row.labelLines, 1, `${width}px: 「编辑」 of ${row.name} must stay on one line: ${JSON.stringify(row)}`);
    }
    assert.deepEqual(pageErrors.filter((message) => !/transformCallback|__TAURI/.test(message)), []);
    await page.close();
  }
  console.log("PASS: provider row actions stay inside the card and 「编辑」 stays single-line from 820px to 1600px");
} finally {
  await browser.close();
}
