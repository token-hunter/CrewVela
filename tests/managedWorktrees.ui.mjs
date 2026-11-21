import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const agents = new Map([
  ["claude_code", "launch_runtime_session"],
  ["codex", "launch_runtime_session"],
  ["kimi_code", "launch_runtime_session"],
  ["zcode_cli", "launch_runtime_session"],
  ["opencode", "launch_runtime_session"],
  ["pi", "launch_runtime_session"],
  ["claude_chat", "create_claude_chat_session"],
  ["codex_chat", "create_codex_chat_session"],
  ["kimi_chat", "create_kimi_chat_session"],
  ["zcode", "create_zcode_session"],
  ["opencode_chat", "create_opencode_chat_session"],
  ["pi_chat", "create_pi_chat_session"],
  ["deepseek_harness", "create_deepseek_harness_session"],
]);
const browser = await chromium.launch({ channel: "chrome", headless: true });

try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.setDefaultTimeout(12000);
  await page.goto(new URL("/tests/fixtures/app-session-async-ui.html?cli-launch=1", base).href, {
    waitUntil: "domcontentloaded", timeout: 60000,
  });
  await page.waitForFunction(() => window.appSessionMock);
  await page.evaluate(() => {
    const mock = window.appSessionMock;
    const invoke = mock.invoke.bind(mock);
    mock.invoke = async (command, args) => {
      if (command === "worktree_branches") {
        return args.cwd === "D:\\Second 项目" ? ["refs/heads/secondary"] : ["refs/heads/main", "refs/remotes/origin/release"];
      }
      return invoke(command, args);
    };
  });

  async function openLauncher(kind) {
    await page.getByRole("button", { name: "新建会话", exact: true }).first().click();
    await page.getByRole("menu", { name: "新建 Runtime 会话" }).getByRole("menuitem", { name: "Claude Code", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "新建会话", exact: true });
    await dialog.getByRole("combobox", { name: "启动 Agent" }).selectOption(kind);
    await dialog.getByRole("combobox", { name: "工作目录", exact: true }).selectOption("fixture-project");
    return dialog;
  }

  async function start(dialog) {
    await dialog.getByRole("button", { name: "启动", exact: true }).click();
    await dialog.waitFor({ state: "detached" });
    return page.evaluate(() => window.appSessionMock.cliLaunch.calls.at(-1));
  }

  const ordinary = await openLauncher("codex");
  assert.equal(await ordinary.getByRole("checkbox", { name: "使用独立工作树" }).isChecked(), false);
  await ordinary.getByRole("combobox", { name: "本次启动模式" }).selectOption("workspace");
  const normal = await start(ordinary);
  assert.equal(normal.command, "launch_runtime_session");
  assert.equal(normal.args.launchMode, "workspace");
  assert.equal(normal.args.cwd, "C:\\fixture");
  assert.equal(normal.args.worktreeOptions ?? null, null);

  for (const [kind, command] of agents) {
    const dialog = await openLauncher(kind);
    await dialog.getByRole("checkbox", { name: "使用独立工作树" }).check();
    const branch = dialog.getByRole("combobox", { name: "基准分支" });
    await branch.selectOption("refs/remotes/origin/release");
    await dialog.getByRole("checkbox", { name: "创建前获取上游更新" }).check();
    const started = await start(dialog);
    assert.equal(started.command, command, kind);
    if (command === "launch_runtime_session") assert.equal(started.args.runtimeKind, kind);
    assert.equal(started.args.projectId, "fixture-project", kind);
    assert.equal(started.args.cwd, "C:\\fixture", kind);
    assert.deepEqual(started.args.worktreeOptions, { baseRef: "refs/remotes/origin/release", fetchUpstream: true }, kind);
  }

  await page.setViewportSize({ width: 390, height: 800 });
  const narrow = await openLauncher("pi_chat");
  await narrow.getByRole("checkbox", { name: "使用独立工作树" }).check();
  const branch = narrow.getByRole("combobox", { name: "基准分支" });
  await branch.selectOption("refs/heads/main");
  await narrow.getByRole("combobox", { name: "工作目录", exact: true }).selectOption("second-project");
  await branch.selectOption("refs/heads/secondary");
  assert.equal(await branch.locator('option[value="refs/heads/main"]').count(), 0);
  assert.equal(await narrow.evaluate(dialog => dialog.scrollWidth > dialog.clientWidth), false);
  const secondProject = await start(narrow);
  assert.equal(secondProject.args.projectId, "second-project");
  assert.equal(secondProject.args.cwd, "D:\\Second 项目");
  assert.deepEqual(secondProject.args.worktreeOptions, { baseRef: "refs/heads/secondary", fetchUpstream: false });

  await page.setViewportSize({ width: 1440, height: 900 });
  await page.evaluate(() => {
    const mock = window.appSessionMock;
    const invoke = mock.invoke.bind(mock);
    const entry = {
      sessionId: "archived", title: "已归档的 Pi Chat", project: "Fixture project", repoRoot: "C:\\fixture",
      path: "F:\\managed\\archived", cwd: "F:\\managed\\archived\\app", branch: "crewvela/session-archived",
      baseRef: "refs/heads/main", runtime: "pi_chat", archived: true, present: true, blocked: null,
    };
    const busy = { ...entry, sessionId: "busy", title: "使用中的任务", archived: false, blocked: "该工作树仍有会话使用" };
    const state = window.worktreePageMock = {
      root: "F:\\managed", entries: [busy, entry], calls: [], policy: { command: "", files: [], autoCleanup: false },
      run: null, snapshots: [], preparationAttempts: 0,
    };
    mock.cliLaunch.pickerValue = "F:\\new-managed-root";
    mock.invoke = async (command, args) => {
      if (!command.startsWith("worktree_")) return invoke(command, args);
      state.calls.push({ command, args });
      if (command === "worktree_overview") return structuredClone({ root: state.root, entries: state.entries });
      if (command === "worktree_set_root") return (state.root = args.root);
      if (command === "worktree_lifecycle_get") return structuredClone({ policy: state.policy, run: state.run, snapshots: state.snapshots, warning: null });
      if (command === "worktree_lifecycle_save") { state.policy = args.policy; return; }
      if (command === "worktree_lifecycle_prepare") {
        state.preparationAttempts++;
        const first = state.preparationAttempts === 1;
        state.run = { state: first ? "failed" : "ready", updatedAtMs: Date.now(), lines: [first ? "fixture preparation failed" : "fixture preparation succeeded"] };
        if (first) throw Error("fixture preparation failed");
        return;
      }
      if (command === "worktree_clean") {
        entry.present = false;
        state.snapshots.push({ id: "snapshot", sessionId: entry.sessionId, branch: entry.branch, path: entry.path, commit: "a".repeat(40), gitRef: "refs/crewvela/snapshots/snapshot", createdAtMs: Date.now() });
        return;
      }
      if (command === "worktree_lifecycle_restore") { entry.present = true; return; }
      throw Error(`Unexpected Worktree command: ${command}`);
    };
  });
  await page.getByRole("button", { name: "设置", exact: true }).click();
  const nav = page.locator(".settings-nav");
  await nav.getByRole("button", { name: "Worktrees", exact: true }).click();
  await page.getByRole("heading", { name: "Worktrees", exact: true }).waitFor();
  const settings = page.locator(".worktree-settings");
  await settings.getByRole("heading", { name: "工作树根目录" }).waitFor();
  const branchIcons = settings.locator("article > strong > svg");
  for (const [theme, accent, color] of [
    ["深色", "紫色", "rgb(155, 122, 240)"],
    ["深色", "青色", "rgb(62, 186, 198)"],
    ["浅色", "无强调色", "rgb(69, 69, 67)"],
  ]) {
    await nav.getByRole("button", { name: "外观", exact: true }).click();
    await page.getByRole("button", { name: theme, exact: true }).click();
    await page.getByRole("radio", { name: accent, exact: true }).click();
    await nav.getByRole("button", { name: "Worktrees", exact: true }).click();
    await branchIcons.first().waitFor();
    assert.deepEqual(await branchIcons.evaluateAll(icons => icons.map(icon => getComputedStyle(icon).color)), [color, color]);
  }
  await settings.getByRole("button", { name: "更改根目录" }).click();
  await settings.getByText("F:\\new-managed-root", { exact: true }).waitFor();
  const busyEntry = settings.locator("article", { hasText: "使用中的任务" });
  assert.equal(await busyEntry.getByRole("button", { name: "清理工作树" }).isDisabled(), true);
  const archivedEntry = settings.locator("article", { hasText: "已归档的 Pi Chat" });
  await archivedEntry.getByRole("button", { name: "管理环境与恢复" }).click();
  await settings.getByText("环境准备与恢复", { exact: true }).click();
  await settings.getByRole("textbox", { name: "准备命令" }).fill("npm ci");
  await settings.getByRole("textbox", { name: "所需文件" }).fill("dev.example.json");
  assert.equal(await settings.getByRole("checkbox", { name: "归档时自动回收" }).isChecked(), false);
  await settings.getByRole("button", { name: "保存项目配置" }).click();
  await settings.getByText("已保存此项目的配置", { exact: true }).waitFor();
  await settings.getByRole("button", { name: "执行准备" }).click();
  await settings.getByRole("alert").filter({ hasText: "fixture preparation failed" }).waitFor();
  await settings.getByRole("button", { name: "重试准备" }).click();
  await settings.getByText("环境准备完成", { exact: true }).waitFor();
  await archivedEntry.getByRole("button", { name: "清理工作树" }).click();
  await archivedEntry.getByRole("button", { name: "确认清理" }).click();
  await settings.getByText(/已保存恢复快照并清理工作树/).waitFor();
  assert.equal(await archivedEntry.getByRole("button", { name: "打开目录" }).isDisabled(), true);
  await settings.getByText(/恢复快照（1）/).click();
  await settings.getByRole("button", { name: "恢复到原路径" }).click();
  await settings.getByText("已恢复到原 Worktree 路径", { exact: true }).waitFor();
  assert.equal(await archivedEntry.getByRole("button", { name: "打开目录" }).isEnabled(), true);
  const calls = await page.evaluate(() => window.worktreePageMock.calls);
  assert.deepEqual(calls.find(call => call.command === "worktree_lifecycle_save").args, {
    repoRoot: "C:\\fixture", policy: { command: "npm ci", files: ["dev.example.json"], autoCleanup: false },
  });
  assert.deepEqual(calls.filter(call => call.command === "worktree_lifecycle_prepare").map(call => call.args), [{ sessionId: "archived" }, { sessionId: "archived" }]);
  assert.deepEqual(calls.filter(call => call.command === "worktree_clean").map(call => call.args), [{ sessionId: "archived" }]);
  assert.deepEqual(errors, []);
  console.log("PASS: all 13 CLI and Chat launch requests, branch switching, root selection, preparation retry, protected cleanup and restoration");
} finally {
  await browser.close();
}
