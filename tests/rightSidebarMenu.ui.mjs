// Run against Vite: CREWVELA_UI_URL=http://127.0.0.1:1420 node tests/rightSidebarMenu.ui.mjs
import assert from "node:assert/strict";
import { chromium } from "playwright";

const baseUrl = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const appUrl = new URL("/", baseUrl);
appUrl.searchParams.set("ui-preview", "1");
appUrl.searchParams.set("sessions", "empty");

const expectedMenuItems = [
  "工作区文件",
  "新建 PowerShell 终端",
  "新建 CMD 终端",
  "浏览器",
  "源代码控制",
  "多智能体",
];

const browser = await chromium.launch({ channel: "chrome", headless: true });

try {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 1000 },
  });
  const page = await context.newPage();
  await page.addInitScript(() => Object.defineProperty(navigator, "platform", { get: () => "Win32" })); // 该用例断言 Windows 形态（PowerShell/CMD、Windows 路径、重启 Codex），macOS 形态由 macosCore.ui.mjs 覆盖
  page.setDefaultTimeout(10000);
  await page.goto(appUrl.href);

  await page.getByRole("button", { name: "切换右侧栏" }).click();
  await page.locator(".right-sidebar-start-tools > button")
    .filter({ hasText: "多智能体" })
    .click();
  await page.waitForFunction(() =>
    document.querySelector(".right-sidebar-resource-tab.selected [role=tab]")
      ?.textContent?.trim() === "多智能体",
  );

  const openMenu = async () => {
    const plus = page.getByRole("button", { name: "新建工作面板标签" });
    if ((await plus.getAttribute("aria-expanded")) !== "true") await plus.click();
    await page.locator(".right-sidebar-new-menu").waitFor({ state: "visible" });
  };

  const assertMenuHitTargets = async (surface) => {
    const menuRows = await page.evaluate(() => {
      const items = Array.from(
        document.querySelectorAll(".right-sidebar-new-menu [role=menuitem]"),
      );
      return items.map((item) => {
        const rect = item.getBoundingClientRect();
        const x = rect.left + rect.width / 2;
        const y = rect.top + rect.height / 2;
        const hit = document.elementFromPoint(x, y);
        return {
          label: item.textContent?.trim() || "",
          disabled: item.disabled,
          x,
          y,
          hit: hit
            ? `${hit.tagName.toLowerCase()}.${typeof hit.className === "string" ? hit.className : ""}`
            : "null",
          hitInsideItem: !!hit && item.contains(hit),
        };
      });
    });

    assert.deepEqual(
      menuRows.map((row) => row.label),
      expectedMenuItems,
      `${surface}: the new-tab menu must show all six tools`,
    );
    for (const row of menuRows) {
      assert.ok(
        row.hitInsideItem,
        `${surface}: elementFromPoint missed menu item "${row.label}" at (${Math.round(row.x)}, ${Math.round(row.y)}); hit ${row.hit}`,
      );
    }
  };

  const clickMenuItem = async (label, activeTabLabel) => {
    const item = page.locator(".right-sidebar-new-menu [role=menuitem]")
      .filter({ hasText: label });
    assert.equal(await item.count(), 1, `expected one menu item: ${label}`);
    assert.equal(await item.isEnabled(), true, `menu item should be enabled: ${label}`);
    const point = await item.evaluate((element) => {
      const rect = element.getBoundingClientRect();
      return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
    });
    await page.mouse.click(point.x, point.y);
    await page.waitForFunction(
      (expected) =>
        !document.querySelector(".right-sidebar-new-menu") &&
        document.querySelector(".right-sidebar-resource-tab.selected [role=tab]")
          ?.textContent?.trim() === expected,
      activeTabLabel,
    );
  };

  await openMenu();
  await assertMenuHitTargets("multi-agent page");
  await clickMenuItem("工作区文件", "文件");
  console.log("PASS: all six menu items receive pointer hits over multi-agent content");

  await openMenu();
  await assertMenuHitTargets("files page");
  await clickMenuItem("浏览器", "浏览器");
  console.log("PASS: menu remains clickable from the files page");

  await openMenu();
  await assertMenuHitTargets("browser page");
  await clickMenuItem("源代码控制", "Git");
  console.log("PASS: menu remains clickable from the browser page and opens Git");

  const selectedTab = () => page.locator(
    ".right-sidebar-resource-tab.selected [role=tab]",
  ).innerText();
  await openMenu();
  await page.mouse.click(1400, 800);
  await page.locator(".right-sidebar-new-menu").waitFor({ state: "hidden" });
  assert.equal(await selectedTab(), "Git", "outside click must not switch tabs");
  console.log("PASS: clicking outside closes the menu without changing tabs");

  await openMenu();
  await page.keyboard.press("Escape");
  await page.locator(".right-sidebar-new-menu").waitFor({ state: "hidden" });
  assert.equal(
    await page.evaluate(() => document.activeElement?.getAttribute("aria-label")),
    "新建工作面板标签",
    "Escape must return focus to the plus button",
  );
  console.log("PASS: Escape closes the menu and restores focus");

  const resizer = page.getByRole("separator", { name: "调整源代码控制侧栏宽度" });
  const widthBefore = Number(await resizer.getAttribute("aria-valuenow"));
  await resizer.focus();
  await page.keyboard.press("ArrowLeft");
  await page.waitForFunction(
    (before) => Number(document.querySelector('[role="separator"][aria-label="调整源代码控制侧栏宽度"]')?.getAttribute("aria-valuenow")) !== before,
    widthBefore,
  );
  console.log("PASS: keyboard resize still changes the sidebar width");

  await page.setViewportSize({ width: 900, height: 800 });
  await openMenu();
  await assertMenuHitTargets("narrow viewport");
  console.log("PASS: menu items remain reachable at a narrow viewport");

  await page.keyboard.press("Escape");
  await page.locator(".right-sidebar-new-menu").waitFor({ state: "hidden" });
  await page.getByRole("button", { name: "最大化工作面板" }).click();
  await page.locator(".unified-right-sidebar.is-maximized").waitFor();
  await openMenu();
  await assertMenuHitTargets("maximized sidebar");
  await page.keyboard.press("Escape");
  await page.locator(".right-sidebar-new-menu").waitFor({ state: "hidden" });
  assert.equal(
    await page.getByRole("separator", { name: "调整源代码控制侧栏宽度" }).count(),
    0,
    "maximized sidebar must hide its resize handle",
  );
  await page.getByRole("button", { name: "恢复工作面板" }).click();
  await page.locator(".unified-right-sidebar:not(.is-maximized)").waitFor();
  console.log("PASS: menu and maximize/restore work at a narrow viewport");

  await context.close();
} finally {
  await browser.close();
}
