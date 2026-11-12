// Isolated form regression; IPC is fixture-only and never changes local settings.
import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: process.env.CREWVELA_BROWSER_CHANNEL || "chrome" });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1024 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  const url = query => `${base}/tests/fixtures/crewvela-settings-frame-ui.html?open=1&section=browserControl&${query}`;
  await page.goto(url("mock=browser&fail=1&theme=dark"));
  const channel = page.getByLabel("使用的浏览器", { exact: true });
  const save = page.getByRole("button", { name: "保存并应用", exact: true });
  assert.equal(await save.isEnabled(), false);
  await channel.selectOption("msedge");
  await save.click();
  await page.getByText(/保存失败.*回归测试：写入失败/).waitFor();
  assert.equal(await channel.inputValue(), "msedge");
  assert.equal(await page.locator(".browser-saved-facts dd").innerText(), "Chrome");
  await save.click();
  await page.getByText("已开启浏览器控制", { exact: true }).waitFor();
  assert.equal(await page.locator(".browser-saved-facts dd").innerText(), "Edge");
  assert.equal(await save.isEnabled(), false);
  await channel.selectOption("custom");
  await save.click();
  await page.getByText("选择自定义浏览器时必须填写可执行文件路径", { exact: true }).waitFor();
  const path = page.getByLabel("浏览器可执行文件路径", { exact: true });
  await path.fill("chrome.exe");
  await save.click();
  await page.getByText("自定义浏览器路径必须是绝对路径", { exact: true }).waitFor();
  await path.fill("/fixture/Chrome/Browser");
  await save.click();
  await page.getByText("已开启浏览器控制", { exact: true }).waitFor();
  const enable = page.getByRole("switch").first();
  await enable.uncheck();
  assert.equal(await channel.isEnabled(), false);
  assert.equal(await path.isEnabled(), false);
  await save.click();
  await page.getByText("已关闭浏览器控制", { exact: true }).waitFor();
  await page.goto(url("mock=browser&failRead=1"));
  await page.getByText(/读取浏览器控制设置失败/).waitFor();
  await page.getByRole("button", { name: "重新读取设置", exact: true }).click();
  await page.getByRole("heading", { name: "当前运行环境", exact: true }).waitFor();
  for (const theme of ["dark", "light"]) {
    for (const viewport of [{ width: 1440, height: 1024 }, { width: 820, height: 540 }, { width: 687, height: 903 }]) {
      await page.setViewportSize(viewport);
      await page.goto(url(`ui-preview=1&theme=${theme}`));
      await channel.selectOption("custom");
      await path.fill("/fixture/" + "long-directory/".repeat(12) + "chrome");
      await page.locator(".runtime-details > summary").click();
      await page.locator(".runtime-details > summary").press("Control+End");
      const geometry = await page.evaluate(() => {
        const frame = document.querySelector(".settings-native").getBoundingClientRect();
        const footer = document.querySelector(".runtime-actions").getBoundingClientRect();
        const body = document.querySelector(".workspace-browser-stack");
        return { footerVisible: footer.bottom <= frame.bottom && footer.top > frame.top,
          scrolled: body.scrollTop > 0, outerScroll: document.querySelector(".settings-content").scrollTop,
          overflow: body.scrollWidth > body.clientWidth || document.documentElement.scrollWidth > innerWidth };
      });
      assert.equal(geometry.footerVisible, true);
      assert.equal(geometry.scrolled, true);
      assert.equal(geometry.outerScroll, 0);
      assert.equal(geometry.overflow, false);
    }
  }
  assert.deepEqual(errors, []);
} finally {
  await browser.close();
}
