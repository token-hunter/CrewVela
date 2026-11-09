// Run against Vite: node tests/collaborationMemberDialog.ui.mjs
// Use CREWVELA_BROWSER=webkit on macOS with Playwright WebKit installed.
import assert from "node:assert/strict";
import { chromium, webkit } from "playwright";

const engine = process.env.CREWVELA_BROWSER === "webkit" ? webkit : chromium;
const browser = await engine.launch();
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  await page.goto(`${process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420"}/tests/fixtures/collaboration-member-dialog-ui.html`);
  const add = page.getByRole("button", { name: "新增成员", exact: true });
  async function assertAboveParent(dialog) {
    await dialog.waitFor();
    assert.equal(await dialog.evaluate(el => el.closest(".member-fixture-host") === null), true, "member form must escape the settings clipping layer");
    const close = dialog.getByRole("button", { name: "关闭成员编辑" });
    assert.equal(await close.evaluate(el => {
      const r = el.getBoundingClientRect();
      const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
      return hit === el || el.contains(hit);
    }), true, "close button must receive pointer events above the clipping layer");
  }
  await add.click();
  let dialog = page.getByRole("dialog", { name: "新增团队成员" });
  await assertAboveParent(dialog);
  await dialog.getByRole("button", { name: "保存成员", exact: true }).click();
  await page.getByRole("alert").filter({ hasText: "请填写成员名称" }).waitFor();
  await dialog.getByLabel("成员名称", { exact: true }).fill("新增回归成员");
  await dialog.getByRole("button", { name: "保存成员", exact: true }).click();
  await dialog.waitFor({ state: "detached" });
  assert.equal(await page.locator(".collab-agent-card").count(), 3);
  const card = page.locator(".collab-agent-card").filter({ hasText: "新增回归成员" });
  await card.click();
  dialog = page.getByRole("dialog", { name: "编辑团队成员" });
  await assertAboveParent(dialog);
  await dialog.getByLabel("成员名称", { exact: true }).fill("修改回归成员");
  await dialog.getByRole("button", { name: "保存成员", exact: true }).click();
  await dialog.waitFor({ state: "detached" });
  assert.equal(await page.locator(".collab-agent-card").filter({ hasText: "修改回归成员" }).count(), 1);
  await page.getByRole("button", { name: "下次保存失败" }).click();
  await add.click();
  dialog = page.getByRole("dialog", { name: "新增团队成员" });
  await dialog.getByLabel("成员名称", { exact: true }).fill("失败后保留草稿");
  await dialog.getByRole("button", { name: "保存成员", exact: true }).click();
  await dialog.getByRole("alert").filter({ hasText: "保存失败" }).waitFor();
  assert.equal(await dialog.getByLabel("成员名称", { exact: true }).inputValue(), "失败后保留草稿");
  await dialog.getByRole("button", { name: "取消", exact: true }).click();
  await dialog.waitFor({ state: "detached" });
  await add.click();
  await page.getByRole("dialog", { name: "新增团队成员" }).press("Escape");
  await page.getByRole("dialog", { name: "新增团队成员" }).waitFor({ state: "detached" });
  assert.equal(await add.evaluate(el => document.activeElement === el), true, "closing restores focus to the opener");
  console.log("member dialog clipping, create/edit, validation, save failure and close regression passed");
} finally { await browser.close(); }
