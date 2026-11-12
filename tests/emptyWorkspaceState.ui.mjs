// Run against Vite: node tests/emptyWorkspaceState.ui.mjs
// 空工作区参照 Claude Code 桌面版新会话页：像素吉祥物站在工作目录卡片上，底部是「选择 Agent」下拉。
// 勾选目录后在下拉里选一个 Agent，即在该目录开始会话；旧的标题、说明和提示都不再出现。
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
    await page.goto(`${base}/?ui-preview=1&sessions=empty`);
    const empty = page.locator(".empty-state");
    await empty.waitFor();
    const card = empty.getByRole("region", { name: "新建会话" });
    await card.waitFor();

    // 只剩吉祥物 + 卡片：旧的标题、说明、输入框式入口和底部提示都已删除。
    assert.equal(await empty.getByText("这个工作区还没有会话").count(), 0, `${size}: old heading removed`);
    assert.equal(await empty.locator("h1, p, small").count(), 0, `${size}: no leftover copy blocks`);
    const mascot = card.locator("img.ews-mascot");
    assert.equal(await mascot.evaluate((img) => img.complete && img.naturalWidth > 0), true, `${size}: mascot loads`);

    // 工作目录：项目在前、「选择其他目录…」在后，默认勾第一个项目。
    const dirs = card.getByRole("radiogroup", { name: "工作目录" });
    const radios = dirs.getByRole("radio");
    assert.ok((await radios.count()) >= 2, `${size}: at least one project plus 选择其他目录…`);
    const other = dirs.getByRole("radio", { name: "选择其他目录…", exact: true });
    assert.equal(await radios.first().isChecked(), true, `${size}: first project checked by default`);
    assert.equal(await other.isChecked(), false);
    const projectName = (await dirs.locator("label").first().innerText()).trim();

    // 卡片和吉祥物都完整落在空状态区域内。
    const area = await empty.boundingBox();
    assert.ok(inside(await card.boundingBox(), area), `${size}: card inside the empty state`);
    assert.ok(inside(await mascot.boundingBox(), area), `${size}: mascot inside the empty state`);

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

    if (width === 1280) {
      // 勾选随点击移动（单选）。
      await dirs.getByText("选择其他目录…", { exact: true }).click();
      assert.equal(await other.isChecked(), true);
      assert.equal(await radios.first().isChecked(), false);
      await dirs.locator("label").first().click();
      assert.equal(await radios.first().isChecked(), true);

      // Claude Code 还要选 Provider：直接进入弹窗的 Provider 步骤，不再让人重新选 Agent。
      await pill.click();
      await popover.getByRole("button", { name: "Claude Code", exact: true }).click();
      const providerStep = page.getByRole("heading", { name: "选择 Provider", exact: true });
      await providerStep.waitFor();
      await page.keyboard.press("Escape");
      await providerStep.waitFor({ state: "detached" });

      // 「管理 Agent」打开设置里管理 Agent 启用状态的页面。
      await pill.click();
      await popover.getByRole("button", { name: "管理 Agent", exact: true }).click();
      await page.getByRole("heading", { name: "模型设置", exact: true }).waitFor();
      await page.getByRole("button", { name: "关闭设置", exact: true }).click();

      // 上面的 Provider 起步只影响那一次：从侧栏再打开新建弹窗，仍从「选择 Agent」开始；
      // 弹窗里点 Agent 卡片走的是与空状态共用的启动链路。
      await page.getByRole("button", { name: "新建会话", exact: true }).click();
      await page.getByRole("menu", { name: "新建 Runtime 会话" }).getByRole("menuitem").filter({ hasText: "Claude Code" }).first().click();
      await page.getByRole("heading", { name: "选择 Agent", exact: true }).waitFor();
      await page.locator(".new-session-agent-choice", { hasText: "DeepSeek Harness" }).click();
      await page.locator(".harness-chat").waitFor();
      await page.getByRole("heading", { name: "选择 Agent", exact: true }).waitFor({ state: "detached" });
      // 侧栏入口没带项目：不能沿用刚才在空状态勾选、又取消掉的那个目录。
      await page.getByText("DeepSeek Harness", { exact: true }).first().waitFor();
      assert.equal(await page.getByText(`DeepSeek Harness · ${projectName}`).count(), 0, "sidebar launch must not reuse the empty-state project");
    } else {
      // 回车启动第一个匹配的 Agent：在勾选的项目里打开 DeepSeek Harness 原生会话。
      await pill.click();
      await popover.waitFor();
      await page.keyboard.type("deep");
      await page.keyboard.press("Enter");
      await page.locator(".harness-chat").waitFor();
      await page.getByText(`DeepSeek Harness · ${projectName}`).first().waitFor();
    }
    assert.deepEqual(pageErrors.filter((message) => !/transformCallback|__TAURI/.test(message)), []);
    await page.close();
  }
  console.log("PASS: empty workspace shows the mascot card, picks a directory, filters and launches an Agent there");
} finally {
  await browser.close();
}
