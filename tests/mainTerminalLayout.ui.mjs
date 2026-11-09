// Run against Vite: node tests/mainTerminalLayout.ui.mjs
import assert from "node:assert/strict";
import { chromium } from "playwright";

const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  await page.goto(`${process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420"}/?ui-preview=1`);
  const host = page.locator(".main-terminal-host:not([hidden])");
  await host.locator(".pane").first().waitFor();

  const checkLayout = async (count, cols) => {
    await page.waitForFunction(({ count }) => {
      const host = document.querySelector(".main-terminal-host:not([hidden])");
      return host && [...host.querySelectorAll(".pane")].filter(p => getComputedStyle(p).display !== "none").length === count;
    }, { count });
    const geometry = await host.evaluate(element => {
      const rect = e => {
        const r = e.getBoundingClientRect();
        return { x: r.x, y: r.y, width: r.width, height: r.height, bottom: r.bottom };
      };
      const grid = element.querySelector(".grid");
      return { host: rect(element), grid: rect(grid), panes: [...grid.querySelectorAll(".pane")]
        .filter(p => getComputedStyle(p).display !== "none").map(rect) };
    });
    assert.ok(Math.abs(geometry.grid.height - geometry.host.height) < 2, "grid must fill its host height");
    assert.ok(geometry.grid.height > 300, "grid must use the workspace, not collapse to content height");
    const gap = await host.locator(".grid").evaluate(e => parseFloat(getComputedStyle(e).columnGap));
    const rows = Math.ceil(count / cols);
    for (const pane of geometry.panes) {
      assert.ok(Math.abs(pane.width - (geometry.grid.width - gap * (cols - 1)) / cols) < 2, "equal column widths");
      assert.ok(Math.abs(pane.height - (geometry.grid.height - gap * (rows - 1)) / rows) < 2, "equal row heights");
    }
    assert.ok(Math.abs(Math.max(...geometry.panes.map(p => p.bottom)) - geometry.host.bottom) < 2, "last row must reach the bottom");
  };

  // Actual React preview: maximize and restore must keep the full height.
  await checkLayout(4, 2);
  await host.getByRole("button", { name: "最大化会话", exact: true }).first().click();
  await checkLayout(1, 1);
  await host.getByRole("button", { name: "还原会话", exact: true }).click();
  await checkLayout(4, 2);

  // Layout fixtures reuse the live application's ancestor chain and loaded CSS.
  // Counts simulate adding/removing panes without invoking native PTY commands.
  await host.evaluate(element => {
    window.layoutPaneTemplate = element.querySelector(".pane").cloneNode(true);
    const banner = document.createElement("div");
    banner.textContent = "接力待处理：布局回归提示条";
    banner.style.cssText = "flex:0 0 40px";
    element.before(banner);
    window.layoutBanner = banner;
  });
  for (const size of [{ width: 1440, height: 1000 }, { width: 1100, height: 720 }]) {
    await page.setViewportSize(size);
    for (const count of [1, 2, 5, 2, 1]) {
      const cols = Math.ceil(Math.sqrt(count));
      await host.evaluate((element, { count, cols }) => {
        const grid = element.querySelector(".grid");
        grid.style.gridTemplateColumns = `repeat(${cols}, 1fr)`;
        grid.replaceChildren(...Array.from({ length: count }, () => window.layoutPaneTemplate.cloneNode(true)));
      }, { count, cols });
      await checkLayout(count, cols);
      assert.ok(await host.evaluate(element => Math.abs(element.getBoundingClientRect().bottom -
        (element.parentElement.getBoundingClientRect().bottom - parseFloat(getComputedStyle(element.parentElement).paddingBottom))) < 2),
      "banner must not push terminals below the workspace");
    }
  }

  // Prove this regression rejects the previous wrapper styling.
  await page.addStyleTag({ content: ".main-terminal-host { display:block; flex:0 1 auto; height:100%; }" });
  await assert.rejects(() => checkLayout(1, 1), /grid must fill its host height/);
  console.log("PASS: actual preview maximize/restore; 1/2/5-pane layouts, count changes, resize, banner; old CSS rejected");
} finally {
  await browser.close();
}
