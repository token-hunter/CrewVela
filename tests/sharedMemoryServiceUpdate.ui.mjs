import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const output = ".scratch/shared-memory-service-update/ui";
mkdirSync(output, { recursive: true });
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 1100 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.clock.install();
  await page.goto(`${base}/tests/fixtures/shared-memory-quick-setup.html`);
  const openLegacy = async () => {
    await page.getByRole("button", { name: "更多选项", exact: true }).click();
    await page.getByRole("menuitem", { name: "旧版共享记忆", exact: true }).click();
  };
  await openLegacy();
  // No project, no installed Agent CLI: a managed service can still be upgraded.
  await page.evaluate(() => Object.assign(window.__quickInfo, {
    installedVersion: "2.6.0", managed: true,
    agents: [{ id: "claude-code", installed: false }, { id: "codex", installed: false }],
  }));
  await page.getByRole("button", { name: "检查更新", exact: true }).click();
  const update = page.locator('button[aria-describedby="sm-update-reason"]');
  assert(await update.isDisabled(), "major upgrades require confirmation");
  await page.getByRole("checkbox", { name: /确认跨大版本迁移/ }).check();
  assert(await update.isEnabled(), "service update must not require a project or Agent CLI");
  await page.screenshot({ path: `${output}/major-confirmed.png`, fullPage: true });
  await update.click();
  const request = await page.evaluate(() => window.__quickCalls.find(x => x.command === "ai_memory_quick_start").args.request);
  assert.equal(request.update, true);
  assert.equal(request.targetVersion, "3.4.5");
  assert.equal(request.confirmMajor, true);
  assert.equal(request.projectDir, undefined, "do not send a project for service updates");
  assert.equal(request.agents, undefined, "do not send Agent configuration targets");
  assert.equal(await page.evaluate(() => window.__quickCalls.filter(x => x.command === "plugin:dialog|open").length), 0);
  assert(await update.isDisabled(), "do not submit overlapping updates");
  await page.evaluate(() => {
    window.__quickJob.steps[0].state = "done";
    const download = window.__quickJob.steps.find(step => step.id === "install");
    download.state = "running";
    download.detail = "已下载 8.0 / 25.9 MiB（31%）";
  });
  await page.clock.runFor(1200);
  await page.getByText("已下载 8.0 / 25.9 MiB（31%）", { exact: true }).waitFor();
  await page.screenshot({ path: `${output}/download-progress.png`, fullPage: true });
  await page.evaluate(() => {
    window.__quickJob.state = "failed";
    const download = window.__quickJob.steps.find(step => step.id === "install");
    download.state = "failed";
    download.detail = "模拟更新失败，已恢复升级前快照";
  });
  await page.clock.runFor(1200);
  await page.getByText("模拟更新失败，已恢复升级前快照", { exact: true }).waitFor();
  await page.getByRole("alert").filter({ hasText: "模拟更新失败，已恢复升级前快照" }).waitFor();
  await page.screenshot({ path: `${output}/failure-summary.png`, fullPage: true });
  assert(await update.isEnabled(), "failed service updates can retry without setup targets");
  await update.click();
  assert.equal(await page.evaluate(() => window.__quickCalls.filter(x => x.command === "ai_memory_quick_start").length), 2);
  await page.evaluate(() => {
    window.__quickInfo.installedVersion = "3.4.5";
    window.__quickJob.state = "updated";
    window.__quickJob.steps.forEach(step => { step.state = "done"; });
  });
  await page.clock.runFor(1200);
  await page.getByText("服务更新完成，已有接入配置保持不变", { exact: true }).waitFor();
  assert.equal(await page.getByRole("button", { name: "前往接入检查", exact: true }).count(), 0);
  assert(await page.getByRole("button", { name: "配置项目接入", exact: true }).isDisabled());
  for (const theme of ["light", "dark", "sand"]) {
    await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
    await page.setViewportSize({ width: 600, height: 1000 });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    await page.screenshot({ path: `${output}/${theme}-600.png`, fullPage: true });
  }
  // A minor update needs neither the migration checkbox nor an Agent selection.
  await page.reload();
  await openLegacy();
  await page.evaluate(() => Object.assign(window.__quickInfo, { installedVersion: "3.4.0", managed: true, agents: [] }));
  await page.getByRole("button", { name: "检查更新", exact: true }).click();
  assert.equal(await page.getByRole("checkbox", { name: /确认跨大版本迁移/ }).count(), 0);
  assert(await update.isEnabled());
  await page.evaluate(() => { window.__quickInfo.updateError = "模拟校验信息获取失败"; });
  await page.getByRole("button", { name: "检查更新", exact: true }).click();
  assert(await update.isDisabled());
  await page.getByRole("status").filter({ hasText: "模拟校验信息获取失败" }).waitFor();
  assert.deepEqual(errors, []);
  console.log("PASS: service update without project/CLI, major confirmation, preserved configuration scope, overlap guard, failure/retry, distinct completion, minor update, failure explanation and responsive themes");
} finally { await browser.close(); }
