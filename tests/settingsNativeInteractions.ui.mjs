// Fixture-only interaction regression. No real credentials or Agent processes.
import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: process.env.CREWVELA_BROWSER_CHANNEL || "chrome" });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1024 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(`${base}/tests/fixtures/crewvela-settings-frame-ui.html?ui-preview=1&theme=light&mock=general&fail=1`);
  const opener = page.getByRole("button", { name: "打开设置回归", exact: true });
  await opener.click();
  const proxy = page.getByRole("textbox", { name: "HTTP 代理", exact: true });
  const save = page.getByRole("button", { name: "保存并应用", exact: true });
  await proxy.fill("invalid-proxy");
  assert.equal(await save.isEnabled(), false);
  await page.getByText(/代理地址必须是/).waitFor();
  await proxy.fill("http://127.0.0.1:9876");
  await save.click();
  await page.getByText(/保存失败.*回归测试：写入失败/).waitFor();
  assert.equal(await proxy.inputValue(), "http://127.0.0.1:9876");
  await save.click();
  await page.getByText(/已保存并应用/).waitFor();
  assert.equal(await save.isEnabled(), false);
  const search = page.getByRole("searchbox", { name: "搜索设置", exact: true });
  await search.fill("mcp");
  assert.deepEqual(await page.locator(".settings-nav-item").allTextContents(), ["MCP"]);
  await search.fill("不存在的设置");
  await page.getByText("没有匹配的设置", { exact: true }).waitFor();
  await search.press("Escape");
  assert.equal(await page.locator(".settings-native").count(), 0);
  assert.equal(await opener.evaluate(el => el === document.activeElement), true);

  await page.goto(`${base}/tests/fixtures/crewvela-settings-frame-ui.html?ui-preview=1&theme=light&archive=long`);
  await opener.click();
  const nav = page.getByRole("navigation", { name: "设置分类" });
  await nav.getByRole("button", { name: "协作中心", exact: true }).click();
  await page.getByRole("button", { name: "创建项目", exact: true }).click();
  const project = page.getByRole("dialog", { name: "创建协作项目" });
  await project.waitFor();
  assert.equal(await project.evaluate(el => el.closest(".settings-native") === null), true);
  await project.getByLabel("项目名称", { exact: true }).press("Escape");
  assert.equal(await project.count(), 0);
  assert.equal(await page.locator(".settings-native").count(), 1);
  await page.getByRole("tab", { name: /团队/ }).click();
  await page.getByRole("button", { name: "新增成员", exact: true }).click();
  await page.getByRole("dialog", { name: "新增团队成员" }).waitFor();
  await page.getByRole("button", { name: "关闭成员编辑", exact: true }).click();
  assert.equal(await page.getByRole("dialog", { name: "新增团队成员" }).count(), 0);
  await nav.getByRole("button", { name: "归档会话", exact: true }).click();
  assert.equal(await page.locator(".archived-settings-row").count(), 100);
  await page.locator(".settings-content").evaluate(el => { el.scrollTop = el.scrollHeight; });
  assert.ok(await page.locator(".settings-content").evaluate(el => el.scrollTop > 0));
  assert.equal(await page.evaluate(() => document.documentElement.scrollTop), 0);
  assert.equal(await page.getByRole("button", { name: "关闭设置", exact: true }).isVisible(), true);
  assert.deepEqual(errors, []);
  console.log("PASS: settings validation/save/error retry, search, focus return, nested dialogs and independent archive scrolling");
} finally { await browser.close(); }
