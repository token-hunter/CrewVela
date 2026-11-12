// Run against Vite: node tests/modalOverResizers.ui.mjs
// 左右侧栏拖拽条不能盖在弹窗或菜单之上，否则弹窗里与拖拽条重叠的那一列点击会被截走。
import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1600, height: 950 } });
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.goto(`${base}/?ui-preview=1`);
  await page.waitForSelector(".unified-sidebar-settings-button");

  // 在 selector 对应元素与 container 的重叠区域沿拖拽条中线取样，返回被拖拽条截获的点数。
  const interceptedBy = (resizerSelector, containerSelector) => page.evaluate(({ resizerSelector, containerSelector }) => {
    const resizer = document.querySelector(resizerSelector);
    const container = document.querySelector(containerSelector);
    if (!resizer || !container) return { missing: true, hits: 0, samples: 0 };
    const strip = resizer.getBoundingClientRect();
    const box = container.getBoundingClientRect();
    const x = strip.x + strip.width / 2;
    if (x <= box.left + 2 || x >= box.right - 2) return { outside: true, hits: 0, samples: 0 };
    let hits = 0;
    let samples = 0;
    for (let y = Math.max(strip.top, box.top) + 8; y < Math.min(strip.bottom, box.bottom) - 8; y += 24) {
      samples += 1;
      if (document.elementFromPoint(x, y)?.closest(resizerSelector)) hits += 1;
    }
    return { hits, samples };
  }, { resizerSelector, containerSelector });

  // 1) 设置弹窗：左右拖拽条都不能压在弹窗上。
  await page.getByRole("button", { name: "切换右侧栏", exact: true }).click();
  await page.locator(".right-sidebar-resizer").waitFor();
  await page.locator(".unified-sidebar-settings-button").click();
  await page.locator(".settings-dialog").waitFor();
  for (const resizer of [".left-sidebar-resizer", ".right-sidebar-resizer"]) {
    const result = await interceptedBy(resizer, ".settings-dialog");
    assert.ok(!result.missing && !result.outside && result.samples > 5, `${resizer} must overlap the settings dialog in this viewport: ${JSON.stringify(result)}`);
    assert.equal(result.hits, 0, `${resizer} must not intercept pointer events above the settings dialog`);
  }
  // 设置导航项正中恰好落在左拖拽条上：必须能正常点击切换。
  await page.locator(".settings-nav-item").filter({ hasText: "协作中心" }).click({ timeout: 5000 });
  await page.getByRole("heading", { name: "协作中心", exact: true }).waitFor();
  await page.getByRole("button", { name: "关闭设置", exact: true }).click();
  await page.locator(".settings-dialog").waitFor({ state: "detached" });

  // 2) 新建会话菜单越过左栏边界时，也应盖在拖拽条之上。
  await page.getByRole("button", { name: "新建会话", exact: true }).first().click();
  await page.locator(".rail-new-menu").waitFor();
  const menu = await interceptedBy(".left-sidebar-resizer", ".rail-new-menu");
  assert.ok(menu.outside || menu.hits === 0, `left resizer must not intercept the new-session menu: ${JSON.stringify(menu)}`);

  assert.deepEqual(pageErrors.filter((message) => !/transformCallback|__TAURI/.test(message)), []);
  console.log("PASS: sidebar resizers stay below modal dialogs and menus; settings navigation is clickable across the resizer strip");
} finally {
  await browser.close();
}
