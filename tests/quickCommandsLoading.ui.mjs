// Real settings/palette UI with a controlled capability probe; no Agent or user data.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1452";
const out = ".scratch/quick-command-open";
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ channel: "chrome", headless: true });
const page = await browser.newPage({ viewport: { width: 1600, height: 1120 } });
const errors = [];
const checks = [];
page.on("pageerror", error => errors.push(error.message));
const commandRows = () => page.locator(".quick-center-list-row > button:first-child").allTextContents();
const probeCount = () => page.evaluate(() => window.__quickCalls.filter(c => c.method === "ma_probe_runtimes").length);
try {
  await page.goto(`${base}/tests/fixtures/quick-commands-ui.html?ui-preview=1&settings=1&open=0&holdProbe=1`);
  await page.getByRole("navigation", { name: "指令列表" }).waitFor();
  await page.waitForFunction(() => window.__quickCalls.some(c => c.method === "ma_probe_runtimes"));
  await page.screenshot({ path: `${out}/cold-loading.png` });
  const partial = await commandRows();
  writeFileSync(`${out}/cold-before.json`, JSON.stringify({ partial }, null, 2));
  await page.screenshot({ path: `${out}/cold.png` });
  assert.equal(partial.length, 0, "首次检测未完成时不能先显示仅三条的假完整列表，随后再插入 Agent 指令");
  await page.getByRole("button", { name: "选择此范围全部", exact: true }).click();
  await page.getByRole("textbox", { name: "消息内容", exact: true }).fill("检测过程中继续编辑");
  await page.getByRole("button", { name: "确认发送", exact: true }).click();
  await page.getByRole("region", { name: "执行结果" }).getByText("内容已写入", { exact: true }).first().waitFor();
  assert.equal(await page.evaluate(() => window.__quickCalls.filter(c => c.method === "send_session_message").length), 2);
  checks.push("冷启动只显示加载占位，检测不阻塞编辑、目标选择和已有会话发送");
  await page.evaluate(() => window.__quickReleaseProbe());
  await page.getByRole("button", { name: "启动 Codex CLI 本地配置", exact: true }).waitFor();
  const full = await commandRows();
  await page.screenshot({ path: `${out}/ready.png` });

  // Switching settings sections unmounts the center, exactly like re-entering it.
  const nav = page.getByRole("navigation", { name: "设置分类" });
  await nav.getByRole("button", { name: "常规", exact: true }).click();
  await page.evaluate(() => window.__quickHoldProbe());
  const before = await probeCount();
  const start = performance.now();
  await nav.getByRole("button", { name: "快捷指令", exact: true }).click();
  await page.getByRole("button", { name: "启动 Codex CLI 本地配置", exact: true }).waitFor({ timeout: 1000 });
  const openMs = Math.round(performance.now() - start);
  assert.ok(openMs < 500, `再次进入应直接显示已有列表：${openMs}ms`);
  assert.deepEqual(await commandRows(), full);
  assert.equal(await probeCount(), before, "短时间内再次进入不重复执行全量 CLI 探测");
  checks.push(`再次进入直接显示完整列表，无重复探测（浏览器 ${openMs}ms）`);

  await page.getByRole("textbox", { name: "消息内容", exact: true }).fill("重新检测不丢草稿");
  await page.getByRole("button", { name: "重新检测", exact: true }).click();
  await page.waitForFunction(count => window.__quickCalls.filter(c => c.method === "ma_probe_runtimes").length > count, before);
  assert.deepEqual(await commandRows(), full, "重新检测不能清空、重排已有列表");
  assert.equal(await page.getByRole("textbox", { name: "消息内容", exact: true }).inputValue(), "重新检测不丢草稿");
  await page.evaluate(() => { window.__quickFailProbe = true; window.__quickReleaseProbe(); });
  await page.getByRole("alert").filter({ hasText: "读取本机能力失败" }).waitFor();
  assert.deepEqual(await commandRows(), full, "失败必须保留上一次列表，并明确显示错误");
  await page.getByRole("button", { name: "启动 Codex CLI 本地配置", exact: true }).click();
  assert.equal(await page.getByRole("button", { name: "确认启动", exact: true }).isEnabled(), false, "检测失败后不能凭旧能力结果启动");
  checks.push("重新检测保留列表与草稿，失败明确展示且不允许凭失效结果启动");
  await page.evaluate(() => { window.__quickFailProbe = false; });
  await page.getByRole("button", { name: "重新检测", exact: true }).click();
  await page.getByRole("button", { name: "确认启动", exact: true }).waitFor();
  await page.waitForFunction(() => !document.querySelector(".quick-center-error"));
  assert.equal(await page.getByRole("button", { name: "确认启动", exact: true }).isEnabled(), true);
  checks.push("检测失败可重试恢复，不掩盖异常");
  const openSamples = [openMs];
  for (let i = 0; i < 9; i++) {
    await nav.getByRole("button", { name: "常规", exact: true }).click();
    const start = performance.now();
    await nav.getByRole("button", { name: "快捷指令", exact: true }).click();
    await page.getByRole("button", { name: "启动 Codex CLI 本地配置", exact: true }).waitFor();
    openSamples.push(Math.round(performance.now() - start));
    assert.deepEqual(await commandRows(), full);
  }
  assert.ok(Math.max(...openSamples) < 500, `重复进入延迟：${openSamples}`);
  checks.push(`10 次进入完整列表均低于 500ms（${Math.min(...openSamples)}–${Math.max(...openSamples)}ms）`);

  // Cold failures end the placeholder and remain actionable/retryable.
  await page.goto(`${base}/tests/fixtures/quick-commands-ui.html?ui-preview=1&holdProbe=1`);
  await page.getByRole("status", { name: "正在检测本机 Agent 与已配置服务", exact: true }).waitFor();
  await page.evaluate(() => { window.__quickFailProbe = true; window.__quickReleaseProbe(); });
  await page.getByRole("alert").filter({ hasText: "读取本机能力失败" }).waitFor();
  assert.equal(await page.locator(".quick-center-placeholder").count(), 0);
  assert.equal(await page.getByRole("button", { name: "启动 Codex CLI 本地配置", exact: true }).count(), 0);
  assert.equal(await page.getByRole("button", { name: "重新检测", exact: true }).isEnabled(), true);
  checks.push("首次检测失败结束占位，显示错误与重试入口，不虚构可用 Agent");
  assert.deepEqual(errors, []);
  writeFileSync(`${out}/ui-result.json`, JSON.stringify({ checks, errors, openSamples }, null, 2));
  console.log(JSON.stringify({ passed: checks.length, checks, openSamples }, null, 2));
} catch (error) {
  await page.screenshot({ path: `${out}/failure.png` });
  throw error;
} finally { await browser.close(); }
