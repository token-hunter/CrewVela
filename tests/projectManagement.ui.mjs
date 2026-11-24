import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  page.setDefaultTimeout(10000);
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(new URL("/tests/fixtures/app-session-async-ui.html?cli-launch=1", base).href, { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() => window.appSessionMock);
  await page.evaluate(async () => {
    const mock = window.appSessionMock;
    const invoke = mock.invoke.bind(mock);
    const projects = await invoke("list_projects", {});
    const states = {}, calls = [];
    window.projectManagementMock = { projects, states, calls, failStart: "" };
    mock.cliLaunch.pickerValue = "F:\\project-fixture";
    mock.invoke = async (command, args = {}) => {
      if (command === "list_projects") return structuredClone(projects);
      if (command === "ma_list_projects") return [
        { id: "team", name: "本地项目协作", rootPath: "F:\\project-fixture" },
        ...Array.from({ length: 60 }, (_, index) => ({
          id: `team-${index + 1}`, name: `协作项目 ${index + 1}`, rootPath: `F:\\project-fixture-${index + 1}`,
        })),
      ];
      if (command === "create_project") {
        const added = { id: "added", name: args.name, path: args.path, createdAt: Date.now() };
        projects.push(added);
        return added;
      }
      if (command === "update_project") {
        const project = projects.find(item => item.id === args.id);
        project.name = args.name;
        return structuredClone(project);
      }
      if (command === "edit_local_project") {
        calls.push(structuredClone(args));
        const { id, edit } = args, project = projects.find(item => item.id === id);
        if (edit.kind === "directory") project.path = edit.path;
        if (edit.kind === "launch") project.launch = edit.config;
        if (edit.kind === "archive") project.archivedAt = edit.archived ? Date.now() : undefined;
        if (edit.kind === "opened") project.lastOpenedAt = Date.now();
        if (edit.kind === "collaboration") project.collaborationProjectIds = edit.ids;
        return structuredClone(project);
      }
      if (command === "local_project_service_statuses") return structuredClone(states);
      if (command === "local_project_service_start") {
        calls.push({ command, ...args });
        if (window.projectManagementMock.failStart === args.id && !args.recheck) throw Error("未找到明确的启动入口");
        return (states[args.id] = { state: "running" });
      }
      if (command === "local_project_service_stop") return (states[args.id] = { state: "idle" });
      if (command === "local_project_service_logs") return { ...states[args.id], logs: `test log for ${args.id}` };
      return invoke(command, args);
    };
  });

  await page.getByRole("button", { name: "设置", exact: true }).click();
  const nav = page.locator(".settings-nav");
  await nav.getByRole("button", { name: "项目管理", exact: true }).click();
  assert.deepEqual(await nav.locator(".settings-nav-group", { hasText: "协作中心" }).getByRole("button").allTextContents(),
    ["协作中心", "项目管理", "共享记忆", "自动化", "快捷指令"]);
  const panel = page.locator(".project-management");
  const card = name => panel.locator("article", { has: page.getByRole("button", { name, exact: true }) });
  const assertCollapsed = async () => {
    await card("Second project").waitFor();
    assert.equal(await panel.getByRole("button", { expanded: true }).count(), 0);
    assert.equal(await panel.getByText("本地开发服务", { exact: true }).filter({ visible: true }).count(), 0);
  };
  await assertCollapsed();
  const fixtureCard = card("Fixture project");
  await fixtureCard.getByRole("button", { name: "展开Fixture project", exact: true }).click();
  assert.equal(await fixtureCard.getByText("本地开发服务", { exact: true }).isVisible(), true);
  const search = panel.getByRole("searchbox");
  await search.fill("Second project");
  await assertCollapsed();
  await search.clear();
  await fixtureCard.getByRole("button", { name: "收起Fixture project", exact: true }).click();
  await assertCollapsed();
  const menu = async (name, action) => {
    await card(name).getByLabel(`${name}的项目操作`, { exact: true }).click();
    await card(name).getByRole("button", { name: action, exact: true }).click();
  };
  const save = async label => {
    const dialog = page.getByRole("dialog", { name: label, exact: true });
    await dialog.getByRole("button", { name: "保存", exact: true }).click();
    await dialog.waitFor({ state: "detached" });
  };
  await panel.getByRole("button", { name: "添加项目", exact: true }).click();
  const add = page.locator(".editor");
  await add.getByRole("button", { name: "选择目录" }).click();
  await add.locator("input").fill("可搜索的新项目");
  await add.getByRole("button", { name: "添加项目", exact: true }).click();
  await card("可搜索的新项目").waitFor();
  await panel.getByRole("searchbox").fill("project-fixture");
  assert.equal(await panel.locator("article").count(), 1);
  await panel.getByRole("searchbox").clear();

  await menu("Fixture project", "改名");
  await page.getByLabel("项目名称", { exact: true }).fill("单位换算器");
  await save("重命名项目");
  await menu("单位换算器", "修改目录");
  await page.getByLabel("项目目录", { exact: true }).fill("F:\\converter-moved");
  await save("修改项目目录");
  await card("单位换算器").getByText("F:\\converter-moved", { exact: true }).waitFor();

  const start = async name => {
    const entry = card(name);
    const title = entry.getByRole("button", { name, exact: true });
    if (await title.getAttribute("aria-expanded") !== "true") await title.click();
    assert.equal(await entry.getByRole("button", { name: /启动配置/ }).count(), 0);
    await entry.getByRole("button", { name: "启动项目", exact: true }).click();
    await entry.getByText("运行中", { exact: true }).waitFor();
    assert.equal(await page.getByRole("dialog", { name: "编辑启动配置" }).count(), 0);
  };
  await start("单位换算器");
  await start("Second project");
  await card("Second project").getByRole("button", { name: "停止项目", exact: true }).click();
  await page.evaluate(() => { window.projectManagementMock.failStart = "second-project"; });
  await card("Second project").getByRole("button", { name: "启动项目", exact: true }).click();
  await card("Second project").getByRole("alert").filter({ hasText: "未找到明确的启动入口" }).waitFor();
  await card("Second project").getByRole("button", { name: "重新识别启动方式", exact: true }).click();
  await card("Second project").getByText("运行中", { exact: true }).waitFor();
  await card("单位换算器").getByRole("button", { name: "单位换算器", exact: true }).click();
  if (process.env.CREWVELA_PROJECT_SCREENSHOT) await page.screenshot({ path: process.env.CREWVELA_PROJECT_SCREENSHOT });
  await page.setViewportSize({ width: 820, height: 640 });
  assert.equal(await panel.evaluate(node => node.scrollWidth <= node.clientWidth), true);
  await page.setViewportSize({ width: 1440, height: 900 });
  await card("单位换算器").getByRole("button", { name: "日志", exact: true }).click();
  const logs = page.getByRole("dialog", { name: "单位换算器的开发服务日志" });
  await logs.getByText("test log for fixture-project").waitFor();
  await logs.getByRole("button", { name: "关闭日志" }).click();
  await card("单位换算器").getByRole("button", { name: "停止项目", exact: true }).click();
  await card("单位换算器").getByText("未运行", { exact: true }).waitFor();
  assert.equal(await page.evaluate(() => window.projectManagementMock.states["second-project"].state), "running");
  await start("单位换算器");
  assert.equal(await page.evaluate(() => window.projectManagementMock.calls.filter(call => call.command === "local_project_service_start" && call.id === "fixture-project").length), 2);
  await card("单位换算器").getByRole("button", { name: "停止项目", exact: true }).click();
  await card("单位换算器").getByText("未运行", { exact: true }).waitFor();
  await page.setViewportSize({ width: 820, height: 500 });
  const linkButton = card("单位换算器").getByRole("button", { name: "关联协作", exact: true });
  await linkButton.click();
  const linkDialog = page.getByRole("dialog", { name: "关联协作项目", exact: true });
  const list = linkDialog.locator(".project-management-links");
  const firstProject = list.getByRole("checkbox", { name: /本地项目协作/ });
  await firstProject.waitFor();
  const cancelLink = linkDialog.getByRole("button", { name: "取消", exact: true });
  const saveLink = linkDialog.getByRole("button", { name: "保存", exact: true });
  const footer = linkDialog.locator("footer");
  const footerBeforeScroll = await footer.boundingBox();
  assert.ok(footerBeforeScroll.y + footerBeforeScroll.height <= 500, "footer is visible before scrolling");
  assert.ok(await list.evaluate(node => node.scrollHeight > node.clientHeight), "only the project list needs scrolling");
  await firstProject.check();
  await cancelLink.click();
  await linkDialog.waitFor({ state: "detached" });
  await linkButton.click();
  await firstProject.waitFor();
  assert.equal(await firstProject.isChecked(), false);
  await firstProject.check();
  await list.hover();
  await page.mouse.wheel(0, 10000);
  await list.getByRole("checkbox", { name: /协作项目 60/ }).waitFor();
  await page.waitForFunction(() => {
    const list = document.querySelector(".project-management-links");
    return list.scrollTop + list.clientHeight >= list.scrollHeight - 1;
  });
  assert.deepEqual(await footer.boundingBox(), footerBeforeScroll, "footer stays in place as the list scrolls");
  await saveLink.click();
  await linkDialog.waitFor({ state: "detached" });
  await page.setViewportSize({ width: 1440, height: 900 });
  await card("单位换算器").getByRole("button", { name: "本地项目协作" }).waitFor();

  await menu("单位换算器", "归档");
  const archive = page.getByRole("dialog", { name: "归档项目" });
  await archive.getByRole("button", { name: "归档项目", exact: true }).click();
  await archive.waitFor({ state: "detached" });
  await panel.getByText("已归档", { exact: true }).click();
  await menu("单位换算器", "恢复项目");
  const restore = page.getByRole("dialog", { name: "恢复项目" });
  await restore.getByRole("button", { name: "恢复项目", exact: true }).click();
  await restore.waitFor({ state: "detached" });
  await card("单位换算器").getByRole("button", { name: "打开工作区", exact: true }).click();
  await panel.waitFor({ state: "detached" });
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await nav.getByRole("button", { name: "项目管理", exact: true }).click();
  await assertCollapsed();
  await page.getByRole("button", { name: "关闭设置", exact: true }).click();
  await page.getByRole("button", { name: "新建会话", exact: true }).first().click();
  await page.getByRole("menuitem", { name: "Claude Code", exact: true }).click();
  const launcher = page.getByRole("dialog", { name: "新建会话", exact: true });
  assert.equal(await launcher.getByRole("combobox", { name: "工作目录", exact: true }).inputValue(), "fixture-project");
  await launcher.getByText("F:\\converter-moved", { exact: true }).waitFor();
  await launcher.getByRole("button", { name: "取消", exact: true }).click();
  const drawer = page.locator(".session-drawer");
  await drawer.getByRole("button", { name: "Second project", exact: true }).click();
  const secondTab = page.getByRole("tab", { name: /Second project/ });
  await secondTab.waitFor();
  assert.equal(await secondTab.getAttribute("aria-selected"), "true");
  await drawer.getByRole("button", { name: "单位换算器", exact: true }).click();
  const firstTab = page.getByRole("tab", { name: /单位换算器/ });
  await page.waitForFunction(() => [...document.querySelectorAll('[role="tab"][aria-selected="true"]')].some(tab => tab.textContent.includes("单位换算器")));
  assert.equal(await firstTab.count(), 1);
  assert.equal(await page.evaluate(() => window.projectManagementMock.states["second-project"].state), "running");
  assert.deepEqual(errors, []);
  console.log("PASS: project navigation, add, search, editing, separate services, logs, archive, and workspace directory");
} finally { await browser.close(); }
