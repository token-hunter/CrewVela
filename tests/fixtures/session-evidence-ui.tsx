import { useState } from "react";
import { createRoot } from "react-dom/client";
import { HandoffSourceView } from "../../src/components/HandoffSourceView";
import { SessionMessageReceipts } from "../../src/components/ConversationReceiptPopover";
import { CodexChat } from "../../src/components/CodexChat";
import type { HarnessSessionSnapshot, SessionRecord } from "../../src/types";
import type { SessionSwitchRecord } from "../../src/sessionSwitch";
import "../../src/styles.css";

document.documentElement.dataset.theme = new URLSearchParams(location.search).get("theme") ?? "dark";
const session = (id: string, agentRuntimeKind: SessionRecord["agentRuntimeKind"], title: string) =>
  ({ id, agentRuntimeKind, kind: "terminal", title, cwd: "F:/fixture/unit-converter", ptyId: null, status: "stopped", providerName: title }) as SessionRecord;
const claude = session("fixture-claude", "claude_chat", "Claude Code · 原会话");
const codex = session("fixture-codex", "codex_chat", "Codex");
const relay: SessionSwitchRecord = {
  id: "forward", source: claude, sourcePosition: 0, sourcePty: null, targetId: codex.id, threadId: null,
  targetRuntime: "codex_chat", preserveSource: true, state: "queued", historyMode: "handoff", context: "", error: null,
};
const returnDraft: SessionSwitchRecord = {
  ...relay, id: "return", source: codex, targetId: claude.id, targetRuntime: "claude_chat", state: "preview", returnOf: relay.id,
  sourceHistory: [
    { role: "assistant", text: "零值用例还需补测\n建议覆盖整数和小数。", at: null },
    { role: "user", text: "输出保留三位小数", at: null },
  ],
};
const now = new Date("2026-11-22T10:24:00+08:00").getTime();
const receipt = {
  id: "fixture-1", sender: claude.id, target: codex.id, body: "请复查零值输入的边界情况",
  stage: "replied", queuedAt: now, submittedAt: null, receivedAt: now + 60_000, replyAt: now + 240_000, reply: "零值可正常换算。",
};
const snapshot: HarnessSessionSnapshot = {
  sessionId: codex.id, connected: true, busy: false, pendingPermission: null, toolCalls: [],
  sequence: 1, controlState: {}, commands: [], configOptions: [],
  history: [{ id: "turn", role: "user", text: "请补充零值输入的测试", createdAt: now },
    { id: "turn", role: "assistant", text: "已补充零值测试并更新说明。", createdAt: now + 240_000 }],
  turnChanges: [["turn", {
    files: [
      { path: "tests/converter.test.js", patch: "@@ -6,2 +6,5 @@\n test('valid', () => {});\n+test('zero', () => {\n+  assert.equal(convert(0), 0);\n+});" },
      { path: "README.md", patch: "@@ -1 +1,2 @@\n # 单位换算器\n+支持输入零值。" },
    ],
    checks: [{ command: "node --test", exitCode: 0 }], limited: false,
  }]],
};
let callback = 0;
Object.assign(window, {
  __TAURI_INTERNALS__: {
    transformCallback: () => ++callback,
    invoke(command: string) {
      if (command === "list_session_message_receipts") return Promise.resolve([receipt]);
      if (command === "session_switch_return_preview") return Promise.resolve(returnDraft);
      if (command === "session_switch_confirm") return Promise.resolve({ ...returnDraft, state: "queued" });
      if (command === "session_switch_restore") return Promise.resolve({ ...returnDraft, state: "cancelled" });
      if (command === "session_switch_history") return Promise.resolve([{ role: "assistant", text: "上一次已完成换算规则。", at: null }]);
      if (command === "codex_chat_snapshot" || command === "start_codex_chat_session") return Promise.resolve(snapshot);
      return Promise.resolve(null);
    },
  },
  __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} },
});
function Fixture() {
  const [selected, setSelected] = useState(new URLSearchParams(location.search).get("view") ?? "handoff");
  const [result, setResult] = useState("");
  return <main style={{ height: "100svh", display: "flex", flexDirection: "column", minWidth: 0, maxWidth: "68rem", padding: "12px", margin: "auto" }}>
    <nav style={{ display: "flex", gap: "8px", padding: "8px" }}>
      <button type="button" onClick={() => setSelected("handoff")}>增量交接</button>
      <button type="button" onClick={() => setSelected("receipt")}>消息回执</button>
      <button type="button" onClick={() => setSelected("diff")}>逐轮差异</button>
    </nav>
    {result && <p role="status">{result}</p>}
    {selected === "handoff" && <HandoffSourceView session={claude} record={relay} target={codex}
      onRecord={value => { if (value.state === "queued") setResult(`恢复目标 ${value.targetId}`); }}
      onSelectTarget={() => {}} onDetails={() => {}} />}
    {selected === "receipt" && <SessionMessageReceipts sessionId={claude.id}
      sessionName={id => id === codex.id ? codex.title : claude.title}
      onOpenSession={id => setResult(`打开原会话 ${id}`)} />}
    {selected === "diff" && <CodexChat session={codex} managedToolbar onSessionRecord={() => {}} />}
  </main>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
