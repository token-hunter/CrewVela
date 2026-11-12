// Run against Vite: node tests/paneHeaderNarrow.ui.mjs
// 窄窗格里会话标题必须保持可读：Claude 桌宠先让位，状态文字收成色点，标题不被挤成 0 宽。
import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  for (const [width, height, narrow] of [[820, 540, true], [1280, 800, false]]) {
    const page = await browser.newPage({ viewport: { width, height } });
    const pageErrors = [];
    page.on("pageerror", (error) => pageErrors.push(error.message));
    await page.goto(`${base}/?ui-preview=1`);
    await page.locator(".pane-head .session-pet").first().waitFor({ state: "attached" });
    const panes = await page.locator(".pane-head").evaluateAll((heads) => heads.map((head) => {
      const title = head.querySelector(".pane-id strong");
      const pet = head.querySelector(".session-pet, .cli-session-pet");
      const claudePet = head.querySelector(".session-pet");
      const label = head.querySelector(".pane-status-label");
      const labelBox = label?.getBoundingClientRect();
      return {
        title: title.textContent,
        titleWidth: Math.round(title.getBoundingClientRect().width),
        titleNeeds: title.scrollWidth,
        claudePet: Boolean(claudePet),
        pet: pet ? getComputedStyle(pet).display : "none",
        statusLabelVisible: Boolean(labelBox && labelBox.width > 4 && labelBox.height > 4),
        statusTitle: head.querySelector(".pane-status")?.getAttribute("title") ?? "",
        headWidth: Math.round(head.getBoundingClientRect().width),
      };
    }));
    assert.ok(panes.length >= 4, `${width}px: expected the four preview panes`);
    for (const pane of panes) {
      const where = `${width}px: ${JSON.stringify(pane)}`;
      assert.ok(pane.titleWidth >= Math.min(pane.titleNeeds, 40) && pane.titleWidth >= pane.titleNeeds - 2, `title must stay fully readable (${where})`);
      assert.ok(pane.statusTitle, `status keeps a tooltip even when its text collapses (${where})`);
    }
    const claude = panes.filter((pane) => pane.claudePet);
    assert.ok(claude.length >= 2, `${width}px: expected Claude panes with a pet`);
    for (const pane of claude) {
      if (narrow) {
        assert.equal(pane.pet, "none", `Claude pet should yield in a narrow pane (${JSON.stringify(pane)})`);
        assert.equal(pane.statusLabelVisible, false, `status text collapses to the dot in a narrow pane (${JSON.stringify(pane)})`);
      } else {
        assert.notEqual(pane.pet, "none", `Claude pet remains in a wide pane (${JSON.stringify(pane)})`);
        assert.equal(pane.statusLabelVisible, true, `status text remains in a wide pane (${JSON.stringify(pane)})`);
      }
    }
    assert.deepEqual(pageErrors.filter((message) => !/transformCallback|__TAURI/.test(message)), []);
    await page.close();
  }
  console.log("PASS: pane titles stay readable in narrow panes; pets and status text remain in wide panes");
} finally {
  await browser.close();
}
