// Real SettingsDialog rendering with empty and long archived-session datasets.
// Browser geometry validates the web layout, not native Tauri window management.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const output = ".scratch/crewvela-brand-cleanup/settings-frame";
mkdirSync(output, { recursive: true });
const browser = await chromium.launch({ channel: "chrome", headless: true });
const failures = [];
const evidence = [];
const menus = ["常规", "Git", "通知", "外观", "陪伴", "模型设置", "协作中心", "共享记忆", "自动化", "快捷指令",
  "浏览器控制", "电脑控制", "插件市场", "Skills", "MCP", "归档会话"];
const sizes = [
  { width: 687, height: 903 },
  { width: 720, height: 540 }, { width: 740, height: 540 }, { width: 760, height: 540 },
  { width: 820, height: 540 }, { width: 1440, height: 900 }, { width: 1920, height: 1080 },
];
const near = (actual, expected, label) => {
  if (Math.abs(actual - expected) > 1) failures.push(`${label}: expected ${expected}, got ${actual}`);
};

async function geometry(page) {
  return page.evaluate(() => {
    const frame = document.querySelector(".settings-dialog");
    const nav = frame.querySelector(".settings-nav");
    const content = frame.querySelector(".settings-content");
    const inner = frame.querySelector(".settings-content-inner");
    const rect = element => {
      const r = element.getBoundingClientRect();
      return { x: r.x, y: r.y, width: r.width, height: r.height };
    };
    return {
      frame: rect(frame), nav: rect(nav), inner: rect(inner), content: rect(content),
      title: rect(frame.querySelector("#settings-title")),
      titleFont: getComputedStyle(frame.querySelector("#settings-title")).fontSize,
      titleLineHeight: getComputedStyle(frame.querySelector("#settings-title")).lineHeight,
      documentWidth: document.documentElement.scrollWidth, documentHeight: document.documentElement.scrollHeight,
      documentTop: document.documentElement.scrollTop, contentWidth: content.scrollWidth,
      contentClientWidth: content.clientWidth, navDisplay: getComputedStyle(nav).display,
      outerWidth, outerHeight, innerWidth, innerHeight,
    };
  });
}

try {
  for (const size of sizes) for (const theme of ["dark", "light"]) for (const archive of ["empty", "long"]) {
    const label = `${size.width}x${size.height}-${theme}-${archive}`;
    const context = await browser.newContext({ viewport: size });
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.goto(`${base}/tests/fixtures/crewvela-settings-frame-ui.html?ui-preview=1&theme=${theme}&archive=${archive}`);
    await page.getByRole("button", { name: "打开设置回归", exact: true }).click();
    const baseline = await geometry(page);
    for (const menu of menus) {
      const entry = page.locator(".settings-nav").getByRole("button", { name: menu, exact: true });
      if (!await entry.isVisible()) { failures.push(`${label}/${menu}: navigation hidden`); continue; }
      await entry.click();
      // The collaboration page supplies its own heading inside the common shell.
      await page.locator("#settings-title").filter({ hasText: menu }).waitFor({ state: "attached" });
      // Compare the common shell at its scroll origin; long sections must scroll only their body.
      await page.locator(".settings-content").evaluate(el => { el.scrollTop = 0; });
      const g = await geometry(page);
      evidence.push({ label, menu, ...g });
      for (const field of ["x", "y", "width", "height"]) {
        near(g.frame[field], baseline.frame[field], `${label}/${menu}/frame-${field}`);
        near(g.nav[field], baseline.nav[field], `${label}/${menu}/nav-${field}`);
      }
      for (const field of ["x", "width"]) near(g.inner[field], baseline.inner[field], `${label}/${menu}/body-${field}`);
      if (g.title.width > 0) near(g.title.x, baseline.title.x, `${label}/${menu}/title-x`);
      if (g.titleFont !== baseline.titleFont || g.titleLineHeight !== baseline.titleLineHeight) failures.push(`${label}/${menu}: common heading typography changed`);
      const margin = size.width <= 720 ? 8 : size.width <= 1000 ? 16 : 32;
      near(g.frame.width, Math.min(1280, size.width - margin * 2), `${label}/${menu}/frame-width`);
      near(g.frame.height, Math.min(900, size.height - margin * 2), `${label}/${menu}/frame-height`);
      assert.ok(await page.getByRole("button", { name: "关闭设置", exact: true }).isVisible(), `${label}: persistent close control`);
      near(g.documentWidth, size.width, `${label}/${menu}/root-width`);
      near(g.documentHeight, size.height, `${label}/${menu}/root-height`);
      near(g.documentTop, 0, `${label}/${menu}/root-scroll`);
      near(g.outerWidth, baseline.outerWidth, `${label}/${menu}/browser-outer-width`);
      near(g.outerHeight, baseline.outerHeight, `${label}/${menu}/browser-outer-height`);
      if (g.contentWidth > g.contentClientWidth + 1) failures.push(`${label}/${menu}: body horizontally overflows`);
      if (menu === "归档会话") {
        assert.equal(await page.locator(".archived-settings-row").count(), archive === "long" ? 100 : 0);
        if (archive === "long") {
          await page.locator(".settings-content").evaluate(el => { el.scrollTop = el.scrollHeight; });
          const scrolled = await geometry(page);
          near(scrolled.nav.y, baseline.nav.y, `${label}/archive-scroll/nav-y`);
          near(scrolled.documentTop, 0, `${label}/archive-scroll/root-scroll`);
          assert.ok(await page.locator(".settings-content").evaluate(el => el.scrollTop > 0), `${label}: archive body scrolls`);
          await page.locator(".settings-content").evaluate(el => { el.scrollTop = 0; });
        }
        if ([820, 1920].includes(size.width) && theme === "dark") {
          await page.screenshot({ path: `${output}/${label}-archive.png`, animations: "disabled" });
        }
      }
    }
    await page.getByRole("button", { name: "关闭设置", exact: true }).click();
    assert.equal(await page.locator(".settings-dialog").count(), 0, `${label}: exit settings`);
    await page.getByRole("button", { name: "打开设置回归", exact: true }).click();
    const reopened = await geometry(page);
    near(reopened.frame.width, baseline.frame.width, `${label}/reopen-width`);
    near(reopened.frame.height, baseline.frame.height, `${label}/reopen-height`);
    assert.deepEqual(errors, [], `${label}: no page errors`);
    console.log(`CHECKED ${label}: ${menus.length} menus, root/body bounds, archive scroll, close/reopen`);
    await context.close();
  }
  writeFileSync(`${output}/geometry.json`, JSON.stringify(evidence, null, 2));
  writeFileSync(`${output}/failures.json`, JSON.stringify(failures, null, 2));
  assert.deepEqual(failures, [], "All settings sections must retain the same frame and body boundary");
  console.log(`PASS: ${sizes.length * 4} viewport/theme/data combinations, ${evidence.length} menu visits`);
} finally { await browser.close(); }
