import { useState } from "react";
import { createRoot } from "react-dom/client";
import { NotificationSettingsPanel } from "../../src/components/NotificationSettingsPanel";
import { DEFAULT_NOTIFICATION_SETTINGS } from "../../src/notificationSettings";
import { UnverifiedRunNotice } from "../../src/components/UnverifiedRunNotice";
import { ClaudeChat } from "../../src/components/ClaudeChat";
import { CodexChat } from "../../src/components/CodexChat";
import { KimiChat } from "../../src/components/KimiChat";
import { OpenCodeChat } from "../../src/components/OpenCodeChat";
import { PiChat } from "../../src/components/PiChat";
import { DeepSeekHarnessChat } from "../../src/components/DeepSeekHarnessChat";
import { ZCodeChat } from "../../src/components/ZCodeChat";
import { ProviderEditor } from "../../src/components/ProviderEditor";
import type { SessionRecord, ProviderConfig, AgentRun } from "../../src/types";
import "../../src/styles.css";
const components = { claude: ClaudeChat, codex: CodexChat, kimi: KimiChat, opencode: OpenCodeChat, pi: PiChat, harness: DeepSeekHarnessChat, zcode: ZCodeChat };
const pending: Array<{ command: string; args: any; resolve: (value: any) => void; reject: (reason: string) => void }> = [];
const calls: Array<{ command: string; args: any }> = [];
let file = 0, callbackId = 0;
const callbacks = new Map<number, (event: unknown) => void>();
const session = (kind: string): SessionRecord => ({ id: kind, kind: "terminal", agentRuntimeKind: "claude_chat", providerId: `local-runtime-${kind}`, providerName: kind, cwd: "F:/fixture", status: "running", title: kind, ptyId: null, shell: null, claudeSessionId: kind, yolo: false, autoMode: false, permissionMode: null, titleLocked: false, exitCode: null, lastError: null, createdAt: 0, updatedAt: 0 });
const snapshot = (sessionId: string) => ({ sessionId, connected: true, busy: false, history: [], pendingPermission: null, toolCalls: [], sequence: 0, controlState: {}, commands: [], configOptions: [] });
let queued = [{ queueItemId: "q1", sourceCommandId: "c1", text: "queued task", attachments: [{ ref: "/queued.txt", fileName: "queued.txt", mime: "text/plain", bytes: 10 }], dispatch: { state: "queued" } }];
const zcode = () => ({ ...snapshot("zcode"), rows: [], ...(new URLSearchParams(location.search).has("side") ? { sideDetail: { childSessionId: "side", rows: [], loading: false, error: null } } : {}), models: [{ id: "model", name: "fixture model" }], selectedModelId: "model", permissionMode: "build", queuedInputs: queued, recoverableInputs: [], queueAutoDrain: false, canQueue: true, inputRouting: "startNow", sequence: 10 + calls.filter(item => item.command === "zcode_queue_action").length });
const result = (command: string, args: any) => {
  if (command === "notification_history_append") return [args.record];
  if (command.startsWith("notification_history_")) return [];
  if (command === "zcode_snapshot" || command === "start_zcode_session") return zcode();
  if (command === "zcode_side_send") return zcode();
  if (command === "zcode_queue_action") { queued = []; return zcode(); }
  if (command === "zcode_composer_catalog") return { skills: { items: [] }, commands: { items: [] }, plugins: { items: [] } };
  if (command.includes("snapshot") || command.startsWith("start_")) return snapshot(args.sessionId);
  if (command === "plugin:dialog|open") return [`/attachment-${++file}.txt`];
  if (command === "test_provider_endpoints") return [{ url: "https://fast.example/anthropic", latencyMs: 5, status: 200, reachable: true, error: null }];
  if (command === "fetch_provider_models") return [{ id: "old-endpoint-model", ownedBy: "mock" }];
  return null;
};
(window as any).__TAURI_INTERNALS__ = {
  transformCallback: (fn: (event: unknown) => void) => { callbacks.set(++callbackId, fn); return callbackId; },
  invoke: (command: string, args: any) => {
    calls.push({ command, args });
    if (["notification_history_clear", "notification_history_mark_all_read"].includes(command) && (window as any).asyncChatMock.historyFail) return Promise.reject(new Error("simulated history write failure"));
    if (command === "show_notification_overlay") return Promise.reject(new Error("simulated notification denied"));
    if (command === "plugin:opener|open_url" && (window as any).asyncChatMock.openerFail) return Promise.reject(new Error("simulated opener failure"));
    if (command === "plugin:event|listen") return Promise.resolve(++callbackId);
    if (command === "plugin:event|unlisten") return Promise.resolve(null);
    if (command.startsWith("send_") || ["zcode_side_send", "zcode_queue_action", "zcode_slash_command", "test_provider_endpoints", "fetch_provider_models", "ma_confirm_run_stopped", "fixture_save_provider"].includes(command)) return new Promise((resolve, reject) => pending.push({ command, args, resolve, reject }));
    return Promise.resolve(result(command, args));
  },
};
(window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
(window as any).asyncChatMock = { calls, pending, release(command: string, failure?: string) {
  const index = pending.findIndex(item => item.command === command);
  if (index < 0) throw new Error("No pending request for " + command);
  const [item] = pending.splice(index, 1);
  if (failure) item.reject(failure); else item.resolve(result(item.command, item.args));
} };
const provider: ProviderConfig = { id: "fixture-provider", name: "Fixture", baseUrl: "https://old.example/anthropic", authTokenRef: "fixture-key", env: {}, timeoutMs: null, disableNonessentialTraffic: false, builtin: false, meta: { endpointAutoSelect: true } as ProviderConfig["meta"] };
function Fixture() {
  const [kind, setKind] = useState(new URLSearchParams(location.search).get("kind") || "claude");
  const creatingProvider = new URLSearchParams(location.search).get("new") === "1";
  const editorProvider = creatingProvider ? { ...provider, id: "", name: "", authTokenRef: "" } : provider;
  const [confirmed, setConfirmed] = useState(false);
  const frozenRun = { id: "frozen-run", taskId: "task", sessionLocator: { stopUnverified: true }, error: "process 123: stop could not be verified" } as unknown as AgentRun;
  const Chat = components[kind as keyof typeof components];
  return <><nav>{[...Object.keys(components), "provider", "frozen", "notification"].map(name => <button key={name} onClick={() => setKind(name)}>{name}</button>)}</nav>{kind === "notification" ? <NotificationSettingsPanel settings={DEFAULT_NOTIFICATION_SETTINGS} onChange={() => {}} /> : kind === "frozen" ? confirmed ? <p>任务可重试</p> : <UnverifiedRunNotice run={frozenRun} taskTitle="冻结任务" onConfirmed={() => setConfirmed(true)} /> : kind === "provider" ? <ProviderEditor provider={editorProvider} providers={[provider]} hasKey={!creatingProvider} onSave={(provider, key) => (window as any).__TAURI_INTERNALS__.invoke("fixture_save_provider", { provider, key })} onClose={() => setKind("claude")} /> : <Chat key={kind} session={session(kind)} managedToolbar onSessionRecord={() => {}} />}</>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
