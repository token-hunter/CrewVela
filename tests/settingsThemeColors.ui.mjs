// Actual App theme switching in an isolated browser context; no native accounts.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const output = "output/playwright/settings-theme-colors";
mkdirSync(output, { recursive: true });
const browser = await chromium.launch({ channel: process.env.CREWVELA_BROWSER_CHANNEL || "chrome" });
try {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 } });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(`${base}/?ui-preview=1`);
  await page.getByRole("button", { name: "设置", exact: true }).click();
  const nav = page.getByRole("navigation", { name: "设置分类" });
  await nav.getByRole("button", { name: "外观", exact: true }).click();
  const palettes = {};
  for (const [id, label] of [["light", "浅色"], ["sand", "暖沙"], ["dark", "深色"], ["ocean", "深海"], ["purple", "石墨黑"], ["aurora", "暖瓷白"]]) {
    const option = page.getByRole("button", { name: label, exact: true });
    await option.click();
    await page.waitForFunction(theme => document.documentElement.dataset.appearanceTheme === theme, id);
    assert.equal(await option.getAttribute("aria-pressed"), "true");
    const palette = await page.evaluate(() => {
      const root = getComputedStyle(document.documentElement);
      const frame = document.querySelector(".settings-dialog.settings-native");
      const css = getComputedStyle(frame);
      const color = selector => getComputedStyle(frame.querySelector(selector)).backgroundColor;
      return {
        rootBg: root.getPropertyValue("--bg-elev").trim(),
        settingsBg: css.getPropertyValue("--bg-elev").trim(),
        canvas: css.backgroundColor,
        header: color(".settings-window-header"),
        nav: color(".settings-nav"),
        input: color(".settings-search input"),
        inputBg: getComputedStyle(frame.querySelector(".settings-search input")).getPropertyValue("--bg-elev").trim(),
        text: css.color,
        border: css.borderTopColor,
        scheme: css.colorScheme,
        box: { width: frame.getBoundingClientRect().width, height: frame.getBoundingClientRect().height },
      };
    });
    palettes[id] = palette;
    await page.screenshot({ path: `${output}/${id}.png`, animations: "disabled" });
    // Sand and dark palettes must inherit the actual selected theme tokens.
    if (id !== "light") assert.equal(palette.settingsBg, palette.rootBg, `${label}: settings overrides selected palette`);
    assert.equal(palette.header, id === "purple" ? "rgb(25, 26, 28)" : palette.canvas, `${label}: header palette`);
    assert.equal(palette.nav, id === "aurora" ? "rgb(235, 229, 224)" : id === "purple" ? "rgb(25, 26, 28)" : palette.canvas, `${label}: navigation palette`);
    assert.equal(palette.inputBg, palette.settingsBg, `${label}: input palette tokens`);
    assert.ok([palette.canvas, id === "purple" ? "rgb(40, 42, 45)" : palette.canvas, "rgba(0, 0, 0, 0)"].includes(palette.input), `${label}: input surface`);
    assert.deepEqual(palette.box, palettes.light.box, `${label}: fixed frame size`);
    assert.equal(palette.scheme, ["light", "sand", "aurora"].includes(id) ? "light" : "dark");
  }
  assert.equal(palettes.light.canvas, "rgb(255, 255, 255)");
  assert.equal(palettes.sand.canvas, "rgb(255, 250, 242)");
  assert.equal(palettes.sand.text, "rgb(53, 47, 40)");
  assert.equal(palettes.sand.border, "rgb(216, 202, 182)");
  assert.equal(palettes.aurora.canvas, "rgb(253, 250, 247)");
  assert.equal(palettes.aurora.text, "rgb(57, 50, 46)");
  assert.equal(palettes.aurora.border, "rgb(217, 209, 202)");
  assert.equal(palettes.purple.canvas, "rgb(32, 33, 35)");
  assert.equal(palettes.purple.text, "rgb(231, 232, 233)");
  assert.equal(palettes.purple.border, "rgb(65, 68, 73)");
  assert.equal(new Set(Object.values(palettes).map(p => p.canvas)).size, 6);
  await page.getByRole("button", { name: "暖沙", exact: true }).click();
  await nav.getByRole("button", { name: "常规", exact: true }).click();
  assert.equal(await page.locator(".settings-native").evaluate(el => getComputedStyle(el).backgroundColor), palettes.sand.canvas);
  await page.getByRole("button", { name: "关闭设置", exact: true }).click();
  await page.reload();
  await page.waitForFunction(() => document.documentElement.dataset.appearanceTheme === "sand");
  await page.getByRole("button", { name: "设置", exact: true }).click();
  assert.equal(await page.locator(".settings-native").evaluate(el => getComputedStyle(el).backgroundColor), palettes.sand.canvas);
  assert.deepEqual(errors, []);
  writeFileSync(`${output}/palettes.json`, JSON.stringify(palettes, null, 2));
  console.log("PASS: six actual App theme choices, canvas/navigation/inputs, fixed geometry, menu switching and reload persistence");
  await context.close();
} finally { await browser.close(); }
