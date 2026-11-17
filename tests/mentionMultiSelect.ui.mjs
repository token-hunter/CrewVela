// Actual composer/menu and routing with isolated IPC; never calls an Agent.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";
const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1453";
const out = ".scratch/mention-multi-select";
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ channel: "chrome", headless: true });
const checks = [], errors = [];
const fixture = "/tests/fixtures/collaboration-conversation-ui.html?empty=1&native=1&header=1";
const page = await browser.newPage({ viewport: { width: 780, height: 1000 } });
page.setDefaultTimeout(10000);
page.on("pageerror", error => errors.push(error.message));
const editor = page.getByRole("combobox", { name: "给团队发消息", exact: true });
try {
  await page.goto(base + fixture);
  await page.locator(".conversation-target").waitFor();
  const menu = page.locator(".multi-agent-mention-menu");
  await editor.fill("@");
  await menu.getByText("选择成员", { exact: true }).waitFor();
  const modes = menu.getByRole("group", { name: "成员选择模式" });
  assert.equal(await modes.getByRole("button", { name: "单选", pressed: true }).count(), 1);
  await modes.getByRole("button", { name: "多选", exact: true }).click();
  assert.equal(await modes.getByRole("button", { name: "多选", pressed: true }).count(), 1);
  assert.equal(await menu.getByRole("listbox").getAttribute("aria-multiselectable"), "true");
  const firstMember = menu.getByRole("option").first();
  await firstMember.getByText("Claude Code", { exact: true }).waitFor();
  await firstMember.click();
  await menu.getByRole("option").nth(1).click();
  assert.equal(await menu.getByRole("option", { selected: true }).count(), 2);
  assert.equal(await page.getByRole("button", { name: "发送消息", exact: true }).isEnabled(), false, "勾选未确认前不能按旧成员选择发送");
  assert.equal(await editor.inputValue(), "@", "勾选只暂存，确认前不改正文");
  await menu.getByRole("option").nth(0).click();
  await menu.getByRole("option").nth(2).click();
  assert.equal(await menu.getByRole("option", { selected: true }).count(), 2);
  await menu.getByRole("status").getByText("已选 2 位成员", { exact: true }).waitFor();
  await page.screenshot({ path: `${out}/multiple-dark.png` });
  await menu.getByRole("button", { name: "插入所选", exact: true }).click();
  assert.equal(await editor.inputValue(), "@模型验收-Codex @聊天-ZCodeCLI ");
  assert.deepEqual(await page.locator(".conversation-mention-highlight mark").allTextContents(), ["@模型验收-Codex", "@聊天-ZCodeCLI"]);
  assert.equal(await page.evaluate(() => window.conversationMock.calls.filter(c => c.command === "ma_conversation_send").length), 0);
  await editor.pressSequentially("请检查接口");
  await page.getByRole("button", { name: "发送消息", exact: true }).click();
  await page.waitForFunction(() => window.conversationMock.calls.some(c => c.command === "ma_conversation_send"));
  const input = await page.evaluate(() => window.conversationMock.calls.find(c => c.command === "ma_conversation_send").args.input);
  assert.deepEqual(input.participantAgentIds, ["codex", "kimi"]);
  checks.push("连续勾选与取消，确认一次插入、高亮，精确路由两位成员且不自动发送");

  await page.goto(base + fixture);
  await editor.fill("@");
  await menu.getByRole("button", { name: "多选", exact: true }).click();
  await editor.press(" ");
  await editor.press("ArrowDown");
  await editor.press("ArrowDown");
  await editor.press(" ");
  assert.equal(await menu.getByRole("option", { selected: true }).count(), 2);
  await editor.press("Enter");
  assert.equal(await editor.inputValue(), "@模型验收-Claude @聊天-ZCodeCLI ");
  await page.waitForFunction(() => {
    const editor = document.querySelector('.conversation-composer textarea');
    return document.activeElement === editor && editor.selectionStart === editor.value.length;
  });
  checks.push("键盘上下移动、空格勾选、Enter 批量插入，不发送消息");

  await editor.fill("@");
  await menu.getByRole("button", { name: "多选", exact: true }).click();
  await menu.getByRole("option").nth(1).click();
  await menu.getByRole("button", { name: "多选", exact: true }).click();
  await menu.getByRole("status").getByText("已选 1 位成员", { exact: true }).waitFor();
  await menu.getByRole("button", { name: "单选", exact: true }).click();
  assert.equal(await menu.getByRole("listbox").getAttribute("aria-multiselectable"), "false");
  assert.equal(await menu.getByRole("button", { name: "插入所选" }).count(), 0);
  await menu.getByRole("option").first().click();
  assert.equal(await editor.inputValue(), "@模型验收-Claude ");
  checks.push("选中模式保持勾选，切回单选可直接插入");

  await editor.fill("@");
  await menu.getByRole("button", { name: "多选", exact: true }).click();
  await menu.getByRole("button", { name: "全选", exact: true }).click();
  assert.equal(await menu.getByRole("option", { selected: true }).count(), 4);
  await menu.getByRole("button", { name: "取消全选", exact: true }).click();
  assert.equal(await menu.getByRole("option", { selected: true }).count(), 0);
  assert.equal(await menu.getByRole("button", { name: "插入所选", exact: true }).isEnabled(), false);
  await menu.getByRole("option").first().click();
  await editor.press("Escape");
  assert.equal(await editor.inputValue(), "@");
  await editor.dispatchEvent("select");
  assert.equal(await menu.count(), 0);
  checks.push("多选模式全选/清空，空选择不可确认，Esc 取消不改正文且不自动重开");

  await editor.fill("@Cod");
  await menu.getByRole("button", { name: "多选", exact: true }).click();
  await menu.getByRole("option").first().click();
  await editor.pressSequentially("不存在");
  await menu.getByText("没有匹配的成员", { exact: true }).waitFor();
  assert.equal(await menu.getByRole("button", { name: "插入所选", exact: true }).isEnabled(), true);
  await editor.press("Enter");
  assert.equal(await editor.inputValue(), "@模型验收-Codex ");
  checks.push("过滤无结果仍能确认已选成员，不会把 Enter 误当发送");

  // Filtering must retain checked identities outside the current result set.
  await editor.fill("前文\n@ 尾文");
  await editor.press("Home");
  await editor.press("ArrowRight");
  await menu.getByRole("button", { name: "多选", exact: true }).click();
  await menu.getByRole("option").nth(0).click();
  await editor.pressSequentially("Codex");
  await menu.getByRole("option").first().click();
  await menu.getByRole("button", { name: "插入所选", exact: true }).click();
  assert.equal(await editor.inputValue(), "前文\n@模型验收-Claude @模型验收-Codex  尾文");
  await page.waitForFunction(position => document.querySelector('.conversation-composer textarea').selectionStart === position, "前文\n@模型验收-Claude @模型验收-Codex ".length);
  checks.push("过滤保持已选成员身份，插入保留前后正文、换行与正确光标");

  await editor.fill("@模型验收-Claude\n@");
  await menu.getByRole("button", { name: "多选", exact: true }).click();
  await menu.getByRole("option").first().click();
  await menu.getByRole("option").nth(1).click();
  await menu.getByRole("button", { name: "插入所选", exact: true }).click();
  assert.equal(await editor.inputValue(), "@模型验收-Claude\n@模型验收-Codex ");
  checks.push("批量插入不重复已有的完整 @成员");

  await editor.fill("@");
  await menu.getByRole("button", { name: "多选", exact: true }).click();
  await menu.getByRole("option").first().click();
  await page.getByRole("combobox", { name: "协作项目", exact: true }).selectOption("B");
  await page.getByRole("combobox", { name: "协作项目", exact: true }).selectOption("A");
  await page.locator('.conversation-target').filter({hasText:'已选 4 位成员'}).waitFor();
  await editor.fill("");
  await editor.fill("@");
  await menu.getByRole("button", { name: "多选", exact: true }).click();
  assert.equal(await menu.getByRole("option", { selected: true }).count(), 0);
  checks.push("切换项目清空临时勾选，避免成员串用");

  for (const [width, theme] of [[780, "light"], [390, "dark"]]) {
    await page.setViewportSize({ width, height: 1000 });
    await page.goto(base + fixture + `&theme=${theme}`);
    await page.locator('.conversation-target').filter({hasText:'已选 4 位成员'}).waitFor();
    await editor.fill("");
    await editor.fill("@");
    await menu.getByRole("button", { name: "多选", exact: true }).click();
    await menu.getByRole("option").nth(1).click();
    const rect = await menu.boundingBox();
    assert.ok(rect.x >= 0 && rect.x + rect.width <= width && rect.y >= 0);
    assert.equal(await menu.evaluate(el => el.scrollWidth > el.clientWidth + 1), false);
    await page.screenshot({ path: `${out}/multiple-${theme}-${width}.png` });
  }
  checks.push("深浅主题及窄窗口多选控件均可见、无横向溢出");
  assert.deepEqual(errors, []);
  writeFileSync(`${out}/ui-result.json`, JSON.stringify({ checks, errors }, null, 2));
  console.log(JSON.stringify({ passed: checks.length, checks }, null, 2));
} catch (error) {
  await page.screenshot({ path: `${out}/failure.png` });
  throw error;
} finally { await browser.close(); }
