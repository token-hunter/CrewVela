import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { chromium } from "playwright";

const output = "output/playwright/theme-contrast";
mkdirSync(output, { recursive: true });
const browser = await chromium.launch({ channel: "chrome", headless: true });
const accents = ["clay", "blue", "purple", "cyan", "green", "orange", "pink", "coral", "gray"];
const luminance = (color) => {
  const components = color.match(/[\d.]+/g).slice(0, 3).map(Number).map(value => {
    const channel = value / 255;
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  });
  return components[0] * 0.2126 + components[1] * 0.7152 + components[2] * 0.0722;
};
const contrast = (a, b) => (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.stack || error.message));
  await page.goto(`${process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420"}/?ui-preview=1`);
  await page.addStyleTag({ content: "*, *::before, *::after { transition: none !important; animation: none !important; }" });
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page.locator(".settings-nav").getByRole("button", { name: "模型设置", exact: true }).click();
  await page.getByRole("button", { name: "管理连接", exact: true }).click();
  const add = page.getByRole("button", { name: "新建模型服务", exact: true });
  await add.waitFor();
  for (const theme of ["light", "dark"]) {
    for (const accent of accents) {
      await page.evaluate(({ theme, accent }) => {
        document.documentElement.dataset.theme = theme;
        document.documentElement.dataset.appearanceTheme = theme;
        document.documentElement.dataset.appearanceAccent = accent;
      }, { theme, accent });
      await add.evaluate(async element => {
        await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        await Promise.all(element.getAnimations().map(animation => animation.finished.catch(() => {})));
      });
      const colors = await add.evaluate(element => {
        const style = getComputedStyle(element);
        return { text: style.color, background: style.backgroundColor };
      });
      const ratio = contrast(luminance(colors.text), luminance(colors.background));
      assert.ok(ratio >= 4.5, `${theme}/${accent}: primary action contrast ${ratio.toFixed(2)} must be at least 4.5 (${JSON.stringify(colors)})`);
      await add.hover();
      const hoverColors = await add.evaluate(element => {
        const style = getComputedStyle(element);
        return { text: style.color, background: style.backgroundColor };
      });
      const hoverRatio = contrast(luminance(hoverColors.text), luminance(hoverColors.background));
      assert.ok(hoverRatio >= 4.5, `${theme}/${accent}: hovered primary action contrast ${hoverRatio.toFixed(2)}`);
      await page.mouse.move(0, 0);
      console.log(`PASS ${theme}/${accent}: primary action contrast ${ratio.toFixed(2)}`);
      if (accent === "blue") await page.screenshot({ path: `${output}/${theme}-models.png` });
    }
  }
  assert.deepEqual(errors, []);
} finally {
  await browser.close();
}
