// Real settings navigation in browser preview; no native runtime or installation.
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const output = ".scratch/plugin-market-size";
mkdirSync(output, { recursive: true });
const browser = await chromium.launch({ channel: "chrome", headless: true });
const failures = [];
const near = (actual, expected, message) => {
  if (Math.abs(actual - expected) > 1) failures.push(`${message}: expected ${expected}, got ${actual}`);
};

async function geometry(page) {
  return page.evaluate(() => {
    const dialog = document.querySelector(".settings-dialog");
    const nav = dialog.querySelector(".settings-nav");
    const list = dialog.querySelector(".settings-nav-list");
    const content = dialog.querySelector(".settings-content");
    // Clicking a lower navigation entry scrolls it into view on short screens.
    // Normalize scroll offsets so measurements compare layout, not scroll state.
    nav.scrollTop = 0;
    list.scrollTop = 0;
    content.scrollTop = 0;
    const inner = dialog.querySelector(".settings-content-inner");
    const title = dialog.querySelector("#settings-title");
    const buttons = [...nav.querySelectorAll(".settings-nav-item")];
    const rect = dialog.getBoundingClientRect();
    return {
      frame: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
      navHeight: nav.getBoundingClientRect().height,
      navScrollHeight: nav.scrollHeight,
      // The MCP help note legitimately reduces the flex list's available height.
      // Compare the entries' intrinsic span rather than that available height.
      navEntriesHeight: buttons.at(-1).getBoundingClientRect().bottom - buttons[0].getBoundingClientRect().top,
      navTop: nav.getBoundingClientRect().top,
      buttonHeights: buttons.map(el => el.getBoundingClientRect().height),
      firstButtonTop: buttons[0].getBoundingClientRect().top,
      titleFontSize: parseFloat(getComputedStyle(title).fontSize),
      titleTop: title.getBoundingClientRect().top,
      bodyPaddingTop: parseFloat(getComputedStyle(inner).paddingTop),
      documentOverflow: document.documentElement.scrollWidth - innerWidth,
      contentOverflow: content.scrollWidth - content.clientWidth,
    };
  });
}

function compare(actual, baseline, label, size) {
  const margin = size.width <= 720 ? 8 : size.width <= 1000 ? 16 : 32;
  const width = Math.min(1280, size.width - margin * 2);
  const height = Math.min(900, size.height - margin * 2);
  near(actual.frame.x, (size.width - width) / 2, `${label}: frame x`);
  near(actual.frame.y, (size.height - height) / 2, `${label}: frame y`);
  near(actual.frame.width, width, `${label}: frame width`);
  near(actual.frame.height, height, `${label}: frame height`);
  assert.ok(actual.documentOverflow <= 1, `${label}: document horizontal overflow`);
  assert.ok(actual.contentOverflow <= 1, `${label}: content horizontal overflow`);
  for (const key of ["navHeight", "navScrollHeight", "navEntriesHeight", "navTop", "firstButtonTop", "titleFontSize", "titleTop", "bodyPaddingTop"]) {
    near(actual[key], baseline[key], `${label}: ${key}`);
  }
  assert.equal(actual.buttonHeights.length, baseline.buttonHeights.length, `${label}: navigation entries retained`);
  actual.buttonHeights.forEach((height, i) => near(height, baseline.buttonHeights[i], `${label}: button ${i} height`));
}

try {
  for (const size of [{ width: 1440, height: 900 }, { width: 820, height: 540 }, { width: 1920, height: 1080 }]) {
    for (const theme of ["dark", "light"]) {
      const label = `${size.width}x${size.height}-${theme}`;
      const context = await browser.newContext({ viewport: size });
      const page = await context.newPage();
      page.setDefaultTimeout(10000);
      const errors = [];
      page.on("pageerror", error => errors.push(error.message));
      await page.addInitScript(theme => localStorage.setItem("crewvela-appearance", JSON.stringify({ theme, accent: "blue", background: "solid" })), theme);
      await page.goto(`${base}/?ui-preview=1`);
      await page.getByRole("button", { name: "设置", exact: true }).click();
      const nav = page.locator(".settings-nav");
      await nav.getByRole("button", { name: "电脑控制", exact: true }).click();
      await page.getByRole("heading", { name: "电脑控制", exact: true }).waitFor();
      const baseline = await geometry(page);
      console.log(`BASELINE ${label} ${JSON.stringify(baseline)}`);
      compare(baseline, baseline, `${label}/电脑控制`, size);

      for (const section of ["插件市场", "Skills", "常规", "模型设置", "MCP", "电脑控制", "插件市场"]) {
        await nav.getByRole("button", { name: section, exact: true }).click();
        await page.locator("#settings-title").filter({ hasText: section }).waitFor();
        compare(await geometry(page), baseline, `${label}/${section}`, size);
      }

      await page.locator(".pm-directory-row").first().waitFor();
      assert.equal(await page.locator(".pm-directory-row").count(), 16, `${label}: preview catalog loaded`);
      await page.getByRole("combobox", { name: "插件分类", exact: true }).selectOption("设计与创作");
      assert.equal(await page.locator(".pm-directory-row").count(), 6, `${label}: category filters catalog`);
      compare(await geometry(page), baseline, `${label}/分类`, size);
      await page.getByRole("combobox", { name: "插件分类", exact: true }).selectOption("全部");
      await page.getByRole("textbox", { name: "搜索插件或使用场景" }).fill("PDF 处理");
      assert.equal(await page.locator(".pm-directory-row").count(), 1, `${label}: search filters catalog`);
      assert.match(await page.locator(".pm-directory-row").innerText(), /PDF/);
      compare(await geometry(page), baseline, `${label}/搜索`, size);
      await page.getByRole("textbox", { name: "搜索插件或使用场景" }).fill("");
      await page.locator(".pm-directory-row").first().click();
      const detail = page.getByRole("region", { name: "插件详情与安装" });
      await detail.waitFor();
      compare(await geometry(page), baseline, `${label}/详情打开`, size);
      await page.locator(".pm-directory-row").nth(1).click();
      await detail.getByRole("heading", { name: "MCP 开发", exact: true }).waitFor();
      compare(await geometry(page), baseline, `${label}/详情切换`, size);
      await page.screenshot({ path: `${output}/${label}-market.png`, animations: "disabled" });
      assert.deepEqual(errors, [], `${label}: no page errors`);
      console.log(`CHECKED ${label}: navigation, category, search, detail selection`);
      await context.close();
    }
  }
  assert.deepEqual(failures, [], "Settings shell geometry must remain stable across sections and interactions");
  console.log("PASS: 6 viewport/theme combinations, stable settings shell and working plugin catalog interactions");
} finally {
  await browser.close();
}
