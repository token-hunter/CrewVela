import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const multiTerminal = readFileSync(
  new URL("../src/components/MultiTerminal.tsx", import.meta.url),
  "utf8",
);
const terminalPane = readFileSync(
  new URL("../src/components/TerminalPane.tsx", import.meta.url),
  "utf8",
);

test("会话标题栏不再显示上下文与用量弹窗入口", () => {
  assert.doesNotMatch(terminalPane, /SessionTelemetryPopover/);
  assert.doesNotMatch(terminalPane, /onRequestTelemetry/);
  assert.doesNotMatch(multiTerminal, /session_telemetry/);
  assert.doesNotMatch(multiTerminal, /telemetryBySession/);
});

test("终端状态行仍由后端 Claude statusLine 注入，不依赖标题栏弹窗", () => {
  const ptySource = readFileSync(
    new URL("../src-tauri/src/pty.rs", import.meta.url),
    "utf8",
  );
  assert.match(ptySource, /build_interactive_settings_json/);
  assert.match(ptySource, /statusLine/);
  assert.match(ptySource, /status_line_command/);
});
