// Real React UI with isolated IPC; validates web behavior, not a native Agent run.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";
const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1447";
const out = ".scratch/quick-command-center";
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ channel: "chrome", headless: true });
const checks = [];
const errors = [];
const fixture = "/tests/fixtures/quick-commands-ui.html?ui-preview=1";
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
const page = await context.newPage();
page.on("pageerror", e => errors.push(e.message));
const callCount = method => page.evaluate(method => window.__quickCalls.filter(c => c.method === method).length, method);
const waitReady = async () => { await page.getByRole("button", { name: "启动 Codex CLI 本地配置", exact: true }).waitFor(); };
try {
  await page.goto(base + fixture); await waitReady();
  assert.equal(await page.getByRole("checkbox").count(), 2);
  assert.equal(await page.getByRole("button", { name: "确认发送", exact: true }).isEnabled(), false);
  await page.getByRole("button", { name: "选择此范围全部", exact: true }).click();
  await page.screenshot({ path: `${out}/palette-dark.png` });
  await page.getByRole("button", { name: "确认发送", exact: true }).click();
  await page.getByRole("region", { name: "执行结果" }).getByText("内容已写入", { exact: true }).first().waitFor();
  assert.equal(await callCount("send_session_message"), 2);
  assert.equal(await callCount("interpret_control_message"), 0);
  const ids = await page.evaluate(() => window.__quickCalls.filter(c => c.method === "send_session_message").map(c => c.args.sessionId));
  assert.deepEqual(ids, ["claude", "codex"]);
  checks.push("结构化发送覆盖 CLI 与空闲原生聊天，精确工作区排除同目录会话，状态不冒充任务完成");

  await page.getByRole("button", { name: "新建自定义指令", exact: true }).click();
  await page.getByLabel("指令名称", { exact: true }).fill("每日进度");
  await page.getByRole("textbox", { name: "消息内容", exact: true }).fill("请汇报进度和阻塞项");
  await page.getByRole("button", { name: "保存指令", exact: true }).click();
  await page.getByRole("button", { name: "收藏 每日进度", exact: true }).click();
  await page.getByRole("button", { name: "配置", exact: true }).click();
  const nav = page.getByRole("navigation", { name: "设置分类" });
  const order = await nav.getByRole("button").allTextContents();
  assert.equal(order.indexOf("快捷指令"), order.indexOf("自动化") + 1);
  await page.getByRole("tab", { name: "收藏", exact: true }).click();
  await page.getByRole("button", { name: "每日进度 自定义", exact: true }).click();
  assert.equal(await page.getByRole("textbox", { name: "消息内容", exact: true }).inputValue(), "请汇报进度和阻塞项");
  await page.screenshot({ path: `${out}/settings-dark.png` });
  await page.reload(); await waitReady();
  await page.getByRole("tab", { name: "收藏", exact: true }).click();
  assert.equal(await page.getByRole("button", { name: "每日进度 自定义", exact: true }).count(), 1);
  checks.push("配置入口位于自动化下方，两处共享自定义与收藏，刷新持久保留");

  await page.getByRole("tab", { name: "全部", exact: true }).click();
  await page.getByRole("button", { name: "启动 Codex CLI 本地配置", exact: true }).click();
  await page.getByRole("textbox", { name: "工作目录", exact: true }).fill("/fixture/project with spaces");
  await page.getByRole("spinbutton", { name: "启动数量", exact: true }).fill("2");
  await page.getByRole("button", { name: "另存为自定义", exact: true }).click();
  await page.getByLabel("指令名称", { exact: true }).fill("启动项目审查");
  await page.getByRole("button", { name: "保存指令", exact: true }).click();
  await page.getByRole("button", { name: "启动 Claude Code 本地配置", exact: true }).click();
  await page.getByRole("button", { name: "启动项目审查 自定义", exact: true }).click();
  assert.equal(await page.getByRole("textbox", { name: "工作目录", exact: true }).inputValue(), "/fixture/project with spaces");
  assert.equal(await page.getByRole("spinbutton", { name: "启动数量", exact: true }).inputValue(), "2");
  await page.getByRole("button", { name: "确认启动", exact: true }).click();
  await page.getByRole("region", { name: "执行结果" }).getByText("已启动", { exact: true }).first().waitFor();
  assert.equal(await callCount("launch_runtime_session"), 2);
  checks.push("本地 CLI 启动指令可保存目录和数量，执行走已有 Runtime 接口");

  await page.getByRole("button", { name: "批量停止会话 停止会话", exact: true }).click();
  await page.getByRole("button", { name: "选择此范围全部", exact: true }).click();
  await page.getByRole("button", { name: "核对批量操作", exact: true }).click();
  assert.equal(await callCount("stop_session"), 0);
  await page.getByRole("button", { name: "取消操作", exact: true }).click();
  assert.equal(await callCount("stop_session"), 0);
  await page.getByRole("button", { name: "核对批量操作", exact: true }).click();
  await page.getByRole("checkbox").first().uncheck();
  assert.equal(await page.getByRole("group", { name: "批量操作二次确认" }).count(), 0);
  await page.getByRole("button", { name: "核对批量操作", exact: true }).click();
  await page.getByRole("button", { name: "确认停止所选会话", exact: true }).click();
  await page.getByRole("region", { name: "执行结果" }).getByText("已停止", { exact: true }).waitFor();
  assert.equal(await callCount("stop_session"), 1);
  checks.push("批量停止先预览再确认；取消无副作用，更换目标撤销旧确认");

  await page.getByRole("button", { name: "批量从列表删除会话 从列表删除会话", exact: true }).click();
  await page.getByRole("button", { name: "选择此范围全部", exact: true }).click();
  await page.getByRole("button", { name: "核对批量操作", exact: true }).click();
  assert.equal(await callCount("delete_session"), 0);
  await page.getByRole("button", { name: "确认删除所选会话", exact: true }).click();
  await page.getByRole("region", { name: "执行结果" }).getByText("已从列表删除", { exact: true }).first().waitFor();
  assert.equal(await callCount("delete_session"), 2);
  checks.push("删除覆盖停止会话，逐目标执行并保留未选工作区");

  await page.getByRole("button", { name: "关闭快捷指令", exact: true }).click();
  await page.getByRole("button", { name: "打开快捷指令", exact: true }).click();
  await page.getByRole("textbox", { name: "搜索快捷指令", exact: true }).press("Escape");
  assert.equal(await page.getByRole("dialog", { name: "快捷指令", exact: true }).count(), 0);
  assert.equal(await page.getByRole("button", { name: "打开快捷指令", exact: true }).evaluate(el => el === document.activeElement), true);
  checks.push("Esc 关闭和焦点归还正常");

  await page.goto(base + fixture + "&fail=1"); await waitReady();
  await page.getByRole("button", { name: "选择此范围全部", exact: true }).click();
  await page.getByRole("button", { name: "确认发送", exact: true }).click();
  await page.getByRole("region", { name: "执行结果" }).getByText("失败", { exact: true }).waitFor();
  assert.equal(await callCount("send_session_message"), 2);
  checks.push("单目标投递失败显式显示，其他目标保留独立成功证据");

  for (const theme of ["dark", "light"]) for (const size of [{ width: 1440, height: 1000 }, { width: 760, height: 600 }]) {
    await page.setViewportSize(size);
    await page.goto(base + fixture + `&theme=${theme}`); await waitReady();
    const geometry = await page.locator(".quick-center").evaluate(el => ({ x: el.getBoundingClientRect().x, right: el.getBoundingClientRect().right, bottom: el.getBoundingClientRect().bottom, overflow: el.scrollWidth > el.clientWidth + 1, width: innerWidth, height: innerHeight }));
    assert.ok(geometry.x >= 0 && geometry.right <= geometry.width && geometry.bottom <= geometry.height && !geometry.overflow, JSON.stringify(geometry));
    await page.screenshot({ path: `${out}/palette-${theme}-${size.width}.png` });
  }
  checks.push("深浅主题与窄窗口没有横向溢出");
  assert.deepEqual(errors, []);
  writeFileSync(`${out}/ui-result.json`, JSON.stringify({ checks, errors }, null, 2));
  console.log(JSON.stringify({ passed: checks.length, checks }, null, 2));
} catch (error) {
  await page.screenshot({ path: `${out}/failure.png` });
  writeFileSync(`${out}/failure.html`, await page.content());
  throw error;
} finally { await browser.close(); }
