import { createRoot } from "react-dom/client";
import App from "../../src/App";
import type { SessionRecord } from "../../src/types";
import "../../src/styles.css";
import "../../src/styles/cowork.css";

let sessions: SessionRecord[] = ["one", "two"].map((id, i) => ({
  id, title: `fixture-${id}`, kind: "terminal", shell: "powershell", claudeSessionId: "",
  providerId: "local-terminal", providerName: "PowerShell", cwd: "C:\\fixture",
  status: "stopped", ptyId: null, yolo: false, autoMode: false, permissionMode: null,
  titleLocked: true, exitCode: 0, lastError: null, createdAt: i, updatedAt: i,
}));
if (new URLSearchParams(location.search).has("retired-runtime")) {
  sessions[0] = { ...sessions[0], agentRuntimeKind: "removed_runtime" as SessionRecord["agentRuntimeKind"] };
}
const pending: Array<{ id: string; resolve: (value: SessionRecord) => void; reject: (reason: Error) => void }> = [];
const calls: string[] = [];
const callbacks = new Map<number, (event: unknown) => void>();
const listeners = new Map<string, number>();
let providers = [{ id: "fixture-provider", name: "Fixture provider", baseUrl: "https://example.com", authTokenRef: "fixture", env: {}, builtin: false, timeoutMs: null, disableNonessentialTraffic: false }];
// 轮询命令的返回内容由测试控制；每次都是新对象，键顺序也随机（Rust HashMap 的序列化顺序就是如此）。
const shuffledKeys = (value: Record<string, unknown>) => Object.fromEntries(Object.entries(value).sort(() => Math.random() - 0.5));
const api = {
  calls, pending, rejectProviderDelete: true,
  rejectSwitchDismiss: false, switchCommands: [] as string[],
  usages: {} as Record<string, unknown>,
  switchRecords: [] as unknown[],
  registerCallback(fn: (event: unknown) => void) { const id = callbacks.size + 1; callbacks.set(id, fn); return id; },
  emit(event: string, payload: unknown) { callbacks.get(listeners.get(event)!)?.({ event, payload }); },
  release(id: string, failure?: string) {
    const index = pending.findIndex(item => item.id === id);
    if (index < 0) throw new Error(`Missing pending resume: ${id}`);
    const entry = pending.splice(index, 1)[0];
    if (failure) entry.reject(new Error(failure));
    else {
      const resumed: SessionRecord = { ...sessions.find(item => item.id === id)!, ptyId: id === "one" ? 1 : 2, status: "running" };
      sessions = sessions.map(item => item.id === id ? resumed : item);
      entry.resolve(resumed);
    }
  },
  async invoke(command: string, args?: Record<string, unknown>) {
    if (command === "list_providers") return providers;
    if (command === "delete_provider") {
      if (api.rejectProviderDelete) throw Error("fixture provider delete rejected");
      providers = []; return null;
    }
    if (command === "resume_session") { calls.push(String(args?.sessionId)); return new Promise<SessionRecord>((resolve, reject) => pending.push({ id: String(args?.sessionId), resolve, reject })); }
    if (["list_sessions", "session_snapshot"].includes(command)) return sessions;
    if (command === "pty_poll_all") return { sessions, chunks: [] };
    if (command === "get_runtime_preferences") return { disabledRuntimes: [] };
    if (command === "session_usages") return shuffledKeys(structuredClone(api.usages));
    if (command === "session_switch_list") return structuredClone(api.switchRecords);
    if (command === "list_archived_sessions" && location.search.includes("handoff-recovery")) return [(api.switchRecords[0] as any).source];
    if (command === "session_switch_dismiss_recovery") {
      api.switchCommands.push(command);
      if (api.rejectSwitchDismiss) throw Error("fixture dismiss failed");
      const index = api.switchRecords.findIndex(item => (item as any).id === args?.id);
      if (index < 0) throw Error("missing fixture handoff");
      const updated = { ...(api.switchRecords[index] as any), recoveryDismissed: true };
      api.switchRecords[index] = updated;
      localStorage.setItem("fixture-handoff-record", JSON.stringify(updated));
      return structuredClone(updated);
    }
    if (command === "plugin:event|listen") {
      if (new URLSearchParams(location.search).has("event-error") && ["multi-agent-workspace-open", "browser-control-panel-needed"].includes(String(args?.event))) {
        throw Error(`fixture listener rejected: ${args?.event}`);
      }
      listeners.set(String(args?.event), Number(args?.handler)); return 1;
    }
    if (command.startsWith("list_") || command === "session_switch_list") return [];
    return null;
  },
};
if (location.search.includes("handoff-recovery")) {
  const saved = localStorage.getItem("fixture-handoff-record");
  api.switchRecords = [saved ? JSON.parse(saved) : {
    id: "stale-handoff", source: { ...sessions[0], kind: "claude", shell: null, agentRuntimeKind: "claude_code", archivedAt: 123, title: "已归档的接力来源" },
    sourcePosition: 0, sourcePty: null, targetId: "deleted-target", threadId: null,
    targetRuntime: "codex", state: "queued", historyMode: "handoff", context: "fixture history retained", error: null,
  }];
  sessions = sessions.filter(item => item.id !== "one");
}
Object.assign(window, { appSessionMock: api });
localStorage.setItem("crewvela-notification-settings-v1", JSON.stringify({ enabled: false }));
localStorage.setItem("crewvela-workspace-tabs", JSON.stringify({ tabs: [{ id: "a", title: "工作区 A" }, { id: "b", title: "工作区 B" }], activeTabId: "a", assignments: { one: "a", two: "a" } }));
createRoot(document.getElementById("root")!).render(<App />);
