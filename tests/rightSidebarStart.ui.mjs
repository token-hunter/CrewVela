import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";
import { checkSidebarStartLayout } from "./helpers/rightSidebarStartLayout.mjs";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const output = "output/playwright/right-sidebar-start";
mkdirSync(output, { recursive: true });
const browser = await chromium.launch({ channel: "chrome" });
try {
  const results = {};
  for (const [platform, expectedTerminal] of [["MacIntel", "zsh"], ["Win32", "powershell"]]) {
    const context = await browser.newContext({ viewport: { width: 1554, height: 950 } });
    await context.addInitScript(platform => Object.defineProperty(navigator, "platform", { get: () => platform }), platform);
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.goto(`${base}/tests/fixtures/right-sidebar-start-ui.html`);
    results[platform] = await page.evaluate(checkSidebarStartLayout);
    const terminal = page.locator('.right-sidebar-start-tools > button').nth(1);
    assert.ok((await terminal.getAttribute("title")).includes(expectedTerminal === "powershell" ? "PowerShell" : "zsh"));
    await page.locator('.right-sidebar-start-tools > button').nth(2).focus();
    await page.keyboard.press("Enter");
    await page.waitForFunction(() => document.querySelector('[data-testid="last-tool"]').textContent === "browser");
    await page.screenshot({ path: `${output}/${platform}-adaptive.png` });
    await page.goto(`${base}${process.env.CREWVELA_UI_ENTRY || "/"}?ui-preview=1&sessions=empty`);
    results[`${platform}-app`] = await page.evaluate(checkSidebarStartLayout, { fullApp: true });
    await page.getByRole("button", { name: "切换右侧栏", exact: true }).click();
    await page.waitForTimeout(250);
    await page.screenshot({ path: `${output}/${platform}-app.png` });
    await page.setViewportSize({ width: 1100, height: 500 });
    const cards = page.locator('.right-sidebar-start-tools > button');
    assert.ok((await cards.evaluateAll(buttons => buttons.map(button => button.getBoundingClientRect().height))).every(height => height >= 44 && height <= 48));
    await cards.last().scrollIntoViewIfNeeded();
    assert.ok(await cards.last().isVisible());
    assert.deepEqual(errors, []);
    await context.close();
  }
  writeFileSync(`${output}/results.json`, JSON.stringify(results, null, 2));
  console.log("Sidebar start layout, repeat toggles, callbacks and platform branches passed.");
} finally { await browser.close(); }
