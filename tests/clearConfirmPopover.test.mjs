import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { readStyles } from "./helpers/loadStyles.mjs";

const styles = readStyles();
const control = readFileSync(
  new URL("../src/components/MultiAgentControl.tsx", import.meta.url),
  "utf8",
);

test("清空确认在消息区域独立展示，保留可访问确认和取消", () => {
  assert.match(styles, /\.multi-agent-clear-confirm\s*\{[^}]*max-width:\s*100%/s);
  assert.match(control, /role="alertdialog"/);
  assert.match(control, /aria-label="确认清空对话记录/);
  assert.match(control, /清空对话？/);
  assert.match(styles, /\.clear-confirm-actions button\.danger\s*\{[^}]*background:\s*var\(--danger\)/s);
  const confirmIndex = control.indexOf('{clearConversationConfirmOpen &&');
  const coordinatorIndex = control.indexOf('className={`multi-agent-coordinator');
  const composerIndex = control.indexOf('className="multi-agent-composer"');
  assert.ok(confirmIndex > coordinatorIndex && confirmIndex < composerIndex, "确认位于消息区域，不受输入栏窄目标控件挤压");
  assert.match(control.slice(confirmIndex, composerIndex), /onClick=\{clearCoordinatorConversation\}/);
  assert.match(control.slice(confirmIndex, composerIndex), /setClearConversationConfirmOpen\(false\)/);
});
