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
  await search.fill("browser control");
  await page.getByRole("button", { name: "Browser control", exact: true }).click();
  await page.getByText("Allow agents to control the browser", { exact: true }).waitFor();
  await page.getByText("Prefer the built-in browser panel", { exact: true }).waitFor();
  await search.fill("privacy");
  await page.getByRole("button", { name: "Privacy", exact: true }).click();
  await page.getByText("Local-first storage", { exact: true }).waitFor();
  await page.getByRole("button", { name: "Where your data is stored", exact: true }).click();
  await page.getByRole("heading", { name: "Local application records", exact: true }).waitFor();
  await page.getByRole("button", { name: "Close privacy information", exact: true }).click();
  await search.fill("shared memory");
  await page.getByRole("button", { name: "Shared memory", exact: true }).click();
  await page.getByRole("button", { name: "More options", exact: true }).click();
  await page.getByRole("menuitem", { name: "Legacy shared memory", exact: true }).click();
  await page.getByRole("button", { name: "Advanced settings", exact: true }).click();
  await page.getByRole("heading", { name: "Local service information", exact: true }).waitFor();
  await page.getByRole("button", { name: "Close settings" }).click();
  await page.getByRole("button", { name: "New", exact: true }).waitFor();
  await page.getByRole("button", { name: "Dismiss messaging tip" }).click();
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("button", { name: "Pin Project management to main sidebar" }).waitFor();
  await page.getByRole("button", { name: "Pin Quick commands to main sidebar" }).waitFor();
  await page.getByRole("button", { name: "Pin Privacy to main sidebar" }).waitFor();
  await page.getByRole("button", { name: "Close settings" }).click();

  const runtimePage = await context.newPage();
  await runtimePage.goto(new URL("/tests/fixtures/app-session-async-ui.html?cli-launch=1", base).href);
  await runtimePage.waitForFunction(() => window.appSessionMock?.nativeLanguageUpdates.at(-1) === "en");
  await runtimePage.getByRole("button", { name: "Select agent" }).click();
  await runtimePage.getByRole("button", { name: "Manage agents" }).click();
  const runtimes = runtimePage.getByRole("dialog", { name: "Manage runtimes" });
  await runtimes.getByRole("heading", { name: "Manage runtimes" }).waitFor();

  const failedTrayUpdate = await context.newPage();
  await failedTrayUpdate.goto(new URL("/tests/fixtures/app-session-async-ui.html?cli-launch=1&tray-language-failure=1", base).href);
  const trayAlert = failedTrayUpdate.getByRole("alert").filter({ hasText: "Could not update the system tray language" });
  await trayAlert.waitFor();
  const details = trayAlert.getByText("无法写入偏好文件", { exact: false });
  assert.equal(await details.isVisible(), false);
  await trayAlert.getByText("Error details", { exact: true }).click();
  await details.waitFor();
  await failedTrayUpdate.close();

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
  await runtimePage.waitForFunction(() => window.appSessionMock?.nativeLanguageUpdates.at(-1) === "zh-CN");
  await runtimePage.close();
  await teamPage.getByRole("tab", { name: "团队聊天" }).waitFor();
  await page.getByRole("combobox", { name: "界面语言" }).waitFor();
  await page.getByRole("button", { name: "关闭设置" }).click();
  await page.getByRole("button", { name: "新建", exact: true }).waitFor();
  assert.equal(await page.locator("html").getAttribute("lang"), "zh-CN");
  console.log("Interface language switches immediately, survives reload, and fits the narrow window.");
} finally {
  await browser.close();
}
