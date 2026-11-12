// Run against Vite: node tests/emptyWorkspaceState.ui.mjs
// 空工作区应给出标题、说明和明确的主操作，而不是只剩一个孤零零的按钮。
// 布局参照 Cowork 新任务页：衬线标题 + 输入框式入口卡片，整张卡片都能打开新建流程。
import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const accents = ["clay", "blue", "purple", "cyan", "green", "orange", "pink", "coral", "gray"];
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  for (const [width, height] of [[1280, 800], [820, 540]]) {
    const page = await browser.newPage({ viewport: { width, height } });
    const pageErrors = [];
    page.on("pageerror", (error) => pageErrors.push(error.message));
    await page.goto(`${base}/?ui-preview=1&sessions=empty`);
    const empty = page.locator(".empty-state");
    await empty.waitFor();
    await empty.getByRole("heading", { name: "这个工作区还没有会话", exact: true }).waitFor();
    assert.match(await empty.locator("p").innerText(), /新建一个 Agent 会话/);
    const action = empty.getByRole("button", { name: "新建会话", exact: true });
    await action.waitFor();
    const mascotLoaded = await empty.locator("img.empty-state-mascot").evaluate(
      (img) => img.complete && img.naturalWidth > 0,
    );
    assert.equal(mascotLoaded, true, `${width}x${height}: heading mascot image loads`);

    // 标题、说明、按钮、提示都在空状态区域内，且彼此不重叠、不被裁掉。
    const boxes = await empty.evaluate((root) => {
      const area = root.getBoundingClientRect();
      return [...root.children].map((child) => {
        const r = child.getBoundingClientRect();
        return { tag: child.tagName, top: r.top, bottom: r.bottom, inside: r.top >= area.top - 1 && r.bottom <= area.bottom + 1 && r.left >= area.left - 1 && r.right <= area.right + 1 };
      });
    });
    assert.ok(boxes.length >= 4, `heading, description, action and hint expected: ${JSON.stringify(boxes)}`);
    for (const [index, box] of boxes.entries()) {
      assert.equal(box.inside, true, `${width}x${height}: child ${index} is clipped: ${JSON.stringify(boxes)}`);
      if (index > 0) assert.ok(box.top >= boxes[index - 1].bottom - 1, `${width}x${height}: child ${index} overlaps the previous one: ${JSON.stringify(boxes)}`);
    }

    // 入口里的「新建会话」文字在各主题、各强调色下都要可读（含悬停态）。
    if (width === 1280) {
      await page.addStyleTag({ content: "*, *::before, *::after { transition: none !important; animation: none !important; }" });
      const chip = action.locator(".empty-state-start-chip");
      for (const theme of ["dark", "light"]) {
        for (const accent of accents) {
          await page.evaluate(({ theme, accent }) => {
            document.documentElement.dataset.theme = theme;
            document.documentElement.dataset.appearanceTheme = theme;
            document.documentElement.dataset.appearanceAccent = accent;
          }, { theme, accent });
          for (const state of ["rest", "hover"]) {
            if (state === "hover") await action.hover();
            else await page.mouse.move(0, 0);
            const ratio = await chip.evaluate((element) => {
              const channels = (value) => {
                const srgb = value.match(/color\(srgb ([\d.]+) ([\d.]+) ([\d.]+)/);
                if (srgb) return srgb.slice(1, 4).map((part) => Number(part) * 255);
                return value.match(/[\d.]+/g).slice(0, 3).map(Number);
              };
              const luminance = (value) => {
                const [r, g, b] = channels(value).map((channel) => {
                  const c = channel / 255;
                  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
                });
                return r * 0.2126 + g * 0.7152 + b * 0.0722;
              };
              const style = getComputedStyle(element);
              const [a, b] = [luminance(style.color), luminance(style.backgroundColor)];
              return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
            });
            assert.ok(ratio >= 4.5, `${theme}/${accent}/${state}: 新建会话 chip contrast ${ratio.toFixed(2)} must be at least 4.5`);
          }
        }
      }
      await page.mouse.move(0, 0);
    }

    // 主操作仍然通向新建会话流程；点卡片空白处（不是右下角的标签）也一样。
    await action.click({ position: { x: 24, y: 20 } });
    await page.getByText("Claude Code", { exact: true }).first().waitFor();
    assert.deepEqual(pageErrors.filter((message) => !/transformCallback|__TAURI/.test(message)), []);
    await page.close();
  }
  console.log("PASS: empty workspace shows heading, guidance, a readable whole-card primary action and hint without clipping");
} finally {
  await browser.close();
}
