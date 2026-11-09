import test from "node:test";
import assert from "node:assert/strict";

import {
  BROWSER_CHANNEL_OPTIONS,
  BROWSER_CONTROL_COVERAGE,
  BROWSER_CONTROL_PREVIEW,
  DEFAULT_BROWSER_CONTROL_SETTINGS,
  validateBrowserControlSettings,
} from "../src/browserControlSettings.ts";

test("validateBrowserControlSettings 校验自定义路径", () => {
  const base = { ...DEFAULT_BROWSER_CONTROL_SETTINGS, enabled: true, browser: "custom" };

  assert.equal(
    validateBrowserControlSettings(base),
    "选择自定义浏览器时必须填写可执行文件路径",
  );
  assert.equal(
    validateBrowserControlSettings({ ...base, customPath: "   " }),
    "选择自定义浏览器时必须填写可执行文件路径",
  );
  assert.equal(
    validateBrowserControlSettings({ ...base, customPath: "chrome.exe" }),
    "自定义浏览器路径必须是绝对路径",
  );
  // Windows / UNC / Unix 绝对路径均放行（存在性由后端校验）。
  assert.equal(
    validateBrowserControlSettings({ ...base, customPath: "C:\\Program Files\\Chrome\\chrome.exe" }),
    null,
  );
  assert.equal(validateBrowserControlSettings({ ...base, customPath: "\\\\server\\chrome.exe" }), null);
  assert.equal(validateBrowserControlSettings({ ...base, customPath: "/usr/bin/google-chrome" }), null);

  // 非自定义模式下路径不参与校验。
  assert.equal(
    validateBrowserControlSettings({ ...DEFAULT_BROWSER_CONTROL_SETTINGS, browser: "auto" }),
    null,
  );
});

test("渠道选项覆盖 auto/chrome/msedge/custom 且互不重复", () => {
  assert.deepEqual(
    BROWSER_CHANNEL_OPTIONS.map((option) => option.id),
    ["auto", "chrome", "msedge", "custom"],
  );
  for (const option of BROWSER_CHANNEL_OPTIONS) {
    assert.ok(option.label.length > 0);
    assert.ok(option.hint.length > 0);
  }
});

test("预览数据与覆盖说明可用（ui-preview 模式渲染依赖）", () => {
  assert.equal(BROWSER_CONTROL_PREVIEW.settings.enabled, true);
  assert.equal(BROWSER_CONTROL_PREVIEW.settings.preferPanel, true);
  assert.ok(BROWSER_CONTROL_PREVIEW.resolvedBrowser.includes("Chrome"));
  assert.ok(BROWSER_CONTROL_PREVIEW.engine.includes("@playwright/mcp"));
  assert.ok(BROWSER_CONTROL_PREVIEW.panel.includes("内置面板"));
  assert.ok(BROWSER_CONTROL_COVERAGE.includes("原生聊天"));
});
