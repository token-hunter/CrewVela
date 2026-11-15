import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { tmpdir } from "node:os";
import { join } from "node:path";

const BROWSER_PATHS = [
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
];

test("项目协作输入框固定在控制侧栏底部", () => {
  const browser = BROWSER_PATHS.find(existsSync);
  assert.ok(browser, "未找到 Chrome，无法执行真实浏览器布局测试");
  const fixture = new URL("./controlMultiAgentLayout.fixture.html", import.meta.url);
  const profile = mkdtempSync(join(tmpdir(), "crewvela-layout-browser-"));
  let result;
  try {
    result = spawnSync(
      browser,
      [
        "--headless=new",
        `--user-data-dir=${profile}`,
        "--disable-gpu",
        "--no-first-run",
        "--allow-file-access-from-files",
        "--window-size=430,900",
        "--dump-dom",
        fileURLToPath(fixture),
      ],
      { encoding: "utf8", timeout: 15_000 },
    );
  } finally { rmSync(profile, { recursive: true, force: true }); }
  assert.equal(result.status, 0, result.stderr || "Chrome 布局测试启动失败");
  const match = result.stdout.match(/<output id="layout-result">(-?\d+)<\/output>/);
  assert.ok(match, "没有读取到布局测量结果");
  const bottomGap = Number(match[1]);
  assert.ok(
    bottomGap >= 0 && bottomGap <= 16,
    `输入框没有贴底：距侧栏底部 ${bottomGap}px，期望在 0–16px 之间`,
  );
});
