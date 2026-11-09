import { useState } from "react";
import { createRoot } from "react-dom/client";
import { GroupChatPanel } from "../../src/components/GroupChatPanel";
import { InterventionCenter } from "../../src/components/InterventionCenter";
import { AutomationPanel } from "../../src/components/AutomationPanel";
import { CollaborationAgentEditor } from "../../src/components/CollaborationAgentEditor";
import { MultiAgentWorkspace } from "../../src/components/MultiAgentWorkspace";
import { GeneralSettingsPanel } from "../../src/components/GeneralSettingsPanel";
import { DEFAULT_GENERAL_SETTINGS } from "../../src/generalSettings";
import { BrowserControlSettingsPanel } from "../../src/components/BrowserControlSettingsPanel";
import { ComputerControlSettingsPanel } from "../../src/components/ComputerControlSettingsPanel";
import { DEFAULT_BROWSER_CONTROL_SETTINGS } from "../../src/browserControlSettings";
import type { SessionRecord } from "../../src/types";
import "../../src/styles.css";

const cards = [1, 2].map(index => ({ sessionId: `session-${index}`, ptyId: index, title: `Fixture approval ${index}`, waitingSince: 1000 + index, waitingReason: "Mock only", providerName: "Fixture", cwd: "fixture", approvalPolicy: "ask" }));
const pending: Array<{ command: string; args: any; resolve: (value: any) => void; reject: (error: Error) => void }> = [];
const calls: Array<{ command: string; args: any }> = [];
const mode = new URLSearchParams(location.search).get("mode");
const members = [
  { id: "acp", runtimeKind: "deepseek_harness", ptyId: null, runtimeCommandHost: true },
  { id: "zcode", runtimeKind: "zcode", ptyId: null, runtimeCommandHost: true },
  { id: "pi", runtimeKind: "pi", ptyId: "fixture-pty", runtimeCommandHost: false },
  { id: "stopped", runtimeKind: "codex", ptyId: null, runtimeCommandHost: false },
  { id: "archived", runtimeKind: "codex", ptyId: null, runtimeCommandHost: true, archivedAt: 1 },
  { id: "retired", runtimeKind: "codex", ptyId: null, runtimeCommandHost: true, retiredAgentRuntime: true },
];
const fixture = { calls, pending, holdReads: false,
  invoke(command: string, args: any) {
    calls.push({ command, args });
    if (command === "get_runtime_preferences") return Promise.resolve({ disabledRuntimes: [] });
    if (command === "plugin:event|listen") return Promise.resolve(1);
    if (command === "ma_list_projects") return Promise.resolve([{ id: "project-A", name: "project-A" }, { id: "project-B", name: "project-B" }]);
    if (["browser_control_get", "browser_control_save", "computer_control_get", "computer_control_save", "kimi_computer_use_get", "kimi_computer_use_save", "open_computer_use_get", "open_computer_use_save", "general_settings_get", "general_settings_save", "fixture_save_agent", "ma_takeover_agent", "ma_return_agent_control", "send_session_message", "respond_session_approval", "create_automation"].includes(command) || (mode === "automation" && command === "ma_snapshot") || (fixture.holdReads && ["list_approval_cards", "ma_list_interventions"].includes(command))) {
      return new Promise((resolve, reject) => pending.push({ command, args, resolve, reject }));
    }
    if (command === "ma_snapshot") return Promise.resolve({ project: { id: args.projectId, name: args.projectId }, agents: members.map(member => ({ id: member.id, name: member.id, runtimeKind: member.runtimeKind, enabled: true, capabilities: [] })), agentSessions: members.map(member => ({ agentId: member.id, appSessionId: member.id })), messages: [], runs: [], tasks: mode === "workspace" ? [{ id: "task", assigneeAgentId: "acp", status: "running" }] : [], events: [], runtimeEvents: [], artifacts: [], memories: [] });
    if (command === "list_approval_cards") return Promise.resolve(cards);
    return Promise.resolve([]);
  },
  releaseProject(projectId: string) {
    const index = pending.findIndex(item => item.command === "ma_snapshot" && item.args.projectId === projectId);
    if (index < 0) throw Error("No snapshot for " + projectId);
    pending.splice(index, 1)[0].resolve({ agents: [{ id: projectId + "-agent", name: projectId + "-agent", enabled: true, runtimeKind: "codex" }] });
  },
  release(command: string, error?: string) {
    const index = pending.findIndex(item => item.command === command);
    if (index < 0) throw Error(`No pending ${command}`);
    const [item] = pending.splice(index, 1);
    if (error) item.reject(new Error(error));
    else if (command.startsWith("general_settings_")) item.resolve({ settings: item.args?.settings ?? { ...DEFAULT_GENERAL_SETTINGS, terminalFontSize: 28 }, proxyEnvActive: false, keepAwakeActive: false, warning: null });
    else if (command.startsWith("browser_control_")) item.resolve({ settings: item.args?.settings ?? DEFAULT_BROWSER_CONTROL_SETTINGS, engine: "fixture", resolvedBrowser: "fixture", warning: null });
    else if (/^(computer_control|kimi_computer_use|open_computer_use)_/.test(command)) item.resolve({ settings: item.args?.settings ?? { enabled: false }, runtime: "fixture", warning: null });
    else item.resolve(null);
  },
  finishReadsNewestFirst() {
    const reads = pending.splice(0).filter(item => ["list_approval_cards", "ma_list_interventions"].includes(item.command));
    for (const command of ["list_approval_cards", "ma_list_interventions"]) {
      const matching = reads.filter(item => item.command === command);
      matching.pop()?.resolve([]);
      for (const item of matching) item.resolve(command === "list_approval_cards" ? cards : []);
    }
  },
};
(window as any).collabMock = fixture;
function Fixture() {
  const [project, setProject] = useState("project-A");
  const [, setCount] = useState(0);
  const [closed, setClosed] = useState(false);
  if (mode === "general") return <GeneralSettingsPanel />;
  if (mode === "browser") return <BrowserControlSettingsPanel />;
  if (mode === "computer") return <ComputerControlSettingsPanel />;
  if (mode === "agent") return closed ? <p>成员已保存</p> : <CollaborationAgentEditor agent={null} runtimeLocked={false} onClose={() => setClosed(true)} onSave={async draft => { await fixture.invoke("fixture_save_agent", { draft }); }} />;
  if (mode === "workspace") return <><button onClick={() => setProject(value => value === "project-A" ? "project-B" : "project-A")}>切换项目</button><p>{project}</p><MultiAgentWorkspace projectId={project} sessions={[]} theme="dark" onSessionsChange={() => {}} onCloseSession={() => {}} onClose={() => {}} /></>;
  if (mode === "automation") return <AutomationPanel sessions={[]} providers={[]} embedded />;
  if (new URLSearchParams(location.search).get("mode") === "approvals") return <InterventionCenter open onClose={() => {}} onCountChange={setCount} onOpenProject={() => {}} />;
  return <><button onClick={() => setProject(value => value === "project-A" ? "project-B" : "project-A")}>切换项目</button><GroupChatPanel projectId={project} active sessions={members as unknown as SessionRecord[]} /></>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
