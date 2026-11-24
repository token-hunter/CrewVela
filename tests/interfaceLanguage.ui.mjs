import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });

try {
  const context = await browser.newContext({ viewport: { width: 860, height: 720 } });
  const page = await context.newPage();
  page.setDefaultTimeout(10000);
  await page.goto(new URL("/tests/fixtures/app-session-async-ui.html?cli-launch=1&ui-preview=1", base).href);

  await page.getByRole("button", { name: "设置", exact: true }).click();
  const language = page.locator(".general-interface-language select");
  assert.equal(await language.inputValue(), "zh-CN");
  const proxy = page.locator("#general-http-proxy");
  await proxy.fill("http://127.0.0.1:8111");
  await language.selectOption("en");

  await page.getByRole("combobox", { name: "Interface language" }).waitFor();
  assert.equal(await page.locator("html").getAttribute("lang"), "en");
  assert.equal(await proxy.inputValue(), "http://127.0.0.1:8111");

  const search = page.getByRole("searchbox", { name: "Search settings" });
  await search.fill("archived sessions");
  await page.getByRole("button", { name: "Archived sessions", exact: true }).click();
  await page.getByRole("heading", { name: "Archived sessions", exact: true }).waitFor();
  await page.getByRole("button", { name: "Close settings" }).click();
  await page.getByRole("button", { name: "New session", exact: true }).waitFor();
  await page.getByRole("button", { name: "Dismiss messaging tip" }).click();
  await page.getByRole("button", { name: "Customize shortcuts" }).click();
  const shortcuts = page.getByRole("group", { name: "Customize shortcuts" });
  await shortcuts.getByRole("checkbox", { name: "Project management" }).waitFor();
  await shortcuts.getByRole("checkbox", { name: "Quick commands" }).waitFor();
  await shortcuts.getByRole("checkbox", { name: "Privacy" }).waitFor();
  await page.mouse.click(835, 650);
  await shortcuts.waitFor({ state: "hidden" });

  const runtimePage = await context.newPage();
  await runtimePage.goto(new URL("/tests/fixtures/app-session-async-ui.html?cli-launch=1", base).href);
  await runtimePage.getByRole("button", { name: "Select agent" }).click();
  await runtimePage.getByRole("button", { name: "Manage agents" }).click();
  const runtimes = runtimePage.getByRole("dialog", { name: "Manage runtimes" });
  await runtimes.getByRole("heading", { name: "Manage runtimes" }).waitFor();
  await runtimePage.close();

  const teamPage = await context.newPage();
  await teamPage.goto(new URL("/tests/fixtures/collaboration-conversation-ui.html", base).href);
  await teamPage.getByRole("tab", { name: "Team chat" }).waitFor();
  await teamPage.getByRole("button", { name: /Planning and auto-management · \d+ selected/ }).waitFor();

  await page.reload();
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  assert.equal(await language.inputValue(), "en");
  assert.equal(await page.locator("html").getAttribute("lang"), "en");
  const bounds = await language.boundingBox();
  assert.ok(bounds && bounds.x >= 0 && bounds.x + bounds.width <= 860);

  await language.selectOption("zh-CN");
  await teamPage.getByRole("tab", { name: "团队聊天" }).waitFor();
  await page.getByRole("combobox", { name: "界面语言" }).waitFor();
  await page.getByRole("button", { name: "关闭设置" }).click();
  await page.getByRole("button", { name: "新建会话", exact: true }).waitFor();
  assert.equal(await page.locator("html").getAttribute("lang"), "zh-CN");
  console.log("Interface language switches immediately, survives reload, and fits the narrow window.");
} finally {
  await browser.close();
}
