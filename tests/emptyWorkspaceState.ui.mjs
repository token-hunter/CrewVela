// Run against Vite: node tests/emptyWorkspaceState.ui.mjs
// 空工作区保留工作目录卡片与「选择 Agent」下拉，不展示吉祥物。
// 勾选目录后在下拉里选一个 Agent，在新建弹窗确认该目录后开始会话。
import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const inside = (inner, outer) =>
  inner.x >= outer.x - 1 &&
  inner.y >= outer.y - 1 &&
  inner.x + inner.width <= outer.x + outer.width + 1 &&
  inner.y + inner.height <= outer.y + outer.height + 1;
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  for (const [width, height] of [[1280, 800], [820, 540]]) {
    const size = `${width}x${height}`;
    const page = await browser.newPage({ viewport: { width, height } });
    const pageErrors = [];
    page.on("pageerror", (error) => pageErrors.push(error.message));
    await page.goto(`${base}${process.env.CREWVELA_UI_ENTRY || "/"}?ui-preview=1&sessions=empty`);
    const empty = page.locator(".empty-state");
    await empty.waitFor();
    const card = empty.getByRole("region", { name: "新建会话" });
    await card.waitFor();

    // 只保留目录卡片，移除吉祥物并收回其顶部空间。
    assert.equal(await empty.getByText("这个工作区还没有会话").count(), 0, `${size}: old heading removed`);
    assert.equal(await empty.locator("h1, p, small").count(), 0, `${size}: no leftover copy blocks`);
    assert.equal(await card.locator("img.ews-mascot").count(), 0, `${size}: empty workspace mascot removed`);
    const topPadding = await empty.evaluate(element => parseFloat(getComputedStyle(element).paddingTop));
    assert.ok(Math.abs(topPadding - Math.min(188, Math.max(24, height * 0.26 - 52))) < 1, `${size}: mascot headroom reclaimed`);

    // 工作目录：默认勾第一个项目，末尾按钮立即打开目录选择。
    const dirs = card.getByRole("radiogroup", { name: "工作目录" });
    const radios = dirs.getByRole("radio");
    assert.ok((await radios.count()) >= 1, `${size}: at least one project`);
    const other = dirs.getByRole("button", { name: "选择其他目录…", exact: true });
    assert.equal(await radios.first().isChecked(), true, `${size}: first project checked by default`);
    assert.equal(await other.isEnabled(), true);
    const projectName = (await dirs.locator("label").first().innerText()).trim();

    // 目录卡片完整落在空状态区域内。
    const area = await empty.boundingBox();
    assert.ok(inside(await card.boundingBox(), area), `${size}: card inside the empty state`);

    // 「选择 Agent」下拉：聚焦搜索框，分组列出 Agent，并完整落在空状态区域内。
    const pill = card.getByRole("button", { name: "选择 Agent", exact: true });
    const popover = card.getByRole("dialog", { name: "选择 Agent" });
    await pill.click();
    await popover.waitFor();
    const search = popover.getByRole("textbox", { name: "搜索 Agent" });
    assert.equal(await search.evaluate((element) => element === document.activeElement), true, `${size}: search focused`);
    // 焦点不在输入框外再套一个框，而是整行下边线变色。
    assert.equal(await search.evaluate((element) => getComputedStyle(element).outlineStyle), "none", `${size}: no nested focus ring`);
    assert.notEqual(
      await popover.locator(".ews-search").evaluate((element) => getComputedStyle(element).boxShadow),
      "none",
      `${size}: search row shows focus`,
    );
    await popover.getByRole("group", { name: "CLI 终端" }).waitFor();
    await popover.getByRole("group", { name: "原生聊天" }).waitFor();
    assert.ok(inside(await popover.boundingBox(), area), `${size}: popover fits inside the empty state`);

    // 搜索过滤；Esc 关闭并把焦点还给按钮。
    await page.keyboard.type("deep");
    assert.deepEqual(await popover.locator("[data-agent-option]").allInnerTexts(), ["DeepSeek Harness"]);
    await page.keyboard.press("Escape");
    await popover.waitFor({ state: "detached" });
    assert.equal(await pill.evaluate((element) => element === document.activeElement), true, `${size}: focus back on the pill`);

    const newSession = page.getByRole("button", { name: "新建会话", exact: true });
    const runtimeMenu = page.getByRole("menu", { name: "新建 Runtime 会话", exact: true });
    const clickNewSession = async () => {
      const bounds = await newSession.boundingBox();
      await page.mouse.click(bounds.x + 80, bounds.y + bounds.height / 2);
    };
    const moreTools = page.getByRole("button", { name: "更多工具", exact: true });
    const toolsMenu = page.getByRole("menu", { name: "更多工具", exact: true });
    const openRightMenu = page.getByRole("button", { name: "新建工作面板标签", exact: true });
    const rightMenu = page.getByRole("menu", { name: "新建标签", exact: true });
    if (!await openRightMenu.isVisible()) {
      await page.getByRole("button", { name: "切换右侧栏", exact: true }).click();
    }
    for (const [open, menu] of [[openRightMenu, rightMenu], [moreTools, toolsMenu]]) {
      await open.click();
      await menu.waitFor();
      await clickNewSession();
      await runtimeMenu.waitFor({ timeout: 2000 });
      assert.equal(await menu.isVisible(), false, `${size}: a single click switches menus`);
      await page.keyboard.press("Escape");
      await runtimeMenu.waitFor({ state: "detached" });
      assert.equal(await newSession.evaluate((element) => element === document.activeElement), true);
    }
    await clickNewSession();
    await runtimeMenu.waitFor();
    await newSession.click({ timeout: 2000 });
    await runtimeMenu.waitFor({ state: "detached" });

    if (width === 1280) {
      // 取消目录选择保留当前项目。入口为按钮，选完才改变单选项。
      await page.evaluate(() => {
        (window).__TAURI_INTERNALS__ = { invoke: async () => null };
      });
      await other.click();
      await page.waitForFunction(() => !document.querySelector(".ews-row.is-other")?.disabled);
      assert.equal(await radios.first().isChecked(), true);
      await page.evaluate(() => { delete window.__TAURI_INTERNALS__; });

      // CLI 统一进入本次启动模式设置；取消后保留已选工作目录。
      await pill.click();
      await popover.getByRole("button", { name: "Claude Code", exact: true }).click();
      const launchStep = page.getByRole("dialog", { name: "新建会话", exact: true });
      await launchStep.waitFor();
      assert.equal(await launchStep.getByRole("combobox", { name: "启动 Agent" }).inputValue(), "claude_code");
      assert.equal(await launchStep.getByRole("combobox", { name: "本次启动模式" }).inputValue(), "cli_default");
      await page.keyboard.press("Escape");
      await launchStep.waitFor({ state: "detached" });
      assert.equal(await radios.first().isChecked(), true);

      // 「管理 Agent」直接打开已有管理弹窗，关闭后返回 Agent 列表。
      await pill.click();
      await popover.getByRole("button", { name: "管理 Agent", exact: true }).click();
      const management = page.getByRole("dialog", { name: "管理 Runtime", exact: true });
      await management.waitFor();
      await management.getByRole("button", { name: "关闭", exact: true }).click();
      await popover.waitFor();
      await page.keyboard.press("Escape");

      // 从侧栏打开新会话不受上一次启动设置影响。
      await newSession.click();
      await runtimeMenu.getByRole("menuitem").filter({ hasText: "Claude Code" }).first().click();
      const directLaunch = page.getByRole("dialog", { name: "新建会话", exact: true });
      await directLaunch.waitFor();
      assert.equal(await directLaunch.getByRole("combobox", { name: "启动 Agent" }).inputValue(), "claude_code");
      // 新流程明确展示当前目录，由用户在同一弹窗确认；切换 Agent 保留目录。
      assert.ok(await directLaunch.locator(".cli-launch-directory-path").innerText());
      await directLaunch.getByRole("combobox", { name: "启动 Agent" }).selectOption("deepseek_harness");
      await directLaunch.getByRole("button", { name: "启动", exact: true }).click();
      await page.locator(".harness-chat").waitFor();
      await directLaunch.waitFor({ state: "detached" });
      await page.getByText(`DeepSeek Harness · ${projectName}`, { exact: true }).first().waitFor();
    } else {
      // 回车选择第一个匹配的 Agent，再确认启动到勾选的项目。
      await pill.click();
      await popover.waitFor();
      await page.keyboard.type("deep");
      await page.keyboard.press("Enter");
      await page.getByRole("dialog", { name: "新建会话", exact: true }).getByRole("button", { name: "启动", exact: true }).click();
      await page.locator(".harness-chat").waitFor();
      await page.getByText(`DeepSeek Harness · ${projectName}`).first().waitFor();
    }
    assert.deepEqual(pageErrors.filter((message) => !/transformCallback|__TAURI/.test(message)), []);
    await page.close();
  }
  console.log("PASS: empty workspace shows the directory card without a mascot, picks a directory, filters and launches an Agent there");
} finally {
  await browser.close();
}
