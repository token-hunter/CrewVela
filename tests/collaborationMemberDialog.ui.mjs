// Run against Vite: node tests/collaborationMemberDialog.ui.mjs
// Use CREWVELA_BROWSER=webkit on macOS with Playwright WebKit installed.
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { chromium, webkit } from "playwright";

const engine = process.env.CREWVELA_BROWSER === "webkit" ? webkit : chromium;
const browser = await engine.launch(process.env.CREWVELA_BROWSER === "webkit" ? {} : { channel: process.env.CREWVELA_BROWSER_CHANNEL || "chrome" });
const fixtureUrl = `${process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420"}/tests/fixtures/collaboration-member-dialog-ui.html`;
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  await page.goto(fixtureUrl);
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
  await page.goto(`${fixtureUrl}?state=empty`);
  await page.getByRole("status").filter({ hasText: "成员必须归属协作项目" }).waitFor();
  assert.equal(await add.isDisabled(), true, "members require an existing project");
  assert.match(await add.getAttribute("title"), /点击「创建项目」/);
  assert.equal(await add.getAttribute("aria-describedby"), "collab-member-unavailable");
  const noticeBounds = await page.locator("#collab-member-unavailable").boundingBox();
  const addBounds = await add.boundingBox();
  assert.ok(noticeBounds.y >= addBounds.y + addBounds.height, "explanation stays below the member controls");
  assert.equal(await page.getByText("暂无可用成员，点击右上角「新增成员」创建。", { exact: true }).count(), 0, "empty projects must not direct users to a disabled button");
  mkdirSync("output/playwright", { recursive: true });
  await page.screenshot({ path: "output/playwright/collaboration-empty-project.png", fullPage: true });
  assert.equal(await page.getByRole("button", { name: "创建项目", exact: true }).isEnabled(), true, "empty projects offer direct creation");
  for (const [state, message] of [["projects-pending", "正在加载协作项目"], ["snapshot-pending", "正在加载当前项目"], ["projects-failed", "协作项目未能加载"]]) {
    await page.goto(`${fixtureUrl}?state=${state}`);
    await page.getByRole("status").filter({ hasText: message }).waitFor();
    assert.equal(await add.isDisabled(), true, `${state} must not allow member creation`);
    assert.equal(await page.getByRole("button", { name: "返回应用创建项目", exact: true }).count(), 0, `${state} is not an empty project list`);
  }
  await page.goto(`${fixtureUrl}?state=agents-empty`);
  await page.getByText("暂无可用成员，点击右上角「新增成员」创建。", { exact: true }).waitFor();
  assert.equal(await add.isEnabled(), true, "an existing project without members can add its first member");
  await add.click();
  await page.getByRole("dialog", { name: "新增团队成员" }).waitFor();
  console.log("member dialog clipping, create/edit, validation, save failure and close regression passed");
  console.log("empty project guidance, creation entry, loading and read failure regression passed");
} finally { await browser.close(); }
