import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1432";
const browser = await chromium.launch({ channel: process.env.CREWVELA_BROWSER_CHANNEL || "chrome" });
try {
  const page = await browser.newPage({ viewport: { width: 1295, height: 992 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?project-overview=1`);

  const nav = page.getByRole("navigation", { name: "协作项目列表" });
  const approvals = nav.getByRole("button", { name: "待审批 1", exact: true });
  const running = nav.getByRole("button", { name: "运行中 2", exact: true });
  const shop = nav.getByRole("button", { name: "小栈商城 待审批" });
  const converter = nav.getByRole("button", { name: "单位换算器 运行中" });
  const search = nav.getByRole("searchbox", { name: "搜索协作项目" });
  const editor = page.getByRole("combobox", { name: "给团队发消息", exact: true });
  await approvals.waitFor();
  assert.deepEqual(await nav.locator(".collaboration-project-group-toggle").allTextContents(), ["待审批 1", "运行中 2", "已交付 1"]);
  assert.equal(await shop.getAttribute("aria-current"), "true");

  await editor.fill("商城项目专属草稿");
  await running.click();
  await converter.waitFor({ state: "hidden" });
  assert.equal(await running.getAttribute("aria-expanded"), "false");
  await search.fill("docs");
  await converter.waitFor();
  assert.equal(await shop.count(), 0);
  await converter.click();
  await page.getByRole("heading", { name: "单位换算器", exact: true }).waitFor();
  assert.equal(await editor.inputValue(), "");
  assert.equal(await page.getByRole("log").getByText("按这个分工执行。").count(), 0);
  await editor.fill("换算器项目专属草稿");

  await search.clear();
  await shop.click();
  await page.waitForFunction(() => document.querySelector("textarea[aria-label='给团队发消息']")?.value === "商城项目专属草稿");
  await page.setViewportSize({ width: 2240, height: 580 });
  await page.getByRole("tab", { name: "任务执行", exact: true }).click();
  const tasksPanel = page.getByRole("tabpanel", { name: "任务执行" });
  const taskList = tasksPanel.locator(".team-task-list");
  await taskList.waitFor();
  const tabsBounds = await page.locator(".conversation-tabs-row").boundingBox();
  const listBounds = await taskList.boundingBox();
  assert.equal(listBounds.width, tabsBounds.width, "wide task list shares the tabs' content width");
  assert.ok(Math.abs(listBounds.x - tabsBounds.x) <= 1, "wide task list aligns with the tabs even when the panel scrolls");
  const task = taskList.getByRole("button", { name: /方案与验收/ });
  await task.click();
  const detail = tasksPanel.getByRole("article", { name: "任务详情 方案与验收" });
  await detail.waitFor();
  const detailBounds = await detail.boundingBox();
  assert.equal(detailBounds.width, listBounds.width);
  assert.equal(detailBounds.x, listBounds.x);

  await page.setViewportSize({ width: 390, height: 820 });
  await taskList.getByRole("button", { name: /列表与筛选/ }).click();
  await tasksPanel.getByRole("article", { name: "任务详情 列表与筛选" }).waitFor();
  assert.equal(await tasksPanel.evaluate(panel => panel.scrollWidth <= panel.clientWidth), true);
  await page.setViewportSize({ width: 1295, height: 992 });
  await page.getByRole("tab", { name: "团队聊天", exact: true }).click();
  await nav.getByRole("button", { name: "新建协作" }).click();
  const create = page.getByRole("dialog", { name: "创建协作项目" });
  await create.getByRole("button", { name: "取消" }).click();
  await create.waitFor({ state: "hidden" });

  await page.setViewportSize({ width: 520, height: 900 });
  const drawer = nav.getByRole("button", { name: "协作项目", exact: true });
  await drawer.waitFor();
  await search.waitFor({ state: "hidden" });
  await drawer.click();
  await running.click();
  await converter.click();
  await search.waitFor({ state: "hidden" });
  await page.waitForFunction(() => document.querySelector("textarea[aria-label='给团队发消息']")?.value === "换算器项目专属草稿");
  assert.equal(await drawer.evaluate(el => el === document.activeElement), true);
  assert.deepEqual(errors, []);

  const app = await browser.newPage({ viewport: { width: 1500, height: 900 } });
  await app.goto(`${base}/tests/fixtures/app-session-async-ui.html?tasks-nav=1`);
  const shortcut = app.getByRole("navigation", { name: "主导航" }).getByRole("button", { name: "多智能体 · 协作项目总览", exact: true });
  const appNav = app.getByRole("navigation", { name: "协作项目列表" });
  await shortcut.click();
  await appNav.getByRole("button", { name: "协作项目 A 待审批" }).waitFor();
  const appEditor = app.getByRole("combobox", { name: "给团队发消息", exact: true });
  await appEditor.fill("保留在原面板的输入");
  await shortcut.click();
  await appNav.waitFor({ state: "hidden" });
  assert.equal(await app.getByRole("combobox", { name: "协作项目", exact: true }).inputValue(), "A");
  assert.equal(await appEditor.inputValue(), "保留在原面板的输入");
  await shortcut.click();
  await appNav.getByRole("button", { name: "协作项目 B 运行中" }).click();
  await app.getByRole("heading", { name: "协作项目 B", exact: true }).waitFor();
  console.log("协作项目导航通过：状态分组、搜索、草稿隔离、任务入口、新建协作与窄窗切换");
} finally {
  await browser.close();
}
