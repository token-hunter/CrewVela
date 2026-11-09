import test from "node:test";
import assert from "node:assert/strict";

import {
  DEFAULT_KIMI_COMPUTER_USE_SETTINGS,
  KIMI_COMPUTER_USE_COVERAGE,
  KIMI_COMPUTER_USE_PREVIEW,
  KIMI_COMPUTER_USE_SAFETY_NOTE,
} from "../src/kimiComputerUseSettings.ts";

test("kimi-computer-use 默认关闭,预览开启,形态同构", () => {
  assert.equal(DEFAULT_KIMI_COMPUTER_USE_SETTINGS.enabled, false);
  assert.equal(KIMI_COMPUTER_USE_PREVIEW.settings.enabled, true);
  assert.ok(KIMI_COMPUTER_USE_PREVIEW.runtime.includes("kimi-cu.exe"));
  assert.equal(KIMI_COMPUTER_USE_PREVIEW.warning, null);
});

test("kimi-computer-use 覆盖与安全说明覆盖关键事实", () => {
  assert.ok(KIMI_COMPUTER_USE_COVERAGE.includes("kimi-cu"));
  assert.ok(KIMI_COMPUTER_USE_COVERAGE.includes("Windows"));
  assert.ok(KIMI_COMPUTER_USE_COVERAGE.includes("KimiCU"));
  assert.ok(KIMI_COMPUTER_USE_SAFETY_NOTE.includes("真实"));
  assert.ok(KIMI_COMPUTER_USE_SAFETY_NOTE.includes("第三方"));
});
