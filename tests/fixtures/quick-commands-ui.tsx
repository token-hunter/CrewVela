// Fixture-only IPC: never launches agents, sends network traffic or reads user data.
import { useState } from "react";
import { createRoot } from "react-dom/client";
import { QuickCommandCenter } from "../../src/components/QuickCommandCenter";
import { SettingsDialog } from "../../src/components/SettingsDialog";
import { DEFAULT_APPEARANCE } from "../../src/appearance";
import { DEFAULT_PET_SETTINGS } from "../../src/petSettings";
import { DEFAULT_NOTIFICATION_SETTINGS } from "../../src/notificationSettings";
import type { SessionRecord } from "../../src/types";
import "../../src/styles.css";
const params = new URLSearchParams(location.search);
const theme = params.get("theme") === "light" ? "light" : "dark";
document.documentElement.dataset.theme = theme;
let sessions = [
  { id: "claude", title: "Claude · 实现", kind: "claude", agentRuntimeKind: "claude_code", status: "running", ptyId: 1, cwd: "/fixture/project", projectId: "project-a", providerId: "local", providerName: "Claude Code" },
  { id: "codex", title: "Codex · 审查", kind: "terminal", agentRuntimeKind: "codex_chat", status: "completed", ptyId: null, cwd: "/fixture/project", projectId: "project-a", providerId: "local", providerName: "Codex Chat" },
  { id: "outside", title: "其他工作区 · Codex", kind: "terminal", agentRuntimeKind: "codex", status: "running", ptyId: 2, cwd: "/fixture/project", projectId: "other", providerId: "local", providerName: "Codex CLI" },
] as SessionRecord[];
const calls: any[] = [];
const evidence: any[] = [];
(window as any).__quickCalls = calls;
(window as any).__TAURI_INTERNALS__ = { transformCallback: () => 1, async invoke(method: string, args: any) {
  calls.push({ method, args });
  if (method === "ma_probe_runtimes") return ["claude_code", "codex"].map(kind => ({ kind, installed: true, error: null }));
  if (method === "session_snapshot") return structuredClone(sessions);
  if (method === "get_key_state") return false;
  if (method === "list_communication_evidence") return evidence;
  if (method === "send_session_message") {
    if (params.get("fail") === "1" && args.sessionId === "codex") throw Error("隔离测试：目标投递失败");
    evidence.push({ id: args.requestId, toSessionId: args.sessionId, stage: "text_written" }); return "accepted";
  }
  if (method === "delete_session") { sessions = sessions.filter(s => s.id !== args.sessionId); return; }
  if (method === "stop_session") { sessions = sessions.map(s => s.id === args.sessionId ? { ...s, status: "stopped", ptyId: null } : s); return; }
  if (method === "launch_runtime_session") { const row = { ...sessions[0], id: `created-${sessions.length}`, title: "新建会话", agentRuntimeKind: args.runtimeKind }; sessions.push(row); return row; }
  throw Error(`隔离页面拒绝未模拟的接口：${method}`);
} };
const context = { workspaceSessionIds: ["claude", "codex"], projectId: "project-a", cwd: "/fixture/project" };
function Fixture() {
  const [open, setOpen] = useState(params.get("open") !== "0");
  const [settings, setSettings] = useState(params.get("settings") === "1");
  const [rows, setRows] = useState(sessions);
  return <div style={{ padding: 32 }}>
    <button onClick={() => setOpen(true)}>打开快捷指令</button>
    <button onClick={() => setSettings(true)}>打开设置</button>
    {settings && <SettingsDialog sessions={rows} archivedSessions={[]} projects={[]} providers={[]} keyStates={{}} usageByProvider={{}}
      appearance={{ ...DEFAULT_APPEARANCE, theme }} resolvedAppearanceTheme={theme} petSettings={DEFAULT_PET_SETTINGS} notificationSettings={DEFAULT_NOTIFICATION_SETTINGS}
      onAppearanceChange={() => {}} onPetSettingsChange={() => {}} onNotificationSettingsChange={() => {}}
      onEdit={() => {}} onNew={() => {}} onDelete={() => {}} onSetHidden={() => {}} onRestoreDefault={() => {}} onReorder={async () => {}} onRestoreArchivedSession={() => {}} onDeleteArchivedSession={() => {}} onViewArchivedTranscript={() => {}}
      quickContext={context} onSessionsChange={setRows} initialSection="quickCommands" onClose={() => setSettings(false)} />}
    {open && <QuickCommandCenter sessions={rows} providers={[]} context={context} onClose={() => setOpen(false)} onSessionsChange={setRows} onOpenSettings={() => { setOpen(false); setSettings(true); }} />}
  </div>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
