import test from "node:test";
import assert from "node:assert/strict";

import {
  COMPUTER_CONTROL_COVERAGE,
  COMPUTER_CONTROL_PREVIEW,
  COMPUTER_CONTROL_SAFETY_NOTE,
  DEFAULT_COMPUTER_CONTROL_SETTINGS,
} from "../src/computerControlSettings.ts";

test("默认关闭,预览开启,形态同构", () => {
  assert.equal(DEFAULT_COMPUTER_CONTROL_SETTINGS.enabled, false);
  assert.equal(COMPUTER_CONTROL_PREVIEW.settings.enabled, true);
  assert.ok(COMPUTER_CONTROL_PREVIEW.runtime.includes("powershell"));
  assert.equal(COMPUTER_CONTROL_PREVIEW.warning, null);
});

test("覆盖与安全说明覆盖关键事实", () => {
  assert.ok(COMPUTER_CONTROL_COVERAGE.includes("原生聊天"));
  assert.ok(COMPUTER_CONTROL_COVERAGE.includes("Windows"));
  assert.ok(COMPUTER_CONTROL_SAFETY_NOTE.includes("真实"));
  assert.ok(COMPUTER_CONTROL_SAFETY_NOTE.includes("模型提供方"));
});
