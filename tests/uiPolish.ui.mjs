// Browser preview only: no native runtime or real generation is started.
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const output = "output/playwright/ui-polish";
mkdirSync(output, { recursive: true });
const browser = await chromium.launch({ channel: "chrome", headless: true });
const cases = [
  { name: "1440-dark", width: 1440, height: 900, scale: 1, theme: "dark" },
  { name: "1114-dark", width: 1114, height: 900, scale: 1, theme: "dark" },
  { name: "1280-dark", width: 1280, height: 720, scale: 1, theme: "dark" },
  { name: "1920-dark", width: 1920, height: 1080, scale: 1, theme: "dark" },
  { name: "125pct", width: 1536, height: 864, scale: 1.25, theme: "dark" },
  { name: "150pct", width: 1280, height: 720, scale: 1.5, theme: "dark" },
  { name: "900-light", width: 900, height: 720, scale: 1, theme: "light" },
  { name: "820-dark", width: 820, height: 540, scale: 1, theme: "dark" },
];
try {
  for (const size of cases) {
    const context = await browser.newContext({ viewport: { width: size.width, height: size.height }, deviceScaleFactor: size.scale });
    const page = await context.newPage();
    page.setDefaultTimeout(10000);
    await page.addInitScript(theme => {
      localStorage.setItem("crewvela-appearance", JSON.stringify({ theme, accent: "blue", background: "solid" }));
    }, size.theme);
    await page.goto(`${base}/?ui-preview=1`);
    const direct = page.getByRole("button", { name: "新建会话", exact: true });
    await direct.waitFor();
    const session = page.locator(".session-item").filter({ hasText: "需求拆解" });
    const canSelectSession = await session.isVisible();
    assert.equal(canSelectSession, true, `session navigation remains available: ${size.name}`);
    if (canSelectSession) {
      await session.click({ position: { x: 35, y: 12 } });
      await page.locator(".session-item.active").waitFor();
    }
    const metaSize = await page.locator(".unified-sidebar .session-meta").first().evaluate(el => parseFloat(getComputedStyle(el).fontSize));
    assert.ok(metaSize >= 12, `session metadata readable: ${size.name}`);
    await page.screenshot({ animations: "disabled", path: `${output}/${size.name}-main.png` });

    await direct.click();
    const menu = page.getByRole("menu", { name: "新建 Runtime 会话" });
    await menu.waitFor();
    const directItems = await menu.getByRole("menuitem").allTextContents();
    for (const runtime of ["pi", "opencode", "pi_chat", "opencode_chat"]) {
      const filter = await menu.locator(`img[data-runtime="${runtime}"]`).evaluate(el => getComputedStyle(el).filter);
      assert.equal(filter, size.theme === "light" ? "none" : "invert(1)", `${size.name}: ${runtime} logo contrast follows the theme`);
    }
    assert.doesNotMatch(directItems.join(" "), /Hermes/);
    if (size.name === "1440-dark") await page.screenshot({ path: `${output}/${size.name}-new-session-menu.png` });
    const bounds = await menu.boundingBox();
    assert.ok(bounds.x >= 0 && bounds.y >= 0 && bounds.x + bounds.width <= size.width && bounds.y + bounds.height <= size.height);
    await page.keyboard.press("Escape");
    assert.equal(await menu.count(), 0);
    assert.equal(await direct.evaluate(el => el === document.activeElement), true);
    if (canSelectSession) assert.match(await page.locator(".session-item.active").innerText(), /需求拆解/, "Escape closes only the menu, preserving selected session");
    await direct.click();
    await page.locator(".menu-backdrop").click({ position: { x: size.width - 5, y: size.height - 5 } });
    assert.equal(await menu.count(), 0);

    // 更多工具 no longer duplicates the sidebar's 新建会话 entry: only the bulk
    // actions remain, and nothing in it opens the runtime list.
    const moreTools = page.getByRole("button", { name: "更多工具", exact: true });
    const toolsMenu = page.getByRole("menu", { name: "更多工具", exact: true });
    await moreTools.click();
    await toolsMenu.waitFor();
    assert.deepEqual(await toolsMenu.getByRole("menuitem").allTextContents(), ["停止全部", "删除全部"]);
    await toolsMenu.getByRole("menuitem").first().hover();
    assert.equal(await menu.count(), 0);
    await page.locator(".menu-backdrop").click({ position: { x: size.width - 5, y: size.height - 5 } });
    assert.equal(await toolsMenu.count(), 0);

    // Opening 更多工具 from the keyboard while the runtime list is open must not
    // bring the list back as a nested submenu.
    await direct.click();
    await menu.waitFor();
    await moreTools.focus();
    await page.keyboard.press("Enter");
    await toolsMenu.waitFor();
    assert.equal(await menu.count(), 0);
    await page.locator(".menu-backdrop").click({ position: { x: size.width - 5, y: size.height - 5 } });
    assert.equal(await toolsMenu.count(), 0);

    await page.getByRole("button", { name: "设置", exact: true }).click();
    await page.getByRole("heading", { name: "模型设置", exact: true }).waitFor();
    await page.screenshot({ animations: "disabled", path: `${output}/${size.name}-models.png` });
    const settingsNav = page.locator(".settings-nav");
    assert.doesNotMatch(await settingsNav.innerText(), /语音|赛博女友|Benchmark|工作台|工作流画布/);
    await settingsNav.getByRole("button", { name: "陪伴", exact: true }).click();
    await page.locator(".pet-settings-panel").waitFor();
    assert.equal(await page.getByText("赛博女友", { exact: true }).count(), 0);
    await page.screenshot({ animations: "disabled", path: `${output}/${size.name}-pets.png` });
    await page.getByRole("button", { name: "关闭设置", exact: true }).click();

    // Direct launch reuses the existing preview runtime path.
    await direct.click();
    await menu.getByRole("menuitem", { name: /DeepSeek Harness/ }).click();
    await page.locator(".harness-chat").waitFor();
    await page.screenshot({ animations: "disabled", path: `${output}/${size.name}-chat.png` });
    assert.equal(await menu.count(), 0);
    console.log(`PASS ${size.name}: readability, more-tools de-dup, menu Escape/outside click, removed-feature navigation, retained pet settings, native-chat preview entry`);
    await context.close();
  }
} finally { await browser.close(); }
