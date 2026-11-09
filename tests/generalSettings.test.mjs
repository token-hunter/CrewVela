import test from "node:test";
import assert from "node:assert/strict";

import {
  DEFAULT_GENERAL_SETTINGS,
  GENERAL_SETTINGS_PREVIEW,
  MAX_TERMINAL_FONT_SIZE,
  MIN_TERMINAL_FONT_SIZE,
  validateGeneralSettings,
} from "../src/generalSettings.ts";

test("默认值走迁移安全路线（代理留空、行为开关默认开）", () => {
  assert.equal(DEFAULT_GENERAL_SETTINGS.httpProxy, "");
  assert.equal(DEFAULT_GENERAL_SETTINGS.httpNoProxy, "");
  assert.equal(DEFAULT_GENERAL_SETTINGS.httpCaCertPath, "");
  assert.equal(DEFAULT_GENERAL_SETTINGS.closeToTray, true);
  assert.equal(DEFAULT_GENERAL_SETTINGS.keepAwake, true);
  assert.equal(DEFAULT_GENERAL_SETTINGS.webview2HardwareAcceleration, true);
  assert.equal(DEFAULT_GENERAL_SETTINGS.terminalFontSize, 12);
});

test("validateGeneralSettings 接受空配置与合法 http 代理", () => {
  assert.deepEqual(validateGeneralSettings(DEFAULT_GENERAL_SETTINGS), []);
  assert.deepEqual(
    validateGeneralSettings({
      ...DEFAULT_GENERAL_SETTINGS,
      httpProxy: "http://127.0.0.1:7890",
      httpNoProxy: ".corp.com,*.internal",
      terminalFontSize: 20,
    }),
    [],
  );
});

test("validateGeneralSettings 拒绝不支持的代理协议与残缺 URL", () => {
  const socks = validateGeneralSettings({
    ...DEFAULT_GENERAL_SETTINGS,
    httpProxy: "socks5://127.0.0.1:1080",
  });
  assert.equal(socks.length, 1);
  assert.match(socks[0], /http:\/\/ 或 https:\/\//);

  const bare = validateGeneralSettings({
    ...DEFAULT_GENERAL_SETTINGS,
    httpProxy: "127.0.0.1:7890",
  });
  assert.equal(bare.length, 1);

  const https = validateGeneralSettings({
    ...DEFAULT_GENERAL_SETTINGS,
    httpProxy: "https://proxy.internal:8443",
  });
  assert.deepEqual(https, []);
});

test("validateGeneralSettings 校验终端字号范围", () => {
  for (const bad of [
    MIN_TERMINAL_FONT_SIZE - 1,
    MAX_TERMINAL_FONT_SIZE + 1,
    12.5,
    Number.NaN,
  ]) {
    const errors = validateGeneralSettings({
      ...DEFAULT_GENERAL_SETTINGS,
      terminalFontSize: bad,
    });
    assert.equal(
      errors.length,
      1,
      `字号 ${bad} 应被拒绝`,
    );
    assert.match(errors[0], /终端字号/);
  }
  assert.deepEqual(
    validateGeneralSettings({
      ...DEFAULT_GENERAL_SETTINGS,
      terminalFontSize: MIN_TERMINAL_FONT_SIZE,
    }),
    [],
  );
  assert.deepEqual(
    validateGeneralSettings({
      ...DEFAULT_GENERAL_SETTINGS,
      terminalFontSize: MAX_TERMINAL_FONT_SIZE,
    }),
    [],
  );
});

test("ui-preview 假数据自身必须通过校验", () => {
  assert.deepEqual(validateGeneralSettings(GENERAL_SETTINGS_PREVIEW.settings), []);
});
