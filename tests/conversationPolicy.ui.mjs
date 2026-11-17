// User superseded the restricted-chat design: preserve native CLI abilities.
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { chromium } from "playwright";
const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome" });
try {
 const page = await browser.newPage({ viewport: { width: 760, height: 1000 } });
 const errors = []; page.on("pageerror", error => errors.push(error.message));
 await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?empty=1&natural=1&header=1&native=1`);
 await page.locator(".conversation-target").click();
 const members = page.locator(".conversation-recipient-menu"); await members.waitFor();
 assert.equal(await members.getByRole("checkbox").count(), 4);
 for (const check of await members.getByRole("checkbox").all()) {
  assert.equal(await check.isEnabled(), true, "all native members must remain selectable");
  assert.equal(await check.isChecked(), true);
 }
 await members.getByRole("button", { name: "全选 / 取消全选" }).click();
 for (const check of await members.getByRole("checkbox").all()) assert.equal(await check.isChecked(), false);
 await members.getByRole("button", { name: "全选 / 取消全选" }).click();
 mkdirSync("output/playwright", { recursive: true });
 await page.screenshot({ path: "output/playwright/native-group-all-members.png" });
 await page.locator(".conversation-target").click();
 await page.getByRole("combobox", { name: "给团队发消息", exact: true }).fill("你们分别介绍一下擅长什么");
 await page.getByRole("button", { name: "发送消息", exact: true }).click();
 await page.waitForFunction(() => window.conversationMock.calls.some(call => call.command === "ma_conversation_send"));
 const sent = await page.evaluate(() => window.conversationMock.calls.find(call => call.command === "ma_conversation_send").args.input);
 assert.equal(sent.nativeCapabilities, true);
 assert.deepEqual(sent.participantAgentIds.sort(), ["claude", "codex", "kimi", "zcode-review"].sort());
 assert.equal(await page.locator(".conversation-policy-legacy-note").count(), 0);
 await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?empty=1&natural=1&header=1&native=1&disabled=zcode_cli`);
 await page.locator(".conversation-target").click();
 const unavailable = page.locator(".conversation-recipient-menu label").filter({ hasText: "聊天-ZCodeCLI" });
 assert.equal(await unavailable.getByRole("checkbox").isDisabled(), true, "explicitly disabled runtime stays disabled");
 await page.goto(`${base}/tests/fixtures/collaboration-member-dialog-ui.html?state=policy`);
 await page.locator(".collab-agent-card").first().click();
 const editor = page.getByRole("dialog", { name: "编辑团队成员", exact: true });
 await editor.getByText("角色设定只说明专长", { exact: false }).waitFor();
 assert.equal(await editor.getByRole("button", { name: "工具与长期限制", exact: true }).count(), 0);
 await editor.getByLabel("职责定位", { exact: true }).fill("负责后端接口设计，关注兼容性与错误处理。");
 await editor.getByRole("button", { name: "保存成员", exact: true }).click();
 await editor.waitFor({ state: "detached" }); await page.locator(".collab-agent-card").first().click();
 assert.equal(await editor.getByLabel("职责定位", { exact: true }).inputValue(), "负责后端接口设计，关注兼容性与错误处理。");
 assert.deepEqual(errors, []);
 console.log("PASS native group: all members selectable, select-all, native payload, explicit disable retained, role editing without capability gating");
} finally { await browser.close(); }
