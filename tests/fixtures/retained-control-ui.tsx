import { useState } from "react";
import { createRoot } from "react-dom/client";
import { MultiAgentControl } from "../../src/components/MultiAgentControl";
import { ControlSidebar } from "../../src/components/ControlSidebar";
import { ControlWindow } from "../../src/components/ControlWindow";
import { CollaborationSettingsPanel } from "../../src/components/CollaborationSettingsPanel";
import { CollaborationSupportPanel } from "../../src/components/CollaborationSupportPanel";
import "../../src/styles.css";

const params = new URLSearchParams(location.search);
const mode = params.get("mode") || "multi";
const projects = ["A", "B", "C"].map(id => ({ id, name: `Project-${id}`, goal: `Goal-${id}`, rootPath: "C:/fixture", defaultBranch: "main", status: "active", createdAt: 1, updatedAt: 1 }));
const baseSnapshot = (id: string) => ({ project: projects.find(item => item.id === id)!, agents: [{ id: `${id}-agent`, projectId: id, name: `${id}-agent`, runtimeKind: "claude_code", role: "worker", enabled: true, description: "", capabilities: [], persona: "efficient", personaCustom: "", model: null, createdAt: 1, updatedAt: 1 }], tasks: [{ id: `${id}-task`, projectId: id, title: `${id}-task`, status: mode === "support" ? "running" : "in_review", stage: 0, assigneeAgentId: `${id}-agent`, acceptanceCriteria: "", description: "", updatedAt: 1 }], messages: [], runs: [], events: [], agentSessions: [], runtimeEvents: [], memories: [], runLogs: [], artifacts: [], dependencies: [], approvals: [], instructions: [], reviewDecisions: [] });
const snapshot = (id: string) => {
  const value = baseSnapshot(id);
  if (mode !== "member-selection") return value;
  return { ...value, tasks: [], messages: calls.filter(item => item.command === "ma_send_direct_message" && item.args.input.projectId === id).flatMap((item, index) => [
    { id: `user-${index}`, createdAt: index + 1, senderType: "user", body: item.args.input.message },
    { id: `reply-${index}`, createdAt: index + 1, senderType: "coordinator", body: "fixture direct queued" },
  ]), agents: [
    { ...value.agents[0], id: `${id}-claude`, name: "架构师", runtimeKind: "claude_code" },
    { ...value.agents[0], id: `${id}-codex`, name: "开发员", runtimeKind: "codex" },
    { ...value.agents[0], id: `${id}-reviewer`, name: "审查员", runtimeKind: "codex", role: "reviewer" },
    { ...value.agents[0], id: `${id}-disabled`, name: "禁用成员", enabled: false },
    { ...value.agents[0], id: `${id}-pi`, name: "禁用运行时", runtimeKind: "pi" },
  ] };
};
const health = { cliPath: "fixture", cliPathExists: true, replyEvidence: "fixture", runtimes: [], messages: [], handoffs: [], sessions: ["source", "target"].map(id => ({ id, title: id, runtime: "claude_code", cwd: "C:/fixture", ptyId: 1, status: "running", processRunning: true, ready: true, reason: "fixture", error: null, waitingReason: null, interactiveHandoff: true, identityEnvironment: "fixture", bridge: { configured: false, error: null } })) };
const plan = { intent: "plan_project", reply: "fixture plan", projectName: "New project", goal: "Fixture goal", rootPath: "C:/fixture", targetTaskId: null, targetAgentId: null, agents: [], tasks: [], requestedRuntimes: [], requiresConfirmation: true };
if (!params.has("preserve-storage")) localStorage.clear();
localStorage.setItem("crewvela-multi-agent-project", "A");
if (mode.startsWith("plan")) localStorage.setItem("crewvela-multi-agent-draft-v1", JSON.stringify({ coordinatorProjectId: "A", projectPath: "C:/fixture", coordinatorDraft: "", coordinatorMessages: [], mentionBindings: [], pendingPlan: plan, pendingPlanContext: { projectId: null, projectPath: "C:/fixture", autoStart: true, requestedRuntimes: [] } }));
const pending: Array<{ command: string; args: any; resolve: (value: any) => void; reject: (reason: Error) => void }> = [];
const calls: Array<{ command: string; args: any }> = [];
const deferred = new Set(["fixture_quick", "fixture_broadcast", "optimize_prompt", "ma_apply_plan", "ma_start_project", "ma_pause_project", "ma_approve_task", "ma_retry_task", "collaboration_handoff_preview", "collaboration_handoff_autofill", "collaboration_handoff_confirm", "collaboration_handoff_cancel", "interpret_control_message", "close_control_window", "dock_control_window"]);
const mock = {
  calls, pending, holdB: false, snapshot,
  invoke(command: string, args: any) {
    calls.push({ command, args });
    if (deferred.has(command) || (command === "ma_snapshot" && args.projectId === "B" && mock.holdB)) return new Promise((resolve, reject) => pending.push({ command, args, resolve, reject }));
    if (command === "ma_send_direct_message") return Promise.resolve({ reply: "fixture direct queued", deliveryTaskIds: [] });
    if (command === "ma_list_projects") return Promise.resolve(projects);
    if (command === "ma_snapshot") return Promise.resolve(snapshot(args.projectId));
    if (command === "ma_conversation_snapshot") return Promise.resolve({conversation:{id:`conversation-${args.projectId}`,projectId:args.projectId,createdAt:1},messages:[],rounds:[],executionPlans:[],supportedDiscussionRuntimes:["claude_code","codex","pi"]});
    if (command === "get_runtime_preferences") return Promise.resolve({ disabledRuntimes: mode === "member-selection" ? ["pi"] : [] });
    if (["ma_probe_runtimes", "ma_check_runtime_updates"].includes(command)) return Promise.resolve(["claude_code", "zcode_cli"].map(kind => ({ kind, installed: true, error: null, version: "1.0", capabilities: [] })));
    if (command === "skill_center_snapshot") return Promise.resolve({ skillSets: [] });
    if (command === "collaboration_health") return Promise.resolve(health);
    if (command === "control_workspace_trust_status") return Promise.resolve({ batchCount: 0, awaitingCount: 0, startingCount: 0, missingCount: 0 });
    if (command === "plugin:event|listen") return Promise.resolve(1);
    return Promise.resolve([]);
  },
  release(command: string, value: any = null, error?: string) {
    const index = pending.findIndex(item => item.command === command);
    if (index < 0) throw new Error(`No pending ${command}`);
    const item = pending.splice(index, 1)[0];
    if (error) item.reject(new Error(error)); else item.resolve(value);
  },
  selectProject(id: string) { window.dispatchEvent(new CustomEvent("crewvela-multi-agent-project-selected", { detail: { projectId: id } })); },
};
(window as any).retainedControlMock = mock;
function Fixture() {
  const [projectId, setProject] = useState("A");
  const [request, setRequest] = useState<{ nonce: number; taskId?: string } | null>(null);
  (mock as any).switchSupport = (id: string) => setProject(id);
  if (mode === "window") return <ControlWindow />;
  if (mode === "sidebar") return <ControlSidebar open providers={[]} sessions={[]} defaultCwd="C:/fixture" onClose={() => {}} onSessionsChange={() => {}} />;
  if (mode === "settings") return <CollaborationSettingsPanel onClose={() => {}} />;
  if (mode === "support") return <><button onClick={() => setRequest({ nonce: Date.now(), taskId: `${projectId}-task` })}>打开交接</button><CollaborationSupportPanel snapshot={snapshot(projectId) as any} enabledRuntimes={["claude_code"]} onChanged={() => {}} onHealthChange={() => {}} onHandoffClose={() => setRequest(null)} request={request} view="diagnostics" /></>;
  return <MultiAgentControl defaultCwd="C:/fixture" />;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
