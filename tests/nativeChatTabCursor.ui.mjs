// Run against Vite: CREWVELA_UI_URL=http://127.0.0.1:1420 node tests/nativeChatTabCursor.ui.mjs
import { chromium } from "playwright";

const baseUrl = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
page.setDefaultTimeout(10000);

async function expectCursor(locator, expected, description) {
  const actual = await locator.evaluate(element => getComputedStyle(element).cursor);
  if (actual !== expected) throw new Error(`${description}: expected cursor ${expected}, got ${actual}`);
}

try {
  await page.goto(new URL("/tests/fixtures/native-chat-tabs-ui.html", baseUrl).href);
  const tabs = page.locator(".native-chat-tab");
  await page.waitForFunction(() => document.querySelectorAll(".native-chat-tab [role=tab]").length === 7);

  for (let index = 0; index < await tabs.count(); index++) {
    const tab = tabs.nth(index).locator("[role=tab]");
    const icon = tab.locator("img");
    await tab.hover();
    await expectCursor(tab, "pointer", `tab ${index + 1} hover`);
    await expectCursor(icon, "pointer", `tab ${index + 1} icon hover`);
  }
  console.log("PASS: all tab and icon hover styles use the standard pointer cursor");

  const source = page.locator('[data-native-tab-id="agent-claude_chat"] [role=tab]');
  const sourceIcon = source.locator("img");
  const target = page.locator('[data-native-tab-id="agent-codex_chat"] [role=tab]');
  const sourceBounds = await source.boundingBox();
  if (!sourceBounds) throw new Error("drag source is not visible");

  await page.mouse.move(sourceBounds.x + sourceBounds.width / 2, sourceBounds.y + sourceBounds.height / 2);
  await page.mouse.down();
  await page.mouse.move(sourceBounds.x + sourceBounds.width / 2 + 8, sourceBounds.y + sourceBounds.height / 2);
  await page.waitForFunction(() => document.querySelector('.native-chat-tab.sorting') !== null);
  await expectCursor(source, "move", "sorting tab");
  await expectCursor(sourceIcon, "move", "sorting tab icon");
  console.log("PASS: drag styles use the standard move cursor on the tab and icon");

  const targetBounds = await target.boundingBox();
  if (!targetBounds) throw new Error("drop target is not visible");
  await page.mouse.move(targetBounds.x + targetBounds.width * 0.2, targetBounds.y + targetBounds.height / 2);
  await page.mouse.up();
  await page.waitForFunction(() => {
    const ids = [...document.querySelectorAll(".native-chat-tab")].map(tab => tab.getAttribute("data-native-tab-id"));
    return ids.indexOf("agent-claude_chat") + 1 === ids.indexOf("agent-codex_chat");
  });
  console.log("PASS: horizontal drag reorders tabs");

  await page.waitForTimeout(450);
  await source.click();
  if (await source.getAttribute("aria-selected") !== "true") throw new Error("ordinary tab click did not select the tab after sorting");
  console.log("PASS: ordinary tab click still selects the tab after sorting");
} finally {
  await browser.close();
}
