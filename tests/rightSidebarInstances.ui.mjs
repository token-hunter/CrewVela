// Run against Vite: node tests/rightSidebarInstances.ui.mjs
import assert from "node:assert/strict";
import { chromium } from "playwright";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  await page.goto(`${process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420"}/?ui-preview=1`);
  // Open the default work panel through its normal shortcut.
  await page.getByRole("button", { name: "切换右侧栏", exact: true }).click();
  const sidebar = page.getByRole("complementary", { name: "右侧栏" });
  const create = async (menuName) => {
    await sidebar.getByRole("button", { name: "新建工作面板标签", exact: true }).click();
    await sidebar.getByRole("menuitem", { name: menuName, exact: true }).click();
  };
  for (const [menu, label] of [["工作区文件", "文件"], ["源代码控制", "Git"], ["多智能体", "多智能体"], ["浏览器", "浏览器"]]) {
    await create(menu);
    await create(menu);
    const tabs = sidebar.getByRole("tab", { name: label, exact: true });
    assert.equal(await tabs.count(), 2, `${label}: add must create two tabs`);
    assert.equal(await tabs.nth(1).getAttribute("aria-selected"), "true");
    await tabs.nth(0).click();
    assert.equal(await tabs.nth(0).getAttribute("aria-selected"), "true");
    assert.equal(await tabs.nth(1).getAttribute("aria-selected"), "false");
    if (label === "多智能体") {
      await sidebar.getByRole("button", { name: "收起工作面板", exact: true }).click();
      await page.getByRole("button", { name: "切换右侧栏", exact: true }).click();
      assert.equal(await tabs.nth(0).getAttribute("aria-selected"), "true", "collapse/reopen retains the instance");
      const mode = sidebar.locator(".right-sidebar-panel.active").getByRole("button", { name: "协作自动确认模式", exact: true });
      await mode.click();
      assert.equal(await mode.getAttribute("aria-pressed"), "true");
      await tabs.nth(1).click();
      const otherMode = sidebar.locator(".right-sidebar-panel.active").getByRole("button", { name: "协作自动确认模式", exact: true });
      assert.equal(await otherMode.getAttribute("aria-pressed"), "true", "Auto is shared for the same project");
      await otherMode.click();
      await tabs.nth(0).click();
      assert.equal(await mode.getAttribute("aria-pressed"), "false", "turning Auto off synchronizes hidden instances");
    }
    if (label === "浏览器") {
      const address = sidebar.locator(".right-sidebar-panel.active").getByRole("textbox", { name: "地址栏" });
      await address.fill("独立的地址草稿");
      // Switch while retaining focus; the second component begins blank.
      await tabs.nth(1).click();
      assert.equal(await sidebar.locator(".right-sidebar-panel.active").getByRole("textbox", { name: "地址栏" }).inputValue(), "");
      await tabs.nth(0).click();
      // Submit local URL state even though native IPC is unavailable in preview.
      await sidebar.locator(".right-sidebar-panel.active").getByRole("textbox", { name: "地址栏" }).fill("https://example.com/first");
      await sidebar.locator(".right-sidebar-panel.active").getByRole("textbox", { name: "地址栏" }).press("Enter");
      await tabs.nth(1).click();
      assert.equal(await sidebar.locator(".right-sidebar-panel.active").getByRole("textbox", { name: "地址栏" }).inputValue(), "");
      await tabs.nth(0).click();
      assert.equal(await sidebar.locator(".right-sidebar-panel.active").getByRole("textbox", { name: "地址栏" }).inputValue(), "https://example.com/first");
    }
    await sidebar.getByRole("button", { name: `关闭${label}标签`, exact: true }).nth(0).click();
    assert.equal(await tabs.count(), 1, `${label}: closing one preserves the other`);
    await tabs.nth(0).click();
    assert.equal(await tabs.nth(0).getAttribute("aria-selected"), "true");
  }
  console.log("PASS: repeat create, independent selection/browser URL state, close sibling retention for files/Git/multi-agent/browser (preview; native WebView not exercised)");
} finally { await browser.close(); }
