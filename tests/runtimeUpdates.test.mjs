import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const backend = readFileSync(new URL("../src-tauri/src/multi_agent/runtime.rs", import.meta.url), "utf8");
const commands = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
const panel = readFileSync(new URL("../src/components/CollaborationSettingsPanel.tsx", import.meta.url), "utf8");

test("Runtime 更新覆盖检查、单项升级、批量升级和运行中确认", () => {
  for (const name of ["ma_check_runtime_updates", "ma_upgrade_runtime"]) {
    assert.match(commands, new RegExp(`async fn ${name}`));
    assert.match(commands, new RegExp(`\\s${name},`));
    assert.match(panel, new RegExp(`"${name}"`));
  }
  assert.match(panel, /升级全部/);
  assert.match(panel, /busyRuntimeKinds/);
  assert.match(panel, /window\.confirm/);
});

test("无法静默升级的 Runtime 提供手动命令", () => {
  assert.match(backend, /KimiCode[\s\S]*display: "kimi upgrade"[\s\S]*automatic: false/);
  assert.match(panel, /复制命令/);
});
