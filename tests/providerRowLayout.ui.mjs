// Run against Vite: node tests/providerRowLayout.ui.mjs
// 模型设置：逐行展开后检查右对齐、操作边界与额度去重。
import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  for (const width of [375, 650, 820, 900, 1001, 1024, 1060, 1090, 1100, 1280, 1501]) {
    const page = await browser.newPage({ viewport: { width, height: 800 } });
    const pageErrors = [];
    page.on("pageerror", (error) => pageErrors.push(error.message));
    await page.goto(`${base}/tests/fixtures/model-settings-ui.html`);
    const rows = page.locator(".settings-provider-row");
    await rows.first().waitFor();
    assert.equal(await rows.count(), 6, `${width}px: expected all providers including hidden`);
    await rows.first().locator(".settings-provider-drag-handle").dragTo(rows.nth(1).locator(".settings-provider-drag-handle"));
    assert.equal(await rows.first().locator("strong").first().innerText(), "DeepSeek", "drag handle must reorder the list");
    for (const row of await rows.all()) {
      await row.locator(".model-provider-toggle").click();
      assert.equal(await page.locator(".model-provider-detail").count(), 1);
      const layout = await row.evaluate((element) => {
        const box = element.getBoundingClientRect();
        const actions = element.querySelector(".settings-provider-actions").getBoundingClientRect();
        const label = element.querySelector(".settings-provider-edit span");
        const labelStyle = getComputedStyle(label);
        return {
          overflowRight: actions.right - box.right,
          overflowLeft: box.left - actions.left,
          labelLines: Math.round(label.getBoundingClientRect().height / parseFloat(labelStyle.lineHeight)),
          fieldOverflow: [...element.querySelectorAll("dd")].some(field => field.scrollWidth > field.clientWidth + 1),
          valueEdges: [...element.querySelectorAll("dd")].map(field => Math.round(field.getBoundingClientRect().right)),
          detailQuotaCount: element.querySelector(".model-provider-detail").querySelectorAll(".provider-quota-values").length,
        };
      });
      assert.ok(layout.overflowRight <= 1 && layout.overflowLeft <= 1, `${width}px: footer overflow`);
      assert.equal(layout.labelLines, 1, `${width}px: edit label wraps`);
      assert.equal(layout.fieldOverflow, false, `${width}px: long endpoint overflows`);
      assert.equal(new Set(layout.valueEdges).size, 1, `${width}px: values must share a right edge`);
      assert.equal(layout.detailQuotaCount, 0, "quota is only rendered in the summary");
    }
    assert.equal(await page.locator(".settings-content").evaluate(e => e.scrollWidth <= e.clientWidth + 1), true);
    assert.deepEqual(pageErrors.filter((message) => !/transformCallback|__TAURI/.test(message)), []);
    await page.close();
  }
  console.log("PASS: all provider details are aligned, quota is not duplicated, and controls fit from 375px to 1501px");
} finally {
  await browser.close();
}
