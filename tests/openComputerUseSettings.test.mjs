import test from "node:test";
import assert from "node:assert/strict";

import {
  DEFAULT_OPEN_COMPUTER_USE_SETTINGS,
  OPEN_COMPUTER_USE_COVERAGE,
  OPEN_COMPUTER_USE_PREVIEW,
  OPEN_COMPUTER_USE_SAFETY_NOTE,
} from "../src/openComputerUseSettings.ts";

test("open-computer-use 默认关闭,预览开启,形态同构", () => {
  assert.equal(DEFAULT_OPEN_COMPUTER_USE_SETTINGS.enabled, false);
  assert.equal(OPEN_COMPUTER_USE_PREVIEW.settings.enabled, true);
  assert.ok(OPEN_COMPUTER_USE_PREVIEW.runtime.includes("open-computer-use.exe"));
  assert.equal(OPEN_COMPUTER_USE_PREVIEW.warning, null);
});

test("open-computer-use 覆盖与安全说明覆盖关键事实", () => {
  assert.ok(OPEN_COMPUTER_USE_COVERAGE.includes("open-computer-use"));
  assert.ok(OPEN_COMPUTER_USE_COVERAGE.includes("npm i -g open-computer-use"));
  assert.ok(OPEN_COMPUTER_USE_COVERAGE.includes("无障碍"));
  assert.ok(OPEN_COMPUTER_USE_SAFETY_NOTE.includes("真实"));
  assert.ok(OPEN_COMPUTER_USE_SAFETY_NOTE.includes("第三方开源"));
});
