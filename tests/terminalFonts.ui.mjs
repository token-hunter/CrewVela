import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1461";
const url = new URL("/tests/fixtures/terminal-init-ui.html?fonts=1", base);
const out = ".scratch/terminal-font-picker";
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({
  channel: "chrome",
  ignoreDefaultArgs: ["--hide-scrollbars"],
  args: ["--disable-features=OverlayScrollbar"],
});
const errors = [];
async function open(width) {
  const page = await browser.newPage({ viewport: { width, height: 850 } });
  page.setDefaultTimeout(10000);
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(url.href);
  await page.waitForFunction(() => window.terminalMock?.terminal?.options.fontFamily.includes('"Consolas"'));
  await page.locator("#general-terminal-font:enabled").waitFor();
  return page;
}

try {
  const page = await open(1000);
  const font = page.locator("#general-terminal-font");
  const size = page.locator("#general-terminal-font-size");
  const browse = page.getByRole("button", { name: "本机字体", exact: true });
  const search = page.getByRole("searchbox", { name: "搜索本机字体" });
  const options = page.getByRole("group", { name: "可选字体" });
  const dialog = page.getByRole("dialog", { name: "设置", exact: true });
  assert.equal(await page.evaluate(() => window.terminalMock.fontReads), 0);
  assert.equal(await font.inputValue(), "Consolas, monospace");
  await browse.click();
  await options.getByRole("button", { name: "Courier New 等宽" }).waitFor();
  assert.deepEqual((await options.getByRole("button").allTextContents()).slice(0, 3), ["默认字体", "Consolas等宽", "Courier New等宽"]);

  const scrollbarStyle = await page.addStyleTag({ content: `
    .terminal-font-options { max-height: 80px; }
    .terminal-font-options::-webkit-scrollbar { width: 16px; }
    .terminal-font-options::-webkit-scrollbar-thumb { background: #aaa; }
  ` });
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const scrollbar = await options.evaluate(element => {
    const rect = element.getBoundingClientRect();
    const gutter = element.offsetWidth - element.clientWidth;
    return { x: rect.right - gutter / 2, top: rect.top, bottom: rect.bottom, gutter };
  });
  assert.ok(scrollbar.gutter > 0, "the test uses a real scrollbar");
  await page.mouse.move(scrollbar.x, scrollbar.bottom - 6);
  await page.mouse.down();
  try {
    assert.equal(await options.count(), 1, "pressing the scrollbar track keeps the picker open");
    await page.waitForFunction(() => document.querySelector(".terminal-font-options")?.scrollTop > 0);
  } finally {
    await page.mouse.up();
  }
  await search.focus();
  await options.evaluate(element => { element.scrollTop = 0; });
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await page.mouse.move(scrollbar.x, scrollbar.top + 6);
  await page.mouse.down();
  await page.mouse.move(scrollbar.x, scrollbar.top + 29, { steps: 4 });
  await page.mouse.up();
  assert.equal(await options.count(), 1, "dragging the scrollbar keeps the picker open");
  await page.waitForFunction(() => document.querySelector(".terminal-font-options")?.scrollTop > 0);
  assert.equal(await font.inputValue(), "Consolas, monospace");
  await page.keyboard.press("Escape");
  assert.equal(await options.count(), 0);
  assert.equal(await dialog.isVisible(), true);
  await scrollbarStyle.evaluate(element => element.remove());
  await browse.click();

  await search.fill("微软雅黑");
  assert.equal(await options.getByRole("button", { name: "Microsoft YaHei" }).count(), 1);
  assert.equal(await options.getByRole("button", { name: "Consolas 等宽" }).count(), 0);
  await search.press("Escape");
  assert.equal(await dialog.isVisible(), true);
  assert.equal(await options.count(), 0);
  await browse.click();
  await size.focus();
  assert.equal(await options.count(), 0, "moving focus outside still closes the picker");
  await browse.click();
  await dialog.getByText("预览", { exact: true }).click();
  assert.equal(await options.count(), 0, "clicking outside still closes the picker");

  await font.fill("找不到的字体, Consolas");
  await size.selectOption("20");
  await page.getByRole("status").filter({ hasText: /本机列表中未找到 找不到的字体/ }).waitFor();
  await page.waitForFunction(() => window.terminalMock.preview?.options.fontSize === 20);
  assert.equal(await page.evaluate(() => window.terminalMock.terminal.options.fontSize), 12);
  await browse.click();
  await search.fill("courier");
  assert.equal(await page.evaluate(() => window.terminalMock.fontReads), 1);
  assert.equal(await page.evaluate(() => {
    const menu = document.querySelector(".terminal-font-catalog").getBoundingClientRect();
    const settings = document.querySelector(".settings-content").getBoundingClientRect();
    return menu.top >= settings.top && menu.bottom <= settings.bottom;
  }), true, "the font menu stays fully inside the scrollable settings viewport");
  await page.screenshot({ path: out + "/font-search.png" });
  await search.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  assert.equal(await font.inputValue(), "Courier New, Consolas");
  await page.waitForFunction(() => window.terminalMock.preview.options.fontFamily === '"Courier New", "Consolas", monospace');
  await page.waitForFunction(() => {
    const buffer = window.terminalMock.preview.buffer.active;
    const text = Array.from({ length: buffer.length }, (_, index) => buffer.getLine(index).translateToString()).join("\n");
    return text.includes("中文终端") && text.includes("0123456789") && text.includes("┌──┬──┐");
  });
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await page.getByRole("region", { name: "终端字体预览", exact: true }).scrollIntoViewIfNeeded();
  await page.screenshot({ path: out + "/font-preview.png" });
  await page.getByRole("button", { name: "保存并应用" }).click();
  await page.waitForFunction(() => window.terminalMock.terminal.options.fontFamily === '"Courier New", "Consolas", monospace'
    && window.terminalMock.terminal.options.fontSize === 20);
  assert.equal(await page.evaluate(() => window.terminalMock.saves.length), 1);
  await browse.click();
  await options.getByRole("button", { name: "默认字体", exact: true }).click();
  assert.equal(await font.inputValue(), "");
  await page.waitForFunction(() => window.terminalMock.preview.options.fontFamily === '"Cascadia Code", Consolas, monospace');
  assert.equal(await page.evaluate(() => window.terminalMock.terminal.options.fontSize), 20);

  const failure = await open(390);
  await failure.evaluate(() => { window.terminalMock.failFontRead = true; });
  await failure.getByRole("button", { name: "本机字体", exact: true }).click();
  await failure.getByText("无法读取本机字体。", { exact: false }).waitFor();
  await failure.getByRole("searchbox").press("Escape");
  await failure.locator("#general-terminal-font").fill("Manual Font, monospace");
  await failure.getByText("无法读取本机字体列表，可以继续手动填写。", { exact: true }).waitFor();
  await failure.getByRole("button", { name: "保存并应用" }).click();
  await failure.waitForFunction(() => window.terminalMock.terminal.options.fontFamily === '"Manual Font", monospace');
  await failure.evaluate(() => { window.terminalMock.failFontRead = false; });
  await failure.getByRole("button", { name: "本机字体", exact: true }).click();
  await failure.getByRole("button", { name: "重试", exact: true }).click();
  await failure.getByRole("button", { name: "Consolas 等宽" }).waitFor();
  await failure.getByRole("searchbox").press("Escape");
  await failure.locator("#general-terminal-font-size").selectOption("28");
  await failure.waitForFunction(() => {
    const term = window.terminalMock.preview;
    const buffer = term.buffer.active;
    const text = Array.from({ length: buffer.length }, (_, index) => buffer.getLine(index).translateToString(true)).join("");
    return term.options.fontSize === 28 && text.includes("中文终端") && text.includes("0123456789");
  });
  assert.equal(await failure.evaluate(() => {
    const preview = document.querySelector(".terminal-font-preview");
    return preview.scrollWidth <= preview.clientWidth + 1;
  }), true);
  assert.deepEqual(errors, []);
  console.log("PASS terminal fonts: scrollbar track/drag, outside dismissal, lazy catalog, alias search, keyboard, fallback preview, apply and retry");
} finally {
  await browser.close();
}
