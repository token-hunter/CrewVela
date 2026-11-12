// Run against Vite: node tests/rightSidebarWidth.ui.mjs
import assert from "node:assert/strict";
import { chromium } from "playwright";

const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  await page.addInitScript(() => Object.defineProperty(navigator, "platform", { get: () => "Win32" })); // 该用例断言 Windows 形态（PowerShell/CMD、Windows 路径、重启 Codex），macOS 形态由 macosCore.ui.mjs 覆盖
  page.setDefaultTimeout(10000);
  const url = `${process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420"}/?ui-preview=1`;
  await page.goto(url);
  await page.getByRole("button", { name: "切换右侧栏", exact: true }).click();
  const sidebar = page.getByRole("complementary", { name: "右侧栏" });
  const width = () => sidebar.evaluate(element => element.getBoundingClientRect().width);
  const equalWidth = async (expected, label) => {
    await page.waitForTimeout(180);
    const actual = await width();
    assert.ok(Math.abs(actual - expected) < 2, `${label}: expected ${expected}px, got ${actual}px`);
  };
  await page.waitForTimeout(250);
  const initial = await width();
  const labels = ["开始"];
  for (const [menu, label] of [["工作区文件", "文件"], ["源代码控制", "Git"], ["多智能体", "多智能体"], ["浏览器", "浏览器"], ["新建 PowerShell 终端", "PowerShell"], ["新建 CMD 终端", "CMD"]]) {
    await sidebar.getByRole("button", { name: "新建工作面板标签", exact: true }).click();
    await sidebar.getByRole("menuitem", { name: menu, exact: true }).click();
    await equalWidth(initial, `open ${label}`);
    labels.push(label);
  }
  const separator = page.locator(".right-sidebar-resizer");
  const bounds = await separator.boundingBox();
  await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2);
  await page.mouse.down();
  await page.mouse.move(bounds.x + bounds.width / 2 - 120, bounds.y + bounds.height / 2, { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  const resized = await width();
  assert.ok(Math.abs(resized - initial - 120) < 2, "drag changes the shared width");
  for (const label of labels) {
    await sidebar.getByRole("tab", { name: label, exact: true }).click();
    await equalWidth(resized, `switch ${label}`);
  }
  await separator.focus();
  await page.keyboard.press("ArrowLeft");
  await equalWidth(resized + 24, "keyboard resize");
  await sidebar.getByRole("button", { name: "新建工作面板标签", exact: true }).click();
  await sidebar.getByRole("menuitem", { name: "浏览器", exact: true }).click();
  await equalWidth(resized + 24, "new browser instance after resize");
  await sidebar.getByRole("tab", { name: "浏览器", exact: true }).first().click();
  await equalWidth(resized + 24, "previous browser instance");
  await page.setViewportSize({ width: 700, height: 720 });
  await page.waitForTimeout(250);
  const available = await page.locator(".main-workspace").evaluate(element => element.clientWidth);
  await equalWidth(Math.min(resized + 24, available), "narrow window clamp");
  await sidebar.getByRole("tab", { name: "Git", exact: true }).click();
  await equalWidth(Math.min(resized + 24, available), "narrow window switch");
  await page.setViewportSize({ width: 1600, height: 1000 });
  await equalWidth(resized + 24, "window restore retains desired width");
  await sidebar.getByRole("button", { name: "最大化工作面板", exact: true }).click();
  await sidebar.getByRole("tab", { name: "Git", exact: true }).click();
  await sidebar.getByRole("button", { name: "恢复工作面板", exact: true }).click();
  await equalWidth(resized + 24, "restore after maximized tab switch");
  await sidebar.getByRole("button", { name: "收起工作面板", exact: true }).click();
  await page.getByRole("button", { name: "切换右侧栏", exact: true }).click();
  await equalWidth(resized + 24, "collapse/reopen");
  await page.reload();
  await page.getByRole("button", { name: "切换右侧栏", exact: true }).click();
  await equalWidth(resized + 24, "reload persistence");
  await page.locator(".right-sidebar-resizer").dblclick();
  await equalWidth(initial, "double-click resets the shared default");
  await sidebar.getByRole("tab", { name: "开始", exact: true }).click();
  await equalWidth(initial, "start after reset");
  await page.screenshot({ path: "output/playwright/right-sidebar-shared-width.png" });
  const legacyPage = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  await legacyPage.addInitScript(() => Object.defineProperty(navigator, "platform", { get: () => "Win32" })); // 该用例断言 Windows 形态（PowerShell/CMD、Windows 路径、重启 Codex），macOS 形态由 macosCore.ui.mjs 覆盖
  await legacyPage.addInitScript(() => {
    if (localStorage.getItem("width-test-seeded")) return;
    localStorage.setItem("width-test-seeded", "1");
    localStorage.setItem("crewvela-right-sidebar-tab", "files");
    localStorage.setItem("crewvela-file-sidebar-width", "760");
    localStorage.setItem("crewvela-git-sidebar-width", "430");
  });
  await legacyPage.goto(url);
  await legacyPage.getByRole("button", { name: "切换右侧栏", exact: true }).click();
  const legacySidebar = legacyPage.getByRole("complementary", { name: "右侧栏" });
  await legacyPage.waitForTimeout(250);
  assert.equal(await legacySidebar.evaluate(element => element.getBoundingClientRect().width), 760);
  await legacySidebar.getByRole("button", { name: "新建工作面板标签", exact: true }).click();
  await legacySidebar.getByRole("menuitem", { name: "源代码控制", exact: true }).click();
  await legacyPage.reload();
  await legacyPage.getByRole("button", { name: "切换右侧栏", exact: true }).click();
  await legacyPage.waitForTimeout(250);
  assert.equal(await legacySidebar.evaluate(element => element.getBoundingClientRect().width), 760,
    "legacy width survives switching tabs and reloading before any manual resize");
  console.log("PASS: all tool widths match; drag, keyboard, maximize/restore, collapse/reopen, reload and reset (browser preview)");
} finally {
  await browser.close();
}
