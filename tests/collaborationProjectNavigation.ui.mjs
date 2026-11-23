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

  const rename = page.getByRole("dialog", { name: "重命名项目", exact: true });
  const name = rename.getByRole("textbox", { name: "项目名称", exact: true });
  const save = rename.getByRole("button", { name: "保存", exact: true });
  const shopActions = nav.getByRole("button", { name: "小栈商城的项目操作", exact: true });
  const openRename = async actions => {
    await actions.click();
    await page.getByRole("menuitem", { name: "重命名项目", exact: true }).click();
    await rename.waitFor();
  };
  await shopActions.focus();
  await page.keyboard.press("Enter");
  const menu = page.getByRole("menu", { name: "小栈商城的项目操作", exact: true });
  await menu.waitFor();
  await page.keyboard.press("Escape");
  await menu.waitFor({ state: "hidden" });
  assert.ok(await shopActions.evaluate(el => document.activeElement === el));

  await openRename(shopActions);
  assert.equal(await name.inputValue(), "小栈商城");
  await name.fill("   ");
  assert.ok(await save.isDisabled());
  await name.fill("  小栈商城 · 验收  ");
  await page.evaluate(() => window.conversationMock.failProjectRename = true);
  await save.click();
  await rename.getByRole("alert").filter({ hasText: /项目名称未保存/ }).waitFor();
  assert.equal(await name.inputValue(), "  小栈商城 · 验收  ");
  await page.getByRole("heading", { name: "小栈商城", exact: true }).waitFor();
  await page.evaluate(() => window.conversationMock.failProjectRename = false);
  await save.click();
  await rename.waitFor({ state: "hidden" });
  await page.getByRole("heading", { name: "小栈商城 · 验收", exact: true }).waitFor();
  assert.equal(await nav.getByRole("button", { name: "小栈商城 · 验收 待审批", exact: true }).getAttribute("aria-current"), "true");
  await search.fill("商城 · 验收");
  await nav.getByRole("button", { name: "小栈商城 · 验收 待审批", exact: true }).waitFor();
  assert.equal(await converter.count(), 0);
  await search.clear();
  assert.equal(await editor.inputValue(), "商城项目专属草稿");
  await page.getByRole("log").getByText("按这个分工执行。").waitFor();
  await page.getByRole("tab", { name: "任务执行", exact: true }).click();
  await task.waitFor();
  await page.getByRole("tab", { name: "团队聊天", exact: true }).click();

  await page.setViewportSize({ width: 520, height: 900 });
  const drawer = nav.getByRole("button", { name: "协作项目", exact: true });
  await drawer.waitFor();
  await search.waitFor({ state: "hidden" });
  await drawer.click();
  await openRename(nav.getByRole("button", { name: "星潮方块的项目操作", exact: true }));
  await name.fill("星潮方块 · 已验收");
  await save.click();
  await rename.waitFor({ state: "hidden" });
  await nav.getByRole("button", { name: "星潮方块 · 已验收 已交付", exact: true }).waitFor();
  await page.getByRole("heading", { name: "小栈商城 · 验收", exact: true }).waitFor();
  const renames = await page.evaluate(() => window.conversationMock.calls.filter(item => item.command === "ma_rename_project").map(item => item.args));
  assert.deepEqual(renames, [
    { projectId: "A", name: "小栈商城 · 验收" },
    { projectId: "A", name: "小栈商城 · 验收" },
    { projectId: "D", name: "星潮方块 · 已验收" },
  ]);
  await running.click();
  await converter.click();
  await search.waitFor({ state: "hidden" });
  await page.waitForFunction(() => document.querySelector("textarea[aria-label='给团队发消息']")?.value === "换算器项目专属草稿");
  assert.equal(await drawer.evaluate(el => el === document.activeElement), true);
  assert.deepEqual(errors, []);

  const app = await browser.newPage({ viewport: { width: 1500, height: 900 } });
  app.on("pageerror", error => errors.push(error.message));
  await app.addInitScript(() => {
    localStorage.setItem("crewvela.team-chat-recipient.B.conversation-B", "agent-B");
  });
  await app.goto(`${base}/tests/fixtures/app-session-async-ui.html?tasks-nav=1`);
  const shortcut = app.getByRole("navigation", { name: "主导航" }).getByRole("button", { name: "Bot 模式", exact: true });
  const appNav = app.getByRole("navigation", { name: "协作项目列表" });
  const workPanel = app.getByRole("complementary", { name: "右侧栏", exact: true });
  const openBot = workPanel.getByRole("button", { name: "在 Bot 模式打开", exact: true });
  const returnToSidebar = workPanel.getByRole("button", { name: "返回辅助栏", exact: true });
  const project = workPanel.getByRole("combobox", { name: "协作项目", exact: true });
  await shortcut.click();
  await appNav.getByRole("button", { name: "协作项目 A 待审批" }).waitFor();
  const appEditor = app.getByRole("combobox", { name: "给团队发消息", exact: true });
  await appEditor.fill("保留在原面板的输入");
  await shortcut.click();
  await appNav.waitFor({ state: "hidden" });
  assert.equal(await project.inputValue(), "A");
  assert.equal(await appEditor.inputValue(), "保留在原面板的输入");
  assert.equal(await openBot.getAttribute("title"), "在 Bot 模式打开");
  const splitWidth = await workPanel.evaluate(panel => parseFloat(getComputedStyle(panel).getPropertyValue("--right-sidebar-width")));
  await openBot.press("Enter");
  await appNav.getByRole("button", { name: "协作项目 B 运行中" }).click();
  await app.getByRole("heading", { name: "协作项目 B", exact: true }).waitFor();
  await returnToSidebar.click();
  await appNav.waitFor({ state: "hidden" });
  assert.equal(await project.inputValue(), "B");
  await app.waitForFunction(width => {
    const panel = document.querySelector(".unified-right-sidebar");
    return panel && Math.abs(panel.getBoundingClientRect().width - width) < 1;
  }, splitWidth);

  await workPanel.getByRole("button", { name: "新建工作面板标签" }).click();
  await workPanel.getByRole("menuitem", { name: "多智能体", exact: true }).click();
  const collaborationTabs = workPanel.getByRole("tab", { name: "多智能体", exact: true });
  assert.equal(await collaborationTabs.count(), 2);
  assert.equal(await collaborationTabs.last().getAttribute("aria-selected"), "true");
  const recipient = workPanel.getByRole("button", { name: "发给 成员 B，切回发给全体", exact: true });
  const conversation = workPanel.getByRole("button", { name: /^切换会话：/ });
  await recipient.waitFor();
  const conversationName = await conversation.getAttribute("aria-label");
  await appEditor.fill("验证付款流程的未发送草稿");
  const originalEditor = await appEditor.elementHandle();

  await shortcut.click();
  await appNav.getByRole("button", { name: "协作项目 B 运行中" }).waitFor();
  assert.equal(await shortcut.getAttribute("aria-pressed"), "true");
  assert.equal(await collaborationTabs.last().getAttribute("aria-selected"), "true");
  assert.equal(await returnToSidebar.getAttribute("title"), "返回辅助栏");
  assert.equal(await conversation.getAttribute("aria-label"), conversationName);
  assert.equal(await appEditor.inputValue(), "验证付款流程的未发送草稿");
  await recipient.waitFor();

  await returnToSidebar.press("Enter");
  await openBot.waitFor();
  assert.equal(await openBot.evaluate(el => el === document.activeElement), true);
  assert.equal(await collaborationTabs.last().getAttribute("aria-selected"), "true");
  assert.equal(await project.inputValue(), "B");
  assert.equal(await conversation.getAttribute("aria-label"), conversationName);
  assert.equal(await appEditor.inputValue(), "验证付款流程的未发送草稿");
  assert.equal(await appEditor.evaluate((el, original) => el === original, originalEditor), true);
  await recipient.waitFor();
  assert.deepEqual(errors, []);
  const antigravity = await browser.newPage({ viewport: { width: 1500, height: 900 } });
  antigravity.on("pageerror", error => errors.push(error.message));
  await antigravity.goto(`${base}/tests/fixtures/app-session-async-ui.html?tasks-nav=1`);
  await antigravity.evaluate(() => {
    const mock = window.appSessionMock;
    const original = mock.invoke.bind(mock);
    window.antigravitySends = [];
    mock.invoke = async (command, args) => {
      if (command === "ma_conversation_send") {
        window.antigravitySends.push(args.input);
        return mock.invoke("ma_conversation_snapshot", { projectId: args.input.projectId });
      }
      const result = await original(command, args);
      if (command === "ma_snapshot" && args.projectId === "A") {
        result.agents[0].name = "Antigravity";
        result.agents[0].runtimeKind = "antigravity";
      }
      if (command === "ma_conversation_snapshot" && args.projectId === "A") {
        result.supportedDiscussionRuntimes.push("antigravity");
      }
      return result;
    };
  });
  await antigravity.getByRole("button", { name: "Bot 模式", exact: true }).click();
  const agyEditor = antigravity.getByRole("combobox", { name: "给团队发消息", exact: true });
  const agySend = antigravity.getByRole("button", { name: "发送消息", exact: true });
  await antigravity.getByRole("button", { name: "分工与托管 · 已选 0 位", exact: true }).click();
  const planMembers = antigravity.locator(".conversation-recipient-menu");
  await planMembers.waitFor();
  assert.equal(await planMembers.getByText("Antigravity").count(), 0);
  await agyEditor.fill("@Antigravity 记住项目验收标记。");
  await agySend.click();
  const agyRecipient = antigravity.getByRole("button", { name: "发给 Antigravity，切回发给全体", exact: true });
  await agyRecipient.waitFor();
  await antigravity.getByRole("button", { name: "返回辅助栏", exact: true }).click();
  await agyRecipient.waitFor();
  await agyEditor.fill("上一轮的标记是什么？");
  await agySend.click();
  await antigravity.waitForFunction(() => window.antigravitySends.length === 2);
  const agyRoutes = await antigravity.evaluate(() => window.antigravitySends.map(input =>
    [input.expectedConversationId, input.mode, input.participantAgentIds]));
  assert.deepEqual(agyRoutes, [
    ["conversation-A", "discussion", ["agent-A"]],
    ["conversation-A", "discussion", ["agent-A"]],
  ]);
  assert.deepEqual(errors, []);
  console.log("协作项目导航和 Bot 模式通过：项目分组、改名、任务入口、原会话、草稿和收件人保留");
} finally {
  await browser.close();
}
