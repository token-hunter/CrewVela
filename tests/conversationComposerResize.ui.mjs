// Production composer and pointer events, isolated IPC; never starts an Agent.
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { chromium, webkit } from "playwright";
const engine = process.env.CREWVELA_BROWSER === "webkit" ? webkit : chromium;
const browser = await engine.launch(engine === chromium ? { channel: process.env.CREWVELA_BROWSER_CHANNEL || "chrome" } : {});
const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
try {
  const page = await browser.newPage({ viewport: { width: 900, height: 1050 } });
  page.setDefaultTimeout(10000);
  const errors = []; page.on("pageerror", error => errors.push(error.message));
  await page.goto(`${base}/tests/fixtures/collaboration-conversation-ui.html?empty=1&natural=1`);
  const form = page.locator(".resizable-conversation-composer");
  await form.waitFor();
  const box = () => form.boundingBox();
  const handle = edge => form.locator(`.conversation-composer-resizer[data-edge="${edge}"]`);
  const closeTo = (actual, expected, message) => assert.ok(Math.abs(actual - expected) < 3, `${message}: ${actual} vs ${expected}`);
  const drag = async (edge, dx, dy) => {
    const r = await handle(edge).boundingBox();
    const x = r.x + r.width / 2, y = r.y + r.height / 2;
    await page.mouse.move(x, y); await page.mouse.down();
    await page.mouse.move(x + dx, y + dy, { steps: 12 }); await page.mouse.up();
    await page.waitForFunction(() => document.querySelector(".resizable-conversation-composer")?.dataset.resizing === "false");
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  };
  const contained = async () => {
    const f = await box(), panel = await page.locator(".team-chat-panel").boundingBox();
    assert.ok(f.x >= panel.x - 1 && f.x + f.width <= panel.x + panel.width + 1, "form stays within panel width");
    assert.ok(f.y >= panel.y - 1 && f.y + f.height <= panel.y + panel.height + 1, "form stays within panel height");
    for (const selector of [".conversation-editor", ".conversation-composer-actions", ".conversation-send", ".conversation-target"]) {
      const r = await form.locator(selector).boundingBox();
      assert.ok(r.x >= f.x - 1 && r.x + r.width <= f.x + f.width + 1, `${selector} stays inside resized form horizontally`);
      assert.ok(r.y >= f.y - 1 && r.y + r.height <= f.y + f.height + 1, `${selector} stays inside resized form vertically`);
    }
    assert.equal(await form.locator("textarea").evaluate(el => getComputedStyle(el).resize), "none", "native diagonal resize glyph removed");
  };
  await contained();
  const initial = await box();
  const textarea = page.getByRole("combobox", { name: "给团队发消息", exact: true });
  await textarea.fill("保留拖拽中的草稿");
  await drag("n", 0, -140);
  let before = await box();
  assert.ok(before.height >= initial.height + 130, "top edge expands entire composer");
  closeTo(before.y + before.height, initial.y + initial.height, "top resize anchors bottom");
  await drag("e", -180, 0);
  let after = await box();
  closeTo(after.x, before.x, "right edge anchors left");
  assert.ok(after.width < before.width - 170);
  before = after;
  await drag("w", 60, 0);
  after = await box();
  closeTo(after.x + after.width, before.x + before.width, "left edge anchors right");
  before = after;
  await drag("s", 0, -45);
  after = await box();
  closeTo(after.y, before.y, "bottom edge anchors top");
  assert.ok(after.height < before.height - 35);
  for (const [edge, dx, dy] of [["nw", -25, -25], ["ne", 25, -25], ["sw", -15, 15], ["se", 15, 15]]) {
    before = await box(); await drag(edge, dx, dy); after = await box();
    assert.ok(after.width > before.width + 8 && after.height > before.height + 8, `${edge} resizes both dimensions`);
    await contained();
  }
  assert.equal(await textarea.inputValue(), "保留拖拽中的草稿", "resizing retains text");
  assert.equal(await page.evaluate(() => window.conversationMock.calls.filter(call => call.command === "ma_conversation_send").length), 0, "resize never submits");

  await drag("nw", -1800, -1800); await contained();
  const large = await box();
  assert.ok(large.height > initial.height * 2 && large.width <= initial.width + 1);
  await drag("se", -1800, -1800); await contained();
  const small = await box();
  assert.ok(small.width >= 279 && small.height >= 90, "minimum dimensions preserve controls");
  await handle("n").dblclick();
  await page.waitForFunction(() => document.querySelector(".resizable-conversation-composer")?.dataset.sized === "false");
  closeTo((await box()).height, initial.height, "double click restores default height");
  await handle("nw").focus(); await handle("nw").press("ArrowUp");
  assert.ok((await box()).height > initial.height + 5, "keyboard resize works");
  await handle("nw").press("Home");
  closeTo((await box()).width, initial.width, "Home restores width");

  // Pointer capture cleans up when focus leaves the window during a drag.
  const grip = await handle("n").boundingBox();
  await page.mouse.move(grip.x + 20, grip.y + 4); await page.mouse.down();
  await page.mouse.move(grip.x + 20, grip.y - 100, { steps: 5 });
  await page.evaluate(() => window.dispatchEvent(new Event("blur"))); await page.mouse.up();
  assert.equal(await form.getAttribute("data-resizing"), "false");
  assert.equal(await page.evaluate(() => document.body.style.userSelect), "");

  await drag("nw", -100, -120);
  await page.setViewportSize({ width: 360, height: 760 });
  await page.waitForTimeout(100); await contained();
  await form.locator(".conversation-target").click();
  await page.locator(".conversation-recipient-menu").waitFor();
  await page.locator(".conversation-target").click();
  await form.locator("textarea").fill("hi");
  await page.getByRole("button", { name: "发送消息", exact: true }).click();
  await page.waitForFunction(() => window.conversationMock.calls.some(call => call.command === "ma_conversation_send"));
  mkdirSync("output/playwright", { recursive: true });
  await page.screenshot({ path: "output/playwright/conversation-composer-resize-narrow.png" });
  await page.setViewportSize({ width: 1100, height: 1000 });
  await page.locator(".conversation-composer-resizer[data-edge='n']").dblclick();
  await page.screenshot({ path: "output/playwright/conversation-composer-resize-default.png" });
  const toolbar = () => form.evaluate(node => {
    const bounds = selector => node.querySelector(selector).getBoundingClientRect();
    const centerY = rect => rect.top + rect.height / 2;
    const target = bounds(".conversation-target"), attachment = bounds(".conversation-add-attachment");
    const layout = bounds(".conversation-composer-actions"), send = bounds(".conversation-send");
    return {
      planningY: [...node.querySelectorAll(".team-generate-plan")].map(button => centerY(button.getBoundingClientRect())),
      targetY: centerY(target), sendY: centerY(send), inset: attachment.left - layout.left, right: layout.right - send.right,
    };
  });
  let actions = await toolbar();
  closeTo(actions.targetY, actions.sendY, "wide composer keeps one toolbar row");
  // Narrow the form while the panel and the window both remain wide.
  await drag("e", 340 - (await box()).width, 0);
  actions = await toolbar();
  await contained();
  closeTo(actions.planningY[0], actions.planningY[1], "planning buttons stay together");
  assert.ok(actions.sendY > actions.planningY[1] + 20, "send controls move to a separate row");
  closeTo(actions.inset, 0, "attachment anchors to the left edge");
  closeTo(actions.right, 0, "send anchors to the right edge");
  assert.equal(await form.locator(".conversation-target span").innerText(), "已选 3 位");
  await form.getByRole("button", { name: /^分工与托管 · 已选 \d+ 位$/ }).click();
  await form.locator(".conversation-recipient-menu").waitFor();
  assert.deepEqual(errors, []);
  console.log("PASS composer resize: 8 edges/corners, native glyph removed, opposite-edge anchors, panel/minimum bounds, keyboard/reset, blur cleanup, draft retention, menus/send, narrow viewport");
} finally { await browser.close(); }
