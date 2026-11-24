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
  const topbarBounds = await page.locator(".grok-topbar").boundingBox();
  const listBounds = await taskList.boundingBox();
  assert.ok(topbarBounds.height <= 50, "Grok topbar stays single-row");
  assert.ok(listBounds.width <= 782 && listBounds.width >= 700, "wide task list stays centered within readable Grok canvas width");
  const task = taskList.getByRole("button", { name: /方案与验收/ });
  await task.click();
  const detail = tasksPanel.getByRole("article", { name: "任务详情 方案与验收" });
  await detail.waitFor();
  // Measure both after opening the detail, in the same frame of the project rail transition.
  await tasksPanel.evaluate(panel => {
    const list = panel.querySelector(".team-task-list").getBoundingClientRect();
    const detail = panel.querySelector(".team-task-detail").getBoundingClientRect();
    if (Math.abs(list.width - detail.width) > 0.05 || Math.abs(list.x - detail.x) > 0.05) {
      throw new Error("Task list and detail must align");
    }
  });

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
  await app.evaluate(() => {
    const mock = window.appSessionMock;
    const invoke = mock.invoke.bind(mock);
    mock.invoke = async (command, args) => {
      if (command === "plugin:clipboard-manager|write_text") { window.layoutCopied = args.text; return; }
      if (command === "ma_conversation_history") return [];
      const result = await invoke(command, args);
      if (command !== "ma_conversation_snapshot" || args.projectId !== "A") return result;
      const at = Date.now();
      const message = { projectId: "A", conversationId: result.conversation.id, phase: "chat", runId: null, createdAt: at, updatedAt: at };
      return { ...result, messages: [
        { ...message, id: "layout-prompt", senderType: "user", kind: "message", status: "completed", body: "请确认支付流程的验收结果。" },
        { ...message, id: "layout-reply", senderType: "agent", kind: "reply", status: "completed", body: "支付流程已通过验收。", agentId: "agent-A", runId: "layout-run" },
        { ...message, id: "layout-cancelled", senderType: "agent", kind: "reply", status: "cancelled", body: "本次回归已取消。", agentId: "agent-A" },
      ] };
    };
  });
  const shortcuts = app.getByRole("navigation", { name: "主导航" });
  await shortcuts.waitFor();
  assert.equal(await shortcuts.getByRole("button", { name: "Bot 模式", exact: true }).count(), 0);
  const appNav = app.getByRole("navigation", { name: "协作项目列表" });
  const workPanel = app.getByRole("complementary", { name: "右侧栏", exact: true });
  const openBot = workPanel.getByRole("button", { name: "Bot模式", exact: true });
  const returnToSidebar = workPanel.getByRole("button", { name: "返回辅助栏", exact: true });
  const project = workPanel.getByRole("combobox", { name: "协作项目", exact: true });
  await app.getByRole("button", { name: "切换右侧栏", exact: true }).click();
  await workPanel.getByRole("button", { name: "多智能体" }).click();
  await openBot.waitFor();
  assert.equal(await openBot.getAttribute("title"), "Bot模式");
  await openBot.click();
  await appNav.getByRole("button", { name: "协作项目 A 待审批" }).waitFor();
  await shortcuts.waitFor({ state: "hidden" });
  const appEditor = app.getByRole("combobox", { name: "给团队发消息", exact: true });
  const prompt = workPanel.locator('[data-message-id="layout-prompt"]');
  const reply = workPanel.locator('[data-message-id="layout-reply"]');
  const checkUserMessage = async () => {
    await prompt.getByRole("img", { name: "我", exact: true }).waitFor();
    const layout = await prompt.evaluate(row => {
      const bubble = row.querySelector(".conversation-user-bubble").getBoundingClientRect();
      const avatar = row.querySelector(".conversation-user-avatar");
      const icon = avatar.getBoundingClientRect();
      const { color, borderRadius } = getComputedStyle(avatar);
      return {
        rowRight: row.getBoundingClientRect().right, bubbleRight: bubble.right, bubbleTop: bubble.top,
        iconRight: icon.right, iconBottom: icon.bottom, style: { color, borderRadius, width: icon.width, height: icon.height },
      };
    });
    assert.ok(Math.abs(layout.rowRight - layout.bubbleRight) < 2, "The user's bubble is right-aligned");
    assert.ok(Math.abs(layout.iconRight - layout.bubbleRight) < 2 && layout.iconBottom <= layout.bubbleTop,
      "The user avatar sits above the right edge of the bubble");
    assert.equal(layout.style.borderRadius, "50%");
    return layout.style;
  };
  const botAvatarStyle = await checkUserMessage();
  const actions = reply.locator(".member-activity-summary");
  const activity = actions.getByRole("button", { name: "查看活动" });
  await activity.click();
  await reply.getByRole("list", { name: "成员活动时间线" }).waitFor();
  await actions.getByRole("button", { name: "收起活动" }).click();
  await actions.getByRole("button", { name: "复制消息" }).click();
  await actions.locator(".conversation-copy-feedback").getByText("已复制", { exact: true }).waitFor();
  assert.equal(await app.evaluate(() => window.layoutCopied), "支付流程已通过验收。");
  await workPanel.locator('[data-message-id="layout-cancelled"]').getByText("已取消", { exact: true }).waitFor();
  const canvas = await workPanel.locator(".conversation-transcript-inner").boundingBox();
  const composer = await workPanel.locator(".multi-agent-composer").boundingBox();
  assert.ok(Math.abs(canvas.x - composer.x) <= 1 && Math.abs(canvas.width - composer.width) <= 1,
    "Bot transcript and composer use the same column");
  const topbar = workPanel.locator(".grok-topbar");
  const controls = [
    topbar.getByRole("heading", { name: "协作项目 A" }), topbar.getByRole("button", { name: /^切换会话：/ }),
    topbar.getByRole("tablist", { name: "协作视图" }), topbar.getByRole("button", { name: /^查看任务/ }),
    topbar.getByRole("button", { name: /^选择参与成员/ }), topbar.getByRole("button", { name: "展开右侧执行画布" }),
    topbar.getByRole("button", { name: "讨论记录操作" }), topbar.getByRole("button", { name: "管理项目与成员" }),
  ];
  const positions = await Promise.all(controls.map(control => control.boundingBox()));
  assert.ok(positions.every((bounds, index) => bounds && (index === 0 || bounds.x > positions[index - 1].x)),
    "Bot toolbar retains the current control order");
  const sessionPicker = controls[1];
  assert.equal(await sessionPicker.getAttribute("aria-expanded"), "false");
  await sessionPicker.click();
  await topbar.locator(".conversation-switch-popover button[aria-current='true']").waitFor();
  await sessionPicker.press("Escape");
  assert.equal(await sessionPicker.getAttribute("aria-expanded"), "false");
  await appEditor.fill("保留在原面板的输入");
  await returnToSidebar.click();
  await appNav.waitFor({ state: "hidden" });
  await shortcuts.waitFor();
  await reply.getByText("已完成", { exact: true }).waitFor();
  assert.deepEqual(await checkUserMessage(), botAvatarStyle, "The same avatar remains visible in the docked panel");
  assert.equal(await project.inputValue(), "A");
  assert.equal(await appEditor.inputValue(), "保留在原面板的输入");
  assert.equal(await openBot.getAttribute("title"), "Bot模式");
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

  await openBot.click();
  await appNav.getByRole("button", { name: "协作项目 B 运行中" }).waitFor();
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
  const agyWorkPanel = antigravity.getByRole("complementary", { name: "右侧栏", exact: true });
  await antigravity.getByRole("button", { name: "切换右侧栏", exact: true }).click();
  await agyWorkPanel.getByRole("button", { name: "多智能体" }).click();
  await agyWorkPanel.getByRole("button", { name: "Bot模式", exact: true }).click();
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
