import assert from "node:assert/strict";
import { chromium } from "playwright";

const base = process.env.CREWVELA_UI_URL || "http://127.0.0.1:1420";
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.addInitScript(() => {
    let allowed = false;
    window.__qaHealthy = false;
    window.__TAURI_INTERNALS__ = { invoke: async (command, args) => {
      if (command === "ai_memory_status") return {
        installed: true, executable: "C:/app/ai-memory.exe", dataDir: "C:/app/data",
        serverUrl: "http://127.0.0.1:47374", serverRunning: window.__qaHealthy,
        healthError: window.__qaHealthy ? null : "记忆服务认证失败（HTTP 401）",
        healthState: window.__qaHealthy ? "ready" : "authentication_error", canStart: false,
      };
      if (command === "plugin:dialog|open") return "C:/projects/alpha";
      if (command === "ai_memory_project_access") {
        if (typeof args.grant === "boolean") allowed = args.grant;
        return allowed;
      }
      if (command === "ai_memory_zcode_sessions") return ["1", "2"].map((n, i) => ({
        id: "22222222-abcd-4abc-bbbb-" + n.repeat(12), cwd: args.projectDir, startedAt: new Date(1000 + i).toISOString(),
      }));
      throw Error("unexpected command: " + command);
    }};
  });
  await page.goto(new URL("/tests/fixtures/shared-memory-repair.html", base).href);
  await page.getByText("记忆服务认证失败（HTTP 401）", { exact: true }).waitFor();
  assert.equal(await page.getByRole("button", { name: "启动服务", exact: true }).count(), 0);
  await page.evaluate(() => { window.__qaHealthy = true; });
  await page.getByRole("button", { name: "重新检测", exact: true }).click();
  await page.getByText("服务运行中", { exact: true }).first().waitFor();
  await page.getByRole("button", { name: "接入配置", exact: true }).click();
  await page.getByRole("button", { name: "选择项目目录", exact: true }).click();
  await page.getByRole("button", { name: "授权所选项目", exact: true }).click();
  await page.getByRole("button", { name: "撤销附加授权", exact: true }).click();
  await page.getByRole("button", { name: "授权所选项目", exact: true }).waitFor();
  await page.getByText("常见问题与手动操作", {exact: true}).click();
  await page.getByRole("button", { name: "查询未结束的 ZCode 会话" }).click();
  const selector = page.getByLabel("目标记忆会话");
  await selector.waitFor();
  const finalize = page.getByRole("button", { name: "复制指定会话收尾命令" });
  assert.equal(await finalize.count(), 0);
  await selector.selectOption({index: 1});
  await finalize.waitFor();
  const command = await page.locator("pre").filter({hasText: "finalize-session"}).innerText();
  assert.match(command, /--session-id .*111111111111/);
  assert.match(command, /47374/);
  assert.doesNotMatch(command, /222222222222/);
  await selector.selectOption("");
  assert.equal(await finalize.count(), 0);
  assert.deepEqual(errors, []);
  console.log("PASS: auth failure blocks startup; project consent is reversible; only the selected ZCode session can be finalized");
} finally { await browser.close(); }
