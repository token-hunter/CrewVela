import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { chromium } from "playwright";

const baseUrl = process.env.CREWVELA_UI_URL || "http://localhost:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
mkdirSync(resolve("output/playwright"), { recursive: true });

try {
  const page = await browser.newPage({ viewport: { width: 1180, height: 820 } });
  page.setDefaultTimeout(10000);
  await page.goto(new URL("/tests/fixtures/session-list-actions-ui.html", baseUrl).href);
  const rows = page.locator(".session-item");
  await page.waitForFunction(() => document.querySelectorAll(".session-item").length === 5);

  const mainRow = rows.nth(0);
  const trigger = mainRow.locator(".session-actions-trigger");
  const mainTitle = mainRow.locator(".session-title");
  const titleLayout = await mainRow.evaluate((row) => {
    const title = row.querySelector(".session-title").getBoundingClientRect();
    const button = row.querySelector(".session-actions-trigger").getBoundingClientRect();
    return {
      titleFits: title.right <= button.left,
      titleIsEllipsized: row.querySelector(".session-title").scrollWidth > row.querySelector(".session-title").clientWidth,
      metaVisible: row.querySelector(".session-meta").getBoundingClientRect().height > 0,
    };
  });
  assert.ok(titleLayout.titleFits, "the reserved action button space must not overlap the title");
  assert.ok(titleLayout.titleIsEllipsized, "the long title should ellipsize in a narrow sidebar");
  assert.ok(titleLayout.metaVisible, "session status and metadata should remain visible");
  assert.equal(await trigger.isVisible(), true, "the more button should have a stable visible position");
  await page.screenshot({ path: resolve("output/playwright/session-list-actions-closed.png"), fullPage: true });

  const expectedActions = [
    "将当前工作交接给其他 Agent",
    "回看接力历史",
    "接力详情与恢复原会话",
    "查看历史记录",
    "收藏",
    "重命名",
    "停止",
    "归档会话",
    "从列表删除",
  ];
  const menu = page.getByRole("menu", { name: /操作$/ });
  const openMenu = async () => {
    await trigger.click();
    await menu.waitFor({ state: "visible" });
  };
  const menuLabels = async () => menu.getByRole("menuitem").allTextContents().then(items => items.map(item => item.trim()));

  await openMenu();
  assert.deepEqual(await menuLabels(), expectedActions, "all eligible session actions should remain available in the menu");
  const menuGeometry = await menu.evaluate(element => {
    const rect = element.getBoundingClientRect();
    const list = document.querySelector(".session-list");
    return {
      portaledToBody: element.parentElement === document.body,
      outsideScrollList: !list.contains(element),
      insideViewport: rect.left >= 0 && rect.top >= 0 && rect.right <= innerWidth && rect.bottom <= innerHeight,
      outsideSidebarRight: rect.left >= document.querySelector(".fixture-sidebar").getBoundingClientRect().right,
      alignedWithRow: Math.abs(rect.top - document.querySelector(".session-item").getBoundingClientRect().top) <= 1,
      deleteHasDivider: getComputedStyle(element.querySelector(".has-divider-before")).borderTopWidth !== "0px",
    };
  });
  assert.ok(menuGeometry.portaledToBody && menuGeometry.outsideScrollList, "the menu must escape sidebar scroll clipping");
  assert.ok(menuGeometry.insideViewport, "the menu must stay inside the viewport");
  assert.ok(menuGeometry.outsideSidebarRight, "the menu should open to the right outside the sidebar");
  assert.ok(menuGeometry.alignedWithRow, "the menu top should align with the current row when there is room");
  assert.ok(menuGeometry.deleteHasDivider, "delete must be visually separated from other actions");
  assert.equal(await page.locator("#selection-status").innerText(), "selected:session-shell", "opening the menu must not select the row");

  const runAction = async (label, expectedLog) => {
    if (!(await menu.isVisible())) await openMenu();
    await menu.getByRole("menuitem", { name: label, exact: true }).click();
    await page.waitForFunction(expected => document.querySelector("#action-log").textContent.trim().split(" | ").at(-1) === expected, expectedLog);
    assert.equal(await page.locator("#selection-status").innerText(), "selected:session-shell", `${label} must not select the row`);
  };
  await runAction("将当前工作交接给其他 Agent", "handoff:session-main");
  await runAction("回看接力历史", "transcript:session-main");
  await runAction("接力详情与恢复原会话", "relay:session-main");
  await runAction("查看历史记录", "transcript:session-main");
  await runAction("重命名", "rename:session-main");
  await runAction("停止", "stop:session-main");
  await runAction("归档会话", "archive:session-main");
  await runAction("从列表删除", "delete:session-main");

  await openMenu();
  await menu.getByRole("menuitem", { name: "收藏", exact: true }).click();
  await openMenu();
  assert.equal(await menu.getByRole("menuitem", { name: "取消收藏", exact: true }).count(), 1, "favorite toggle must persist and update its label");
  await menu.getByRole("menuitem", { name: "取消收藏", exact: true }).click();

  await trigger.focus();
  await page.keyboard.press("ArrowDown");
  await menu.waitFor({ state: "visible" });
  await page.waitForFunction(expected => document.activeElement?.textContent?.trim() === expected, expectedActions[0]);
  assert.equal(await page.evaluate(() => document.activeElement?.textContent?.trim()), expectedActions[0], "ArrowDown on the trigger should focus the first menu item");
  await page.keyboard.press("End");
  assert.equal(await page.evaluate(() => document.activeElement?.textContent?.trim()), "从列表删除", "End should focus the last menu item");
  await page.keyboard.press("ArrowUp");
  assert.equal(await page.evaluate(() => document.activeElement?.textContent?.trim()), "归档会话", "ArrowUp should move to the previous menu item");
  await page.keyboard.press("Home");
  assert.equal(await page.evaluate(() => document.activeElement?.textContent?.trim()), expectedActions[0], "Home should focus the first menu item");
  await page.keyboard.press("Escape");
  await menu.waitFor({ state: "hidden" });
  await page.waitForFunction(() => document.activeElement?.classList.contains("session-actions-trigger"));
  assert.equal(await page.evaluate(() => document.activeElement?.classList.contains("session-actions-trigger")), true, "Escape should restore focus to the trigger");

  const secondTrigger = rows.nth(1).locator(".session-actions-trigger");
  await trigger.focus();
  await page.keyboard.press("ArrowDown");
  await menu.waitFor({ state: "visible" });
  await page.waitForFunction(expected => document.activeElement?.textContent?.trim() === expected, expectedActions[0]);
  await page.keyboard.press("Tab");
  await menu.waitFor({ state: "hidden" });
  assert.equal(await secondTrigger.evaluate(element => element === document.activeElement), true, "Tab should continue at the next control after the trigger");

  await trigger.focus();
  await page.keyboard.press("ArrowDown");
  await menu.waitFor({ state: "visible" });
  await page.waitForFunction(expected => document.activeElement?.textContent?.trim() === expected, expectedActions[0]);
  await page.keyboard.press("Shift+Tab");
  await menu.waitFor({ state: "hidden" });
  assert.equal(
    await page.locator(".session-filter-trigger").evaluate(element => element === document.activeElement),
    true,
    "Shift+Tab should continue at the previous control before the trigger",
  );

  await trigger.click();
  await menu.waitFor({ state: "visible" });
  await page.locator("#outside-control").click();
  await menu.waitFor({ state: "hidden" });
  assert.equal(await page.locator("#selection-status").innerText(), "selected:session-shell", "outside click should close the menu without selecting a session");

  const shellMenu = rows.nth(1).locator(".session-actions-trigger");
  await shellMenu.click();
  await menu.waitFor({ state: "visible" });
  const shellLabels = await menuLabels();
  assert.deepEqual(shellLabels, ["收藏", "重命名", "停止", "归档会话", "从列表删除"], "optional handoff and transcript items must keep their existing visibility conditions");
  await page.keyboard.press("Escape");

  const sessionList = page.locator(".session-list");
  await sessionList.evaluate(element => { element.scrollTop = element.scrollHeight; });
  const lastTrigger = rows.nth(4).locator(".session-actions-trigger");
  await lastTrigger.click();
  await menu.waitFor({ state: "visible" });
  const scrolledMenuFits = await menu.evaluate(element => {
    const rect = element.getBoundingClientRect();
    return rect.top >= 0 && rect.bottom <= innerHeight;
  });
  assert.ok(scrolledMenuFits, "the portaled menu should stay in the viewport when opened near the bottom of the scroll list");

  await trigger.click();
  await menu.waitFor({ state: "visible" });
  await page.screenshot({ path: resolve("output/playwright/session-list-actions.png"), fullPage: true });
  await page.setViewportSize({ width: 420, height: 400 });
  await page.waitForFunction(() => {
    const rect = document.querySelector(".session-action-menu").getBoundingClientRect();
    return rect.left >= 8 && rect.top >= 8 && rect.right <= innerWidth - 8 && rect.bottom <= innerHeight - 8;
  });
  assert.ok(await menu.isVisible(), "the fallback menu should remain usable in a narrow window");
  console.log("PASS: narrow titles, portal placement, preserved actions, outside click, Escape and keyboard navigation");
  await page.close();

  const appPage = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  appPage.setDefaultTimeout(10000);
  await appPage.goto(new URL("/?ui-preview=1", baseUrl).href);
  const appRows = appPage.locator(".session-item");
  await appPage.waitForFunction(() => document.querySelectorAll(".session-item").length > 0);
  const selectedRow = appRows.first();
  await selectedRow.click();
  await appPage.waitForFunction(() => document.querySelector(".session-item.active") !== null);
  await appPage.screenshot({ path: resolve("output/playwright/session-list-app-preview.png"), fullPage: true });
  await selectedRow.locator(".session-actions-trigger").click();
  const appMenu = appPage.locator(".session-action-menu");
  await appMenu.waitFor({ state: "visible" });
  await appPage.waitForFunction(() => {
    const menuRect = document.querySelector(".session-action-menu").getBoundingClientRect();
    const sidebarRect = document.querySelector(".sidebar").getBoundingClientRect();
    return menuRect.left >= sidebarRect.right;
  });
  await appPage.screenshot({ path: resolve("output/playwright/session-list-app-menu-right.png"), fullPage: true });
  await appPage.keyboard.press("Escape");
  await appMenu.waitFor({ state: "hidden" });
  assert.equal(await selectedRow.evaluate(element => element.classList.contains("active")), true, "App's global Escape handler must leave the selected session active while the menu closes");
  console.log("PASS: app-level Escape closes the menu without clearing selected session state");
  await appPage.close();
} finally {
  await browser.close();
}
