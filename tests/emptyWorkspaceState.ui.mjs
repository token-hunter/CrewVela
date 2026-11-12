// Run against Vite: node tests/emptyWorkspaceState.ui.mjs
// 空工作区应给出标题、说明和明确的主操作，而不是只剩一个孤零零的按钮。
import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
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
    const action = empty.getByRole("button", { name: "＋ 新建会话", exact: true });
    await action.waitFor();

    // 标题、说明、按钮、提示都在空状态区域内，且彼此不重叠、不被裁掉。
    const boxes = await empty.evaluate((root) => {
      const area = root.getBoundingClientRect();
      return [...root.children].map((child) => {
        const r = child.getBoundingClientRect();
        return { tag: child.tagName, top: r.top, bottom: r.bottom, inside: r.top >= area.top - 1 && r.bottom <= area.bottom + 1 && r.left >= area.left - 1 && r.right <= area.right + 1 };
      });
    });
    assert.ok(boxes.length >= 4, `kicker, heading, description, action and hint expected: ${JSON.stringify(boxes)}`);
    for (const [index, box] of boxes.entries()) {
      assert.equal(box.inside, true, `${width}x${height}: child ${index} is clipped: ${JSON.stringify(boxes)}`);
      if (index > 0) assert.ok(box.top >= boxes[index - 1].bottom - 1, `${width}x${height}: child ${index} overlaps the previous one: ${JSON.stringify(boxes)}`);
    }

    // 主操作仍然通向新建会话流程。
    await action.click();
    await page.getByText("Claude Code", { exact: true }).first().waitFor();
    assert.deepEqual(pageErrors.filter((message) => !/transformCallback|__TAURI/.test(message)), []);
    await page.close();
  }
  console.log("PASS: empty workspace shows heading, guidance, primary action and hint without clipping");
} finally {
  await browser.close();
}
