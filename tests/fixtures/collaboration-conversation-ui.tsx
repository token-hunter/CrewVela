// Isolated IPC fixture. Never starts a CLI, edits a user project, or calls a model.
import { useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Robot, CornersOut, X } from "@phosphor-icons/react";
import { MultiAgentControl } from "../../src/components/MultiAgentControl";
import "../../src/styles.css";

const params = new URLSearchParams(location.search);
document.documentElement.dataset.theme = params.get("theme") === "light" ? "light" : "dark";
document.documentElement.dataset.appearanceAccent = params.get("accent") ?? "clay";
const clock = new Date("2026-11-16T10:20:00+08:00").getTime();
const agent = (id: string, name: string, runtimeKind: string, role: string, projectId = "A") => ({ id, projectId, name, runtimeKind, role, avatarId: null, enabled: true, description: "", persona: "efficient", personaCustom: "", model: null, permissionProfile: "default", maxConcurrency: 1, capabilities: [], createdAt: clock, updatedAt: clock });
const agents = [agent("claude", "Claude Code", "claude_code", "orchestrator"), agent("codex", "Codex", "codex", "worker"), agent("kimi", "Kimi Code", "kimi_code", "reviewer")];
let projects = [
  { id: "A", name: "团队待办应用", rootPath: "/fixture/todo-app", goal: "实现团队待办应用", defaultBranch: "main", status: "active", createdAt: clock, updatedAt: clock },
  { id: "B", name: "文档项目", rootPath: "/fixture/docs", goal: "整理文档", defaultBranch: "main", status: "active", createdAt: clock, updatedAt: clock },
];
const planned = [
  { key: "design", title: "方案与验收", assigneeAgentId: "claude", reviewerAgentId: null, workspaceMode: "read_only", writePaths: [], dependsOn: [], description: "梳理方案及验收项", acceptanceCriteria: "接口与交互要求完整", verificationCommand: null },
  { key: "build", title: "列表与筛选", assigneeAgentId: "codex", reviewerAgentId: "kimi", workspaceMode: "worktree", writePaths: ["src/"], dependsOn: ["design"], description: "实现列表与筛选交互", acceptanceCriteria: "通过组件测试及构建", verificationCommand: "npm test" },
  { key: "review", title: "审查与测试", assigneeAgentId: "kimi", reviewerAgentId: null, workspaceMode: "read_only", writePaths: [], dependsOn: ["build"], description: "核对交互边界与回归", acceptanceCriteria: "记录检查结果和未验证项", verificationCommand: null },
];
const emptyChat = (id: string): any => ({ conversation: { id: `conversation-${id}`, projectId: id, createdAt: clock }, messages: [], rounds: [], executionPlans: [], supportedDiscussionRuntimes: params.get("unsupported") === "1" ? ["claude_code", "codex"] : ["claude_code", "codex", "kimi_code"] });
const message = (id: string, senderType: string, body: string, agentId: string | null = null, status = "completed", kind = "reply"): any => ({ id, projectId: "A", conversationId: "conversation-A", roundId: "round-1", replyToMessageId: "m1", taskId: null, runId: agentId ? `run-${agentId}` : null, agentId, senderType, kind, phase: "opinions", body, status, createdAt: clock + Number(id.slice(1) || 0) * 1000, updatedAt: clock });
const chats: Record<string, any> = { A: emptyChat("A"), B: emptyChat("B") };
if (params.get("empty") !== "1") {
  chats.A.rounds = [{ id: "round-1", projectId: "A", conversationId: "conversation-A", userMessageId: "m1", participantAgentIds: ["claude", "codex", "kimi"], mode: "execution_plan", status: params.get("running") === "1" ? "running" : "completed", phase: "summary", maxRounds: 1, currentRound: 1, deadlineAt: clock + 600000, stopReason: null, createdAt: clock, updatedAt: clock }];
  chats.A.messages = [
    message("m1", "user", "@Claude Code 拆解方案，@Codex 开发，@Kimi Code 审查。", null, "completed", "message"),
    message("m2", "agent", "@Codex 先实现任务列表与状态切换，我来整理验收项。", "claude"),
    message("m3", "agent", "", "claude", "completed", "plan"),
    message("m4", "user", "按这个分工执行。", null, "completed", "message"),
    message("m5", "agent", "@Kimi Code 列表交互已实现，筛选仍在进行；稍后提交审查。", "codex"),
    message("m6", "agent", "@Codex 我先整理边界用例，等你交接后开始检查。", "kimi"),
    message("m7", "system", "Codex 正在执行 · 本轮尚未验收", "codex", "running", "status"),
  ];
  chats.A.executionPlans = [{ id: "plan-1", projectId: "A", roundId: "round-1", messageId: "m3", summary: "按成员分工与依赖推进", tasks: planned, status: params.get("plan") === "pending" ? "pending" : "applied", taskIds: params.get("plan") === "pending" ? [] : ["t-design", "t-build", "t-review"], createdAt: clock }];
}
const tasks = planned.map((task, index) => ({ ...task, id: `t-${task.key}`, projectId: "A", parentId: null, stage: index, status: ["done", "running", "blocked"][index], priority: 0, requiredCapabilities: [], maxAttempts: 2, retryBackoffSeconds: 1, nextRetryAt: null, createdAt: clock, updatedAt: clock }));
const snapshot = (id: string): any => ({ project: projects.find(project => project.id === id), agents: id === "A" ? agents : [agent("b-codex", "文档 Codex", "codex", "worker", "B")], tasks: id === "A" && params.get("plan") !== "pending" ? tasks : [], runs: [], messages: [], events: [], dependencies: [{ taskId: "t-review", dependsOnTaskId: "t-build" }], agentSessions: (id === "A" ? agents : []).map(agent => ({ agentId: agent.id, appSessionId: `session-${agent.id}`, projectId: id, id: `agent-session-${agent.id}`, status: agent.id === "codex" ? "running" : "idle", controlMode: "auto", currentTaskId: null, currentRunId: null, lastOutputAt: null, createdAt: clock, updatedAt: clock })), runLogs: [], runtimeEvents: [], memories: [], instructions: [], artifacts: [], approvals: [], reviewDecisions: [], wakeups: [] });
const calls: Array<{ command: string; args: any }> = [];
const pending: Array<{ command: string; args: any; resolve(value: any): void; reject(reason: Error): void }> = [];
const receipts = new Map<string, string>();
let failSend = params.get("fail") === "send";
let failBeforeSave = params.get("fail") === "send-before-save";
const mock = {
  calls, pending, chats, projects, snapshot, holdProject: "", holdSend: false,
  release(command: string, value?: any, error?: string) { const index = pending.findIndex(item => item.command === command); if (index < 0) throw Error(`No pending ${command}`); const item = pending.splice(index, 1)[0]; if (error) item.reject(Error(error)); else item.resolve(value ?? (command === "ma_snapshot" ? snapshot(item.args.projectId) : chats[item.args.projectId ?? item.args.input.projectId])); },
  async invoke(command: string, args: any = {}) {
    calls.push({ command, args: structuredClone(args) });
    if ((["ma_snapshot", "ma_conversation_snapshot"].includes(command) && mock.holdProject === args.projectId) || (command === "ma_conversation_send" && mock.holdSend)) return new Promise((resolve, reject) => pending.push({ command, args, resolve, reject }));
    if (command === "ma_list_projects") return structuredClone(projects);
    if (command === "ma_snapshot") return structuredClone(snapshot(args.projectId));
    if (command === "ma_conversation_snapshot") return structuredClone(chats[args.projectId]);
    if (["ma_conversation_send", "ma_conversation_interrupt"].includes(command)) {
      const input = args.input, chat = chats[input.projectId];
      if (failBeforeSave) { failBeforeSave = false; throw Error("隔离测试：响应丢失，发送结果未确认"); }
      if (!receipts.has(input.clientMessageId)) {
        receipts.set(input.clientMessageId, input.projectId);
        chat.messages.push({ ...message(input.clientMessageId, "user", input.body, null, "completed", "message"), clientMessageId: input.clientMessageId, projectId: input.projectId, conversationId: chat.conversation.id, createdAt: clock + 50000, updatedAt: clock + 50000 });
        chat.rounds.push({ id: `round-${input.clientMessageId}`, projectId: input.projectId, conversationId: chat.conversation.id, userMessageId: input.clientMessageId, participantAgentIds: input.participantAgentIds ?? ["claude", "codex"], mode: input.mode ?? "discussion", status: command === "ma_conversation_interrupt" ? "paused" : "running", phase: "opinions", maxRounds: 1, currentRound: 0, deadlineAt: clock + 600000, stopReason: command === "ma_conversation_interrupt" ? "人工插话已保存，后续发言暂停" : null, createdAt: clock + 50000, updatedAt: clock + 50000 });
      }
      if (failSend) { failSend = false; throw Error("隔离测试：响应丢失，发送结果未确认"); }
      return structuredClone(chat);
    }
    if (command === "ma_conversation_control") { const chat = chats[args.projectId]; if (chat.rounds.length) chat.rounds[chat.rounds.length - 1].status = params.get("stopping") === "1" ? "stopping" : args.action === "pause" ? "paused" : "cancelled"; return structuredClone(chat); }
    if (command === "ma_conversation_execute_plan") { const chat = chats[args.input.projectId]; const plan = chat.executionPlans.find((item: any) => item.id === args.input.planId); plan.status = "applied"; plan.taskIds = ["t-design", "t-build", "t-review"]; return structuredClone(chat); }
    if (command === "ma_pause_project") { projects.find(project => project.id === args.projectId)!.status = "paused"; return { accepted: true, projectId: args.projectId }; }
    if (command === "ma_create_project") { const project = { ...args.input, id: crypto.randomUUID(), status: "active", createdAt: clock, updatedAt: clock }; projects = [...projects, project]; chats[project.id] = emptyChat(project.id); return project; }
    if (command === "plugin:dialog|open") return "/fixture/new-project";
    if (command === "get_runtime_preferences") return { disabledRuntimes: params.get("disabled") ? [params.get("disabled")] : [] };
    if (command === "plugin:event|listen") return 1;
    if (command === "plugin:event|unlisten") return null;
    throw Error(`隔离页面未模拟 ${command}`);
  },
};
(window as any).conversationMock = mock;
(window as any).__TAURI_INTERNALS__ = { invoke: mock.invoke, transformCallback: () => 1 };
function Fixture() {
  const [active, setActive] = useState(true);
  const [instance, setInstance] = useState(0);
  (mock as any).setActive = setActive;
  (mock as any).selectInstance = setInstance;
  const panel = (index: number) => <MultiAgentControl active={active && index === instance} defaultCwd="/fixture/todo-app" onOpenSession={id => { calls.push({ command: "fixture_open_session", args: { sessionId: id } }); }} onOpenProjectSettings={id => { calls.push({ command: "fixture_open_settings", args: { projectId: id } }); }} onOpenWorkspace={id => { calls.push({ command: "fixture_open_workspace", args: { projectId: id } }); }} />;
  return <main className="conversation-fixture-frame"><header className="conversation-fixture-resource"><span><Robot size={21} />多智能体<XMark /></span><small>隔离示例 · 无真实 Agent 调用</small><CornersOut size={19} /></header>{params.get("instances") === "2" ? [0,1].map(index => <div style={{ display: index === instance ? "flex" : "none", flex: 1, minHeight: 0 }} key={index} data-instance={index}>{panel(index)}</div>) : panel(0)}</main>;
}
function XMark() { return <X size={17} aria-hidden="true" style={{ marginLeft: 18 }} />; }
const style = document.createElement("style"); style.textContent = 'html,body,#root{height:100%;margin:0;overflow:hidden}.conversation-fixture-frame{height:100%;display:flex;flex-direction:column;background:var(--bg-elev);color:var(--text)}.conversation-fixture-resource{height:64px;flex:none;display:flex;align-items:center;gap:18px;padding:0 30px;border-bottom:1px solid var(--border);background:var(--bg)}.conversation-fixture-resource>span{white-space:nowrap;flex:none;display:flex;align-items:center;gap:9px;padding:8px 13px;border-radius:8px;background:var(--bg-hover);font:600 17px var(--display-font)}.conversation-fixture-resource>small{margin-left:auto;color:var(--text-dim);font:11px var(--display-font)}@media(max-width:480px){.conversation-fixture-resource{padding:0 14px;gap:8px}.conversation-fixture-resource>small{font-size:9px}}'; document.head.append(style);
const hot = import.meta.hot as { data: { root?: Root } } | undefined;
const root = hot?.data.root ?? createRoot(document.getElementById("root")!); if (hot) hot.data.root = root; root.render(<Fixture />);
