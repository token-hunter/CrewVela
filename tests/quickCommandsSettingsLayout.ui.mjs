// Run against Vite: node tests/quickCommandsSettingsLayout.ui.mjs
// 设置 > 快捷指令：搜索框是单层边框并与「重新检测」同一行，不重复页面标题已有的说明，
// 切换分类时窗口、标题栏和导航位置不变，短窗口的页签仍在首屏可见。
import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  for (const [width, height] of [[1500, 1050], [1280, 800], [820, 540], [390, 844]]) {
    const page = await browser.newPage({ viewport: { width, height } });
    const pageErrors = [];
    page.on("pageerror", (error) => pageErrors.push(error.message));
    await page.goto(`${base}/?ui-preview=1`);
    await page.getByRole("button", { name: "设置", exact: true }).click();
    const nav = page.getByRole("navigation", { name: "设置分类" });
    const frame = async () => {
      const boxes = await Promise.all([".settings-dialog", ".settings-window-header", ".settings-nav"].map(selector => page.locator(selector).boundingBox()));
      assert.ok(boxes.every(Boolean));
      return boxes;
    };
    await nav.getByRole("button", { name: "常规", exact: true }).click();
    const before = await frame();
    await nav.getByRole("button", { name: "快捷指令", exact: true }).click();
    await page.locator(".quick-center-embedded .quick-center-tabs").waitFor();
    assert.deepEqual(await frame(), before, `${width}x${height}: 快捷指令沿用相同的窗口、标题栏和导航尺寸`);

    const m = await page.evaluate(() => {
      const input = document.querySelector(".quick-center-search > input");
      const search = document.querySelector(".quick-center-search").getBoundingClientRect();
      const refresh = [...document.querySelectorAll(".quick-center-embedded button")].find((b) => b.textContent.trim() === "重新检测")?.getBoundingClientRect();
      const tabs = document.querySelector(".quick-center-tabs").getBoundingClientRect();
      const dialog = document.querySelector(".settings-dialog").getBoundingClientRect();
      const footer = document.querySelector(".quick-center-footer").getBoundingClientRect();
      return {
        inputBorder: getComputedStyle(input).borderTopWidth,
        inputPadding: getComputedStyle(input).paddingTop,
        sameRow: refresh ? Math.abs(search.top - refresh.top) <= 1 && Math.abs(search.height - refresh.height) <= 2 : false,
        duplicatedIntro: [...document.querySelectorAll(".quick-center-embedded p")].some((p) => p.textContent.includes("管理本机常用操作")),
        tabsVisible: tabs.bottom <= dialog.bottom - footer.height,
        numbers: { tabsBottom: Math.round(tabs.bottom), dialogBottom: Math.round(dialog.bottom), footer: Math.round(footer.height) },
      };
    });
    const where = `${width}x${height}: ${JSON.stringify(m)}`;
    assert.equal(m.inputBorder, "0px", `search input must not draw a second border inside the field (${where})`);
    assert.equal(m.inputPadding, "0px", where);
    assert.equal(m.sameRow, true, `「重新检测」 must share the search row (${where})`);
    assert.equal(m.duplicatedIntro, false, `page heading already explains the page (${where})`);
    assert.equal(m.tabsVisible, true, `category tabs must be visible without scrolling (${where})`);
    await nav.getByRole("button", { name: "外观", exact: true }).click();
    assert.deepEqual(await frame(), before, `${width}x${height}: 离开快捷指令时窗口不跳动`);
    assert.deepEqual(pageErrors.filter((message) => !/transformCallback|__TAURI/.test(message)), []);
    await page.close();
  }
  console.log("PASS: settings frame stays fixed when switching sections at four viewport sizes, with the search and tabs accessible");
} finally {
  await browser.close();
}
