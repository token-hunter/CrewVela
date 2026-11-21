// Isolated IPC fixture. Never starts a CLI, edits a user project, or calls a model.
import { useRef, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Robot, CornersOut, X } from "@phosphor-icons/react";
import { MultiAgentControl } from "../../src/components/MultiAgentControl";
import { SettingsDialog } from "../../src/components/SettingsDialog";
import { DEFAULT_APPEARANCE } from "../../src/appearance";
import { DEFAULT_PET_SETTINGS } from "../../src/petSettings";
import { DEFAULT_NOTIFICATION_SETTINGS } from "../../src/notificationSettings";
import { GENERAL_SETTINGS_PREVIEW } from "../../src/generalSettings";
import type { AppAppearance } from "../../src/types";
import type { CollaborationTasksRequest } from "../../src/collaborationTasks";
import "../../src/styles.css";

const params = new URLSearchParams(location.search);
document.documentElement.dataset.theme = params.get("theme") === "light" ? "light" : "dark";
document.documentElement.dataset.appearanceAccent = params.get("accent") ?? "clay";
const dualViews = params.get("dual-views") === "1";
const clock = new Date("2026-11-16T10:20:00+08:00").getTime();
const agent = (id: string, name: string, runtimeKind: string, role: string, projectId = "A") => ({ id, projectId, name, runtimeKind, role, avatarId: null, enabled: true, description: "", persona: "efficient", personaCustom: "", model: null, permissionProfile: "default", maxConcurrency: 1, capabilities: [], createdAt: clock, updatedAt: clock });
const agents = params.get("header") === "1"
  ? [{ ...agent("claude", "模型验收-Claude", "claude_code", "worker"), avatarId: "orbit" }, { ...agent("codex", "模型验收-Codex", "codex", "worker"), avatarId: "block" }, { ...agent("kimi", "聊天-ZCodeCLI", "zcode_cli", "orchestrator"), avatarId: "sprout" }]
  : [agent("claude", "Claude Code", "claude_code", "orchestrator"), agent("codex", "Codex", "codex", "worker"), agent("kimi", "Kimi Code", "kimi_code", "reviewer")];
if (params.get("native") === "1") agents.push({ ...agent("zcode-review", "语义复核员", "zcode_cli", "reviewer"), avatarId: "ribbon" });
if (params.get("reply-routing") === "1") {
  agents[0].name = "后端审查";
  agents[0].role = "reviewer";
  agents[1].name = "后端开发";
  agents[2].name = "联调集成";
  agents[2].role = "integrator";
}
if (params.get("members") === "none") agents.length = 0;
if (params.get("members") === "eight") for (let i=0;i<5;i++) agents.push(agent(`extra-${i}`, `开发成员-${i}`, "codex", "worker"));
if (params.get("members") === "many") for (let i=0;i<24;i++) agents.push(agent(`extra-${i}`, `开发成员-${i}：很长的成员名称用于检查省略与布局`, "codex", "worker"));
let projects = [
  { id: "A", name: "团队待办应用", rootPath: "/fixture/todo-app", goal: "实现团队待办应用", defaultBranch: "main", status: "active", createdAt: clock, updatedAt: clock },
  { id: "B", name: "文档项目", rootPath: "/fixture/docs", goal: "整理文档", defaultBranch: "main", status: "active", createdAt: clock, updatedAt: clock },
];
if (params.get("header") === "1") { projects[0].name = "模型设置实测-20261115"; projects[0].rootPath = "/fixture/crewvela-model-live-e79907a4ca"; }
if (params.get("long") === "1") { projects[0].name = "很长的协作项目名称".repeat(12); projects[0].rootPath = "/fixture/" + "很长的目录名称".repeat(12); }
const planned = [
  { key: "design", title: "方案与验收", assigneeAgentId: "claude", reviewerAgentId: null, workspaceMode: "read_only", writePaths: [], dependsOn: [], description: "梳理方案及验收项", acceptanceCriteria: "接口与交互要求完整", verificationCommand: null },
  { key: "build", title: "列表与筛选", assigneeAgentId: "codex", reviewerAgentId: "kimi", workspaceMode: "worktree", writePaths: ["src/"], dependsOn: ["design"], description: "实现列表与筛选交互", acceptanceCriteria: "通过组件测试及构建", verificationCommand: "npm test" },
  { key: "review", title: "审查与测试", assigneeAgentId: "kimi", reviewerAgentId: null, workspaceMode: "read_only", writePaths: [], dependsOn: ["build"], description: "核对交互边界与回归", acceptanceCriteria: "记录检查结果和未验证项", verificationCommand: null },
];
if (dualViews) {
  projects[0].name = "登录功能改版";
  ["Claude", "Codex", "Kimi"].forEach((name,index) => { agents[index].name=name; agents[index].avatarId=["orbit","block","sprout"][index] as any; });
  ["登录页面","接口对接","测试与审查"].forEach((title,index) => { planned[index].title=title; planned[index].description=["登录表单与状态反馈","对接登录相关接口","功能验证与体验检查"][index]; planned[index].acceptanceCriteria="覆盖成功、失败和加载状态"; });
}
const emptyChat = (id: string): any => ({ conversation: { id: `conversation-${id}`, projectId: id, createdAt: clock }, messages: [], rounds: [], executionPlans: [], supportedDiscussionRuntimes: params.get("native") === "1" ? ["claude_code","codex","kimi_code","pi","opencode","deepseek_harness","zcode","zcode_cli"] : params.get("unsupported") === "1" ? ["claude_code", "codex"] : ["claude_code", "codex", "kimi_code", ...(params.get("header") === "1" ? ["zcode_cli"] : [])] });
const message = (id: string, senderType: string, body: string, agentId: string | null = null, status = "completed", kind = "reply"): any => ({ id, projectId: "A", conversationId: "conversation-A", roundId: "round-1", replyToMessageId: "m1", taskId: null, runId: agentId ? `run-${agentId}` : null, agentId, senderType, kind, phase: "opinions", body, status, createdAt: clock + Number(id.slice(1) || 0) * 1000, updatedAt: clock });
const chats: Record<string, any> = { A: emptyChat("A"), B: emptyChat("B") };
const memberModelSettings = new Map<string, {model: string | null; reasoning: string | null}>();
if (params.get("rules") === "1") chats.A.archivedTaskRules = [{ id: "acceptance-rule", agentId: "claude", label: "Claude 原验收规则", instructions: "只回复 ABC，不调用工具", tools: "none", createdAt: clock }];
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
if (dualViews && params.get("empty") !== "1") {
  chats.A.messages = [message("m1","user","@Claude @Codex 帮我做登录页，先给分工。",null,"completed","message"),message("m2","agent","我负责页面交互，Codex 对接接口，Kimi 做测试。","claude"),message("m3","agent","","claude","completed","plan")];
  chats.A.executionPlans[0].summary="基于登录功能改版的需求，建议如下分工：";
}
const tasks = planned.map((task, index) => ({ ...task, id: `t-${task.key}`, projectId: "A", parentId: null, stage: index, status: ["done", "running", "blocked"][index], priority: 0, requiredCapabilities: [], maxAttempts: 2, retryBackoffSeconds: 1, nextRetryAt: null, createdAt: clock, updatedAt: clock }));
const resumedDelivery = params.has("resumed-delivery") ? {
  id: "delivery-approval", projectId: "A", taskId: "conversation-task-resumed",
  runId: "development-run", approvalType: "task_review", status: "approved",
} : null;
if (resumedDelivery) {
  tasks[1].id = resumedDelivery.taskId;
  tasks[1].status = "done";
  tasks[2].status = "ready";
}
if (dualViews) tasks.forEach((task,index)=>{task.status=["running","ready","blocked"][index];});
if (params.get("internal") === "1") tasks.push({ ...tasks[0], id: "cli-message-conversation-internal", title: "讨论内部执行", status: "running", stage: 9 });
if (params.get("header") === "1") { tasks.forEach(task=>{task.status="done";}); for(let i=tasks.length;i<28;i++) tasks.push({...tasks[0],id:`history-${i}`,title:`历史任务 ${i+1}`}); }
const runHistory = params.get("evidence") === "1" ? [1,2].map(attempt=>({ id:`evidence-${attempt}`,projectId:"A",taskId:"t-build",agentId:"codex",attempt,status:"failed",sessionLocator:attempt===2?{stopUnverified:true}:null,leaseExpiresAt:0,lastHeartbeatAt:null,outputSummary:`尝试${attempt}输出`,error:attempt===2?"fixture 原进程停止状态待确认":"fixture 旧尝试失败",createdAt:clock+attempt,startedAt:clock,finishedAt:clock+attempt })) : [];
if (dualViews) runHistory.push({id:"login-run",projectId:"A",taskId:"t-design",agentId:"claude",attempt:1,status:"running",sessionLocator:null,leaseExpiresAt:clock+60000,lastHeartbeatAt:clock,outputSummary:"开始实现登录页面",error:null,createdAt:clock,startedAt:clock,finishedAt:null});
const runLogs = runHistory.flatMap(run=>Array.from({length:25},(_,seq)=>({seq,projectId:"A",taskId:run.taskId,runId:run.id,stream:"stdout",content:`尝试${run.attempt}日志${seq}`,createdAt:clock+seq})));
if (params.has("launch-mode")) runLogs.unshift({ seq: -1, projectId: "A", taskId: "t-build", runId: "evidence-2",
  stream: "launch_mode", content: "启动模式 · 已保存：Approve for me · 实际：Approve for me（Codex TUI） · 原因：已授权的可写任务继承管理 Runtime", createdAt: clock });
if(params.get("copy")==="1") {
  chats.A.executionPlans=[];
  chats.A.rounds=chats.A.rounds.map((round: any)=>({...round,mode:"discussion",participantAgentIds:["claude"]}));
  chats.A.messages=[message("m1","user","@Claude 请检查接口。\n保留第二行与缩进。"),message("m2","agent","**结果**：已核对。\n\n```ts\nconst answer = 42;\n```\n\n[文档](https://example.com/docs)","claude"),message("m3","agent","","codex","paused")];
}
const activityEvents: any[] = [];
const activityApprovals: any[] = [];
if (params.has("codex-login") && !params.has("login-plan")) {
  chats.A.executionPlans = [];
  chats.A.rounds = [{ ...chats.A.rounds[0], mode: "discussion", status: params.has("login-running") ? "running" : "failed",
    chatDispatch: { kind: "parallel" }, participantAgentIds: ["claude", "codex"], phase: "chat" }];
  chats.A.messages = [message("m1", "user", "请各自回复", null, "completed", "message"),
    message("m2", "agent", "Claude 已回复", "claude"), message("m3", "agent", "", "codex", "failed", "codex_login_required")];
}
let codexReady = false;
if (params.has("delegation")) {
  const sourceAt = new Date(2026, 10, 16, 10, 42).getTime();
  projects[0].name = "登录功能改版";
  agents.splice(0, agents.length,
    { ...agent("claude", "评审 Agent", "claude_code", "reviewer"), avatarId: "orbit" },
    { ...agent("codex", "后端 Agent", "codex", "worker"), avatarId: "block" },
    { ...agent("architect", "架构 Agent", "claude_code", "orchestrator"), avatarId: "sprout" },
    { ...agent("frontend", "前端 Agent", "opencode", "worker"), avatarId: "messenger" },
  );
  chats.A.executionPlans = [];
  chats.A.rounds = [];
  chats.A.supportedDiscussionRuntimes.push("opencode");
  chats.A.delegations = [{
    id: "login-design", sourceAgentId: "codex", targetAgentId: "architect",
    sourceRunId: "backend-source", deliveryRunId: "architecture-delivery", resumeRunId: "backend-resume",
    title: "登录接口设计", brief: "我先负责梳理登录接口的设计方案，确认接口契约和返回结果，然后委派给架构 Agent 评审整体方案。",
    checkpointAt: sourceAt, submittedAt: sourceAt + 6 * 60000, acceptedAt: sourceAt + 6 * 60000 + 1000,
    resumedAt: sourceAt + 7 * 60000, filename: "登录接口契约.md", status: "resumed", failure: null,
  }];
  chats.A.messages = [
    { ...message("delegate-source", "system", "等待设计交付", "codex", "paused", "status"), runId: "backend-source", createdAt: sourceAt },
    { ...message("delegate-delivery", "agent", "已收到委派。我从架构一致性、接口边界和扩展性三个方面完成评审，整体方案可行，建议按当前设计推进。", "architect"), runId: "architecture-delivery", createdAt: sourceAt + 6 * 60000 },
    { ...message("delegate-resume", "agent", "评审通过。我已更新接口契约文档并续跑后续实现，登录接口设计部分可以进入开发阶段。", "codex"), runId: "backend-resume", createdAt: sourceAt + 7 * 60000 },
    { ...message("parallel-frontend", "system", "正在推进登录页面的交互与状态反馈开发…", "frontend", "running", "status"), createdAt: sourceAt + 8 * 60000 },
  ];
  if (params.get("delegation") === "pending") {
    Object.assign(chats.A.delegations[0], { filename: null, deliveryRunId: null, resumeRunId: null,
      submittedAt: null, acceptedAt: null, resumedAt: null, status: "waiting_design" });
    chats.A.messages.splice(1, 2);
  }
  tasks.length = 0;
}
if (params.get("activity") === "1") {
  const at = Date.now();
  projects[0].name = "登录功能改版";
  ["Claude","Codex","Pi"].forEach((name,index)=>{agents[index].name=name;agents[index].avatarId=["orbit","block","sprout"][index] as any;});
  chats.A.executionPlans = [];
  chats.A.rounds = [{...chats.A.rounds[0],id:"activity-round",status:"running",phase:"chat",mode:"discussion",updatedAt:at}];
  chats.A.messages = [message("a0","user","核对登录接口，整理交互方案。"),message("a1","agent","接口参数已核对，我继续查看登录态处理。","claude","running"),message("a2","agent","","codex","running"),message("a3","agent","我已列出空态、加载和失败状态。","kimi")];
  chats.A.messages.forEach((item:any,index:number)=>{item.createdAt=at-60000+index;item.updatedAt=at-2000;item.roundId="activity-round";item.phase="chat";if(item.agentId)item.taskId=`activity-task-${item.agentId}`;});
  runHistory.splice(0,runHistory.length,...agents.slice(0,3).map((item,index)=>({id:`run-${item.id}`,projectId:"A",taskId:`activity-task-${item.id}`,agentId:item.id,attempt:1,status:index===2?"succeeded":"running",sessionLocator:null,leaseExpiresAt:at+120000,lastHeartbeatAt:at-1000,outputSummary:null,error:null,createdAt:at-(index===1?52000:24000),startedAt:at-(index===1?52000:24000),finishedAt:index===2?at-3000:null})));
  const event=(seq:number,offset:number,payload:any,agentId="claude")=>({seq,projectId:"A",runId:`run-${agentId}`,taskId:`activity-task-${agentId}`,agentId,eventType:"agent_activity",payload,createdAt:at-offset});
  activityEvents.push(event(1,21000,{kind:"analysis"}),event(2,16000,{kind:"tool",toolId:"read-auth",operation:"read",path:"src/api/auth.ts",status:"running"}),event(3,14000,{kind:"tool",toolId:"read-auth",status:"completed"}),event(4,4000,{kind:"tool",toolId:"read-session",operation:"read",path:"src/stores/session.ts",status:"running"}),event(5,2000,{kind:"tool",toolId:"read-session"}));
  chats.A.activityEvents = activityEvents;
}
if (params.get("approval") === "1") {
  chats.A.rounds[0].status = "waiting_approval";
  chats.A.messages.find((item:any)=>item.id==="a1").status="waiting_approval";
  runHistory.find(item=>item.id==="run-claude")!.status="waiting_approval";
  activityApprovals.push({id:"permission-claude",projectId:"A",taskId:"activity-task-claude",runId:"run-claude",approvalType:"runtime_permission",summary:"Claude 请求运行命令：npm test\n{\"command\":\"npm test\"}",status:"pending",createdAt:Date.now(),decidedAt:null});
}
const snapshot = (id: string): any => ({ project: projects.find(project => project.id === id), agents: id === "A" ? agents : [agent("b-codex", "文档 Codex", "codex", "worker", "B")], tasks: id === "A" && (params.get("plan") !== "pending" || chats.A.executionPlans.some((plan:any)=>plan.status === "applied")) ? tasks : params.get("task-link")==="1" ? [{...tasks[0],id:"b-task",projectId:"B",title:"文档任务",assigneeAgentId:"b-codex"}] : [], runs: id==="A"?runHistory:[], messages: [], events: [], dependencies: [{ taskId: "t-review", dependsOnTaskId: "t-build" }], agentSessions: (id === "A" ? agents : []).map(agent => ({ agentId: agent.id, appSessionId: `session-${agent.id}`, projectId: id, id: `agent-session-${agent.id}`, status: agent.id === "codex" ? "running" : "idle", controlMode: params.get("manual") === agent.id ? "manual" : "auto", currentTaskId: null, currentRunId: null, lastOutputAt: null, createdAt: clock, updatedAt: clock })), runLogs: id==="A"?runLogs:[], runtimeEvents: [], memories: [], instructions: [], artifacts: [], approvals: [], reviewDecisions: [], wakeups: [] });
const calls: Array<{ command: string; args: any }> = [];
const pending: Array<{ command: string; args: any; resolve(value: any): void; reject(reason: Error): void }> = [];
const receipts = new Map<string, string>();
let failSend = params.get("fail") === "send";
let failBeforeSave = params.get("fail") === "send-before-save";
const planDraftBody = '```json\n' + JSON.stringify({ summary: "范围样例", tasks: [{ writePaths: ["frontend/"] }] }, null, 2) + '\n```';
const planRetryRequests = new Map<string, any>();
const stagedAttachments = new Map<string, { id: string; name: string; bytes: number; mediaType: string }>();
const mock = {
  preview: { state: "idle", url: undefined as string | undefined, error: undefined as string | undefined, logs: undefined as string | undefined },
  previewBrowsed: [] as string[],
  previewVersion: "",
  clipboardWrites: [] as string[], copyError: "", holdCopy: false, pendingCopies: [] as Array<()=>void>, failPermission:false, stalePermission:false,
  calls, pending, chats, projects, snapshot, activityEvents, activityApprovals, runHistory, archivedConversations: [] as any[], holdProject: "", holdSend: false, holdClear: false, holdNew: false, failExecution: false, failApproval: false, failClear: false, failNew: false,
  release(command: string, value?: any, error?: string) { const index = pending.findIndex(item => item.command === command); if (index < 0) throw Error(`No pending ${command}`); const item = pending.splice(index, 1)[0]; if (error) item.reject(Error(error)); else item.resolve(value ?? (command === "ma_snapshot" ? snapshot(item.args.projectId) : chats[item.args.projectId ?? item.args.input.projectId])); },
  failPlanning() {
    const round = chats.A.rounds.at(-1);
    const reply = chats.A.messages.find((item: any) => item.id === "streaming-plan");
    round.status = "needs_decision";
    round.stopReason = "真实成员已回复，但分工计划未通过校验：写入范围必须是明确的项目内相对目录或文件：frontend/；可补充约束后重试";
    reply.status = "completed";
    reply.body = planDraftBody;
  },
  async invoke(command: string, args: any = {}) {
    calls.push({ command, args: structuredClone(args) });
    if (command === "ma_conversation_export") {
      if (chats[args.projectId]?.conversation.id !== args.conversationId) throw Error("会话已变化");
      const scenario = new URLSearchParams(location.search).get("export");
      if (scenario === "failure") throw Error("保存目录无写入权限");
      return scenario === "cancel" ? null : "团队会话-测试.md";
    }
    if(command === "plugin:clipboard-manager|write_text") {
      if(mock.holdCopy)await new Promise<void>(resolve=>mock.pendingCopies.push(resolve));
      if(mock.copyError)throw Error(mock.copyError);
      mock.clipboardWrites.push(args.text);return null;
    }
    if (params.get("slow-open") === "1" && ["ma_probe_runtimes", "ma_check_runtime_updates", "skill_center_snapshot", "collaboration_health"].includes(command)) await new Promise(resolve => setTimeout(resolve, 2500));
    if (params.get("slow-data") === "1" && command === "ma_snapshot" && calls.some(item => item.command === "fixture_open_settings")) await new Promise(resolve => setTimeout(resolve, 2500));
    if (params.get("fail-aux") === "1" && ["ma_probe_runtimes", "skill_center_snapshot"].includes(command)) throw Error("隔离测试：辅助检测失败");
    if (params.get("slow-data") === "1" && command === "ma_snapshot" && calls.some(item => item.command === "fixture_open_settings")) await new Promise(resolve => setTimeout(resolve, 2500));
    if (params.get("fail-aux") === "1" && ["ma_probe_runtimes", "skill_center_snapshot"].includes(command)) throw Error("隔离测试：辅助读取失败");
    if ((["ma_snapshot", "ma_conversation_snapshot"].includes(command) && mock.holdProject === args.projectId) || (command === "ma_conversation_send" && mock.holdSend) || (command === "ma_conversation_clear" && mock.holdClear) || (command === "ma_conversation_new" && mock.holdNew)) return new Promise((resolve, reject) => pending.push({ command, args, resolve, reject }));
    if (command === "ma_list_projects") return structuredClone(projects);
    if (command === "ma_list_interventions") {
      const item={id:"permission-current",projectId:"A",taskId:"activity-task-claude",runId:"run-claude",kind:"approval_required",status:"open",metadata:{approvalId:"permission-claude"}};
      return mock.stalePermission ? [] : [{...item,id:"foreign-project",projectId:"B"},{...item,id:"old-run",runId:"previous-run"},item];
    }
    if (command === "ma_resolve_intervention") {
      if(args.input.interventionId!=="permission-current"||args.input.action!=="approve")throw Error("Wrong permission identity");
      if(mock.failPermission)throw Error("隔离测试：批准未保存，请重试");
      activityApprovals[0].status="approved";
      runHistory.find(item=>item.id==="run-claude")!.status="running";
      chats.A.rounds[0].status="running";chats.A.messages.find((item:any)=>item.id==="a1").status="running";
      return {interventionId:"permission-current",projectId:"A",resumeProject:false,message:"已批准本次工具操作"};
    }
    if (command === "ma_snapshot") { const result=snapshot(args.projectId); if(params.get("activity")==="1"&&args.projectId==="A"){result.runtimeEvents=activityEvents;result.approvals=activityApprovals;} if(resumedDelivery && args.projectId==="A")result.approvals=[resumedDelivery]; if(params.has("logs-only"))result.runLogs=[]; return structuredClone(result); }
    if (resumedDelivery && args.taskId === resumedDelivery.taskId && command === "ma_update_resumed_delivery") {
      resumedDelivery.status = "pending";
      tasks[1].status = "in_review";
      tasks[2].status = "blocked";
      return structuredClone(tasks[1]);
    }
    if (resumedDelivery && args.taskId === resumedDelivery.taskId && command === "ma_approve_task") {
      resumedDelivery.status = "approved";
      tasks[1].status = "done";
      tasks[2].status = "ready";
      return structuredClone(tasks[1]);
    }
    if (command === "ma_run_log_history") {
      return structuredClone(runLogs.filter(log => log.projectId === args.projectId && log.runId === args.runId).slice(-args.limit));
    }
    if (command === "ma_conversation_snapshot") return structuredClone(chats[args.projectId]);
    if (["ma_conversation_member_models", "ma_set_conversation_member_models"].includes(command)) {
      const key = `${args.projectId}:${args.agentId}`;
      if (command === "ma_set_conversation_member_models") memberModelSettings.set(key, args.selection);
      return { supported: true, observed: true,
        selection: memberModelSettings.get(key) ?? {model: null, reasoning: null},
        model: [{value: "fixture-a", label: "测试模型 A"}], reasoning: [{value: "high", label: "高"}] };
    }
    if (command === "ma_conversation_codex_login") {
      if (params.has("hold-login")) await new Promise((resolve, reject) => pending.push({ command, args, resolve, reject }));
      codexReady = true;
      return;
    }
    if (command === "ma_conversation_retry_codex_members") {
      const chat = chats[args.input.projectId];
      if (!codexReady || args.input.expectedConversationId !== chat.conversation.id || args.input.roundId !== "round-1"
        || JSON.stringify(args.input.turns) !== JSON.stringify([{ messageId: "m3", runId: "run-codex" }])) throw Error("错误的成员重试");
      Object.assign(chat.messages.find((m: any) => m.id === "m3"), { kind: "reply", status: "completed", runId: "run-codex-retry", body: "Codex 补发成功" });
      chat.rounds[0].status = "completed";
      return structuredClone(chat);
    }
    if (command === "ma_conversation_plan_preflight" && params.has("login-plan") && !codexReady)
      return [{ kind: "codex_login", message: "CrewVela 的 Codex 专用目录尚未登录" }];
    if (command === "ma_conversation_execute_plan" && params.get("login-plan") === "apply" && !codexReady)
      throw Error("CrewVela 的 Codex 专用目录尚未登录");
    if (command === "ma_conversation_plan_retry_input") {
      const input = planRetryRequests.get(args.roundId);
      if (!input || input.projectId !== args.projectId) throw Error("无法重试其他项目的分工");
      return { ...input, expectedConversationId: args.expectedConversationId, clientMessageId: args.clientMessageId };
    }
    if (command === "ma_conversation_plan_preflight") return params.get("plan-blocked") === "1" ? [
      { kind: "git", message: "项目目录缺少 Git 工作区，请选择已初始化的 Git 项目目录。" },
      { kind: "member", message: "本轮缺少 Codex 审查成员。" },
      { kind: "member", message: "本轮缺少「集成」角色。" },
    ] : [];
    if (command === "ma_conversation_design_delivery") {
      if (args.projectId !== "A" || args.conversationId !== "conversation-A" || args.delegationId !== "login-design") throw Error("交付标识不匹配");
      return { filename: "登录接口契约.md", markdown: "# 登录接口契约\n\n登录成功返回 200，认证失败返回 401。",
        acceptanceEvidence: [{ criterion: "描述错误码", evidence: "接口契约包含成功和失败响应。", met: true }] };
    }
    if (command === "ma_conversation_history") {
      const current = chats[args.projectId];
      const archived = mock.archivedConversations.filter(chat => chat.conversation.projectId === args.projectId);
      return [current, ...archived].map((chat, index) => ({
        ...chat.conversation, current: index === 0, legacy: index > 0 && params.has("legacy-history"),
        lastMessageAt: chat.messages.at(-1)?.createdAt ?? null,
        preview: chat.messages.find((message: any) => message.senderType === "user")?.body ?? null,
      }));
    }
    if (command === "ma_conversation_switch") {
      const input = args.input, chat = chats[input.projectId];
      if (input.expectedConversationId !== chat.conversation.id) throw Error("当前会话已变化");
      const index = mock.archivedConversations.findIndex(item => item.conversation.projectId === input.projectId && item.conversation.id === input.conversationId);
      if (index < 0) throw Error("该会话已不可用");
      const [target] = mock.archivedConversations.splice(index, 1, structuredClone(chat));
      chats[input.projectId] = target;
      return structuredClone(target);
    }
    if (command === "ma_conversation_rename") {
      const input = args.input, chat = chats[input.projectId];
      const target = chat.conversation.id === input.conversationId ? chat : mock.archivedConversations.find(item =>
        item.conversation.projectId === input.projectId && item.conversation.id === input.conversationId);
      if (!target) throw Error("该会话已不可用");
      target.conversation.title = input.title;
      return structuredClone(chat);
    }
    if (command === "ma_conversation_delete") {
      const input = args.input, chat = chats[input.projectId];
      if (input.expectedConversationId !== chat.conversation.id
        || JSON.stringify([...input.expectedMessageIds].sort()) !== JSON.stringify(chat.messages.map((message: any) => message.id).sort())) throw Error("讨论记录已变化");
      chats[input.projectId] = {
        ...chat, conversation: { ...chat.conversation, title: null, id: crypto.randomUUID(), createdAt: Date.now() },
        messages: [], rounds: [], executionPlans: [],
        retainedExecutionPlans: [...(chat.retainedExecutionPlans ?? []), ...chat.executionPlans.filter((plan: any) => plan.status === "applied")],
      };
      return structuredClone(chats[input.projectId]);
    }
    if (command === "ma_conversation_new") {
      if (mock.failNew) throw Error("隔离测试：新建会话失败，原记录保留");
      const input = args.input, chat = chats[input.projectId];
      if (chat.rounds.some((round: any) => ["queued","running","waiting_approval","stopping","unconfirmed"].includes(round.status))) throw Error("讨论正在进行，暂不能新建会话");
      if (input.expectedConversationId !== chat.conversation.id || JSON.stringify([...input.expectedMessageIds].sort()) !== JSON.stringify(chat.messages.map((message: any) => message.id).sort())) throw Error("讨论记录已变化，请刷新后重试新建会话");
      mock.archivedConversations.push(structuredClone(chat));
      chat.conversation = { ...chat.conversation, title: null, id: crypto.randomUUID(), createdAt: Date.now() };
      chat.messages = []; chat.rounds = []; chat.executionPlans = [];
      return structuredClone(chat);
    }
    if (command === "ma_conversation_clear") {
      if (mock.failClear) throw Error("隔离测试：清空失败，记录未更改");
      const input = args.input, chat = chats[input.projectId];
      if (chat.rounds.some((round: any) => ["queued","running","waiting_approval","stopping","unconfirmed"].includes(round.status))) throw Error("讨论正在进行，暂不能清空记录");
      if (input.expectedConversationId !== chat.conversation.id || JSON.stringify([...input.expectedMessageIds].sort()) !== JSON.stringify(chat.messages.map((message: any) => message.id).sort())) throw Error("讨论记录已变化，请刷新后重新确认清空");
      chats[input.projectId] = { ...chat, conversation: { ...chat.conversation, title: null, id: crypto.randomUUID() }, messages: [], rounds: [], executionPlans: [] };
      return structuredClone(chats[input.projectId]);
    }
    if (command === "ma_conversation_stage_attachments") {
      return args.paths.map((path: string) => {
        const file = { id: crypto.randomUUID(), name: path.split(/[\\/]/).pop()!, bytes: 21,
          mediaType: path.endsWith(".png") ? "image/png" : path.endsWith(".mp4") ? "video/mp4" : "text/markdown" };
        stagedAttachments.set(file.id, file);
        return file;
      });
    }
    if (command === "ma_conversation_preview_image") {
      if (params.has("broken-image")) throw Error("图片已丢失");
      const canvas = document.createElement("canvas");
      canvas.width = 360;
      canvas.height = 220;
      const ctx = canvas.getContext("2d")!;
      ctx.fillStyle = "#cc6746";
      ctx.fillRect(0, 0, 360, 220);
      ctx.fillStyle = "#ffe6c9";
      ctx.fillRect(50, 50, 260, 120);
      const blob = await new Promise<Blob>(resolve => canvas.toBlob(blob => resolve(blob!), "image/png"));
      return blob.arrayBuffer();
    }
    if (command === "ma_conversation_open_attachment") return null;
    if (command === "ma_conversation_preview_video") return params.has("broken-video")
      ? "/tests/fixtures/collaboration-conversation-ui.html"
      : "/tests/fixtures/conversation-preview.webm";
    if (command === "ma_conversation_open_video") return null;
    if (["ma_conversation_send", "ma_conversation_interrupt"].includes(command)) {
      const input = args.input, chat = chats[input.projectId];
      if (params.get("natural") === "1") {
        if (receipts.has(input.clientMessageId)) return structuredClone(chat);
        receipts.set(input.clientMessageId,input.projectId);
        const planningStates = params.has("planning-states");
        const firstPlan = planningStates && input.mode === "execution_plan" && !chat.rounds.some((item: any) => item.mode === "execution_plan");
        if (!planningStates) chat.rounds.forEach((round: any)=>{round.status="paused";});
        let user=chat.messages.find((m: any)=>m.clientMessageId===input.clientMessageId);
        if(user){user.status="completed";user.queuedInput=null;}else{user={...message(input.clientMessageId,"user",input.body,null,"completed","message"),clientMessageId:input.clientMessageId,createdAt:clock+50000,attachments:input.attachmentIds?.map((id: string) => stagedAttachments.get(id))};chat.messages.push(user);}
        const roundId=`round-${input.clientMessageId}`;
        chat.rounds.push({id:roundId,projectId:input.projectId,conversationId:chat.conversation.id,userMessageId:user.id,participantAgentIds:input.participantAgentIds,chatDispatch:input.chatDispatch,mode:input.mode,intent:input.intent,status:"completed",phase:input.mode==="execution_plan"?"plan":input.intent==="exchange"?"exchange":"chat",maxRounds:input.maxRounds,currentRound:0,deadlineAt:clock+600000,stopReason:null,createdAt:clock+50000,updatedAt:clock+50000});
        if (input.managed && params.has("managed-test")) {
          const { rootPath: root, defaultBranch: branch } = projects.find(project => project.id === input.projectId)!;
          Object.assign(chat.rounds.at(-1), { status: "running", managedTarget: { root, branch } });
          return structuredClone(chat);
        }
        if (firstPlan) {
          planRetryRequests.set(roundId, structuredClone(input));
          chat.rounds.at(-1).status = "running";
          chat.messages.push({ ...message("streaming-plan", "agent", '{"summary":"分工正在返回', input.participantAgentIds[0], "running"),
            roundId, phase: "plan", replyToMessageId: user.id, createdAt: clock + 50001 });
          return structuredClone(chat);
        }
        if (input.mode === "execution_plan" && params.get("propose") === "1") {
          const proposal = {...message("generated-proposal", "agent", planningStates ? planDraftBody : "", input.participantAgentIds[0], "completed", "plan"),
            roundId, phase: "plan", replyToMessageId: user.id, createdAt: clock + 50001 };
          chat.messages.push(proposal);
          chat.executionPlans.push({ id: "generated-plan", projectId: input.projectId, roundId,
            messageId: proposal.id, summary: "先实现，再审查与联调", tasks: structuredClone(planned),
            status: "pending", taskIds: [], createdAt: clock + 50005 });
          return structuredClone(chat);
        }
        if (planningStates && input.mode === "discussion") {
          chat.messages.push({ ...message("plain-json", "agent", planDraftBody, input.participantAgentIds[0]),
            roundId, phase: "chat", replyToMessageId: user.id, createdAt: clock + 50008 });
          return structuredClone(chat);
        }
        const responding = input.chatDispatch?.kind === "assigned" ? [input.chatDispatch.agentId] : input.participantAgentIds;
        for(const [index,agentId] of responding.entries()) chat.messages.push({...message(`natural-${chat.messages.length}`,"agent",input.intent==="exchange"?(index===0?"@Codex 我建议先核对接口，你觉得呢？":"我看到了前面的建议，接口一致，可以开始下一步。"):"你好！有什么需要我帮忙？",agentId),roundId,phase:"chat",replyToMessageId:input.intent==="exchange"&&index?chat.messages.at(-1).id:user.id,createdAt:clock+50001+index});
        return structuredClone(chat);
      }
      if (failBeforeSave) { failBeforeSave = false; throw Error("隔离测试：响应丢失，发送结果未确认"); }
      if (!receipts.has(input.clientMessageId)) {
        receipts.set(input.clientMessageId, input.projectId);
        chat.messages.push({ ...message(input.clientMessageId, "user", input.body, null, "completed", "message"), clientMessageId: input.clientMessageId, projectId: input.projectId, conversationId: chat.conversation.id, createdAt: clock + 50000, updatedAt: clock + 50000 });
        chat.rounds.push({ id: `round-${input.clientMessageId}`, projectId: input.projectId, conversationId: chat.conversation.id, userMessageId: input.clientMessageId, participantAgentIds: input.participantAgentIds ?? ["claude", "codex"], mode: input.mode ?? "discussion", status: command === "ma_conversation_interrupt" ? "paused" : "running", phase: "opinions", maxRounds: 1, currentRound: 0, deadlineAt: clock + 600000, stopReason: command === "ma_conversation_interrupt" ? "人工插话已保存，后续发言暂停" : null, createdAt: clock + 50000, updatedAt: clock + 50000 });
      }
      if (failSend) { failSend = false; throw Error("隔离测试：响应丢失，发送结果未确认"); }
      return structuredClone(chat);
    }
    if (command === "ma_conversation_control") { const chat = chats[args.projectId]; if (chat.rounds.length) chat.rounds[chat.rounds.length - 1].status = params.get("stopping") === "1" ? "stopping" : args.action === "pause" ? "paused" : "cancelled"; if(params.get("activity")==="1"&&params.get("stopping")!=="1"){for(const item of chat.messages.filter((item:any)=>item.senderType==="agent"&&item.status==="running")){item.status=args.action==="pause"?"paused":"cancelled";item.updatedAt=Date.now();const run=runHistory.find(run=>run.id===item.runId);if(run){run.status=args.action==="pause"?"interrupted":"cancelled";run.finishedAt=Date.now();}}} return structuredClone(chat); }
    if (command === "ma_conversation_execute_plan") { if(mock.failApproval) throw Error("隔离测试：分工确认失败"); const chat = chats[args.input.projectId]; const plan = chat.executionPlans.find((item: any) => item.id === args.input.planId); plan.status = "applied"; plan.taskIds = ["t-design", "t-build", "t-review"]; return structuredClone(chat); }
    if (command === "workspace_reveal_path" && params.has("managed-test")) return null;
    if (command === "ma_project_preview_status") return { ...mock.preview, logs: args.logs ? mock.preview.logs : undefined };
    if (command === "ma_project_preview_save") {
      chats[args.projectId].executionPlans.find((plan: any) => plan.id === args.planId).managed.preview = args.config;
      return null;
    }
    if (command === "ma_project_preview_start") {
      if (mock.previewVersion && args.confirmedVersion !== mock.previewVersion) return {
        kind: "confirm", version: mock.previewVersion, message: "当前目录已切换版本，继续启动？",
      };
      if (mock.preview.state !== "running") mock.preview = { state: "starting", url: undefined, error: undefined, logs: undefined };
      return { kind: "status", status: { ...mock.preview } };
    }
    if (command === "ma_project_preview_stop") {
      mock.preview = { state: "idle", url: undefined, error: undefined, logs: undefined };
      return mock.preview;
    }
    if (command === "ma_pause_project" || command === "ma_start_project") { if(mock.failExecution) return {accepted:false,message:"隔离测试：执行控制未确认"}; projects.find(project => project.id === args.projectId)!.status = command === "ma_pause_project" ? "paused" : "active"; return { accepted: true, projectId: args.projectId }; }
    if (command === "ma_confirm_run_stopped") { if(params.get("confirm-fails")==="1")throw Error("fixture 确认失败"); const run=runHistory.find(run=>run.id===args.runId);if(!run||args.confirmedStopped!==true)throw Error("Wrong run confirmation");run.sessionLocator=null;return null; }
    if (["ma_probe_runtimes","ma_check_runtime_updates"].includes(command)) return agents.map(agent=>({kind:agent.runtimeKind,installed:true,version:"fixture",capabilities:[],error:null}));
    if (command === "skill_center_snapshot") return {skillSets:[]};
    if (command === "collaboration_health") return {scopeLabel:"当前项目",sessions:[],runtimes:[],messages:[],handoffs:[],cliPath:"fixture",cliPathExists:true,replyEvidence:"fixture"};
    if (command === "general_settings_get") return GENERAL_SETTINGS_PREVIEW;
    if (command === "plugin:opener|open_url") return null;
    if (command === "plugin:event|emit") return null;
    if (command === "ma_create_project") { const project = { ...args.input, id: crypto.randomUUID(), status: "active", createdAt: clock, updatedAt: clock }; projects = [...projects, project]; chats[project.id] = emptyChat(project.id); return project; }
    if (command === "ma_project_git_status") return { state: "missingRepository", repositoryPath: null, hasFiles: false, stagedFiles: [], stagedToken: null };
    if (command === "plugin:dialog|open") {
      if (params.has("video")) return ["/fixture/操作演示.mp4"];
      return params.has("attachments") ? ["/fixture/设计截图.png", "/fixture/需求说明.md"] : "/fixture/new-project";
    }
    if (command === "get_runtime_preferences") return { disabledRuntimes: params.get("disabled") ? [params.get("disabled")] : [] };
    if (command === "plugin:event|listen") return 1;
    if (command === "plugin:event|unlisten") return null;
    throw Error(`隔离页面未模拟 ${command}`);
  },
};
window.addEventListener("crewvela:browse", (event) => {
  mock.previewBrowsed.push((event as CustomEvent<{ url: string }>).detail.url);
});
if(params.get("queued")==="1") {
 const input={projectId:"A",clientMessageId:"pending-original",body:"@Codex 先回答这个问题",participantAgentIds:["codex"],mode:"discussion",intent:"chat",maxRounds:1,maxDurationSeconds:600};
 chats.A.rounds[0].status=params.get("unconfirmed")==="1"?"unconfirmed":"needs_decision";
 chats.A.messages.push({...message("queued-original","user",input.body,null,"needs_decision","message"),clientMessageId:input.clientMessageId,queuedInput:input});
}
if(params.get("markdown")==="1") chats.A.messages.push(message("m90","agent",'# 方案\n\n**已经确认**，使用 `read()`。\n\n- 第一步\n- 第二步\n\n```js\nconst value = "<safe>";\n```\n\n| 名称 | 状态 |\n| --- | --- |\n| 接口 | 完成 |\n\n[安全链接](https://example.com/) [危险链接](javascript:alert(1))\n\n![不要自动加载](https://example.invalid/tracking.png)\n\n<script>window.unsafeExecuted=true</script>',"claude"));
(window as any).conversationMock = mock;
(window as any).__TAURI_INTERNALS__ = { invoke: mock.invoke, transformCallback: () => 1, convertFileSrc: (path: string) => path };
function Fixture() {
  const [taskRequest,setTaskRequest] = useState<CollaborationTasksRequest>();
  const taskSequence = useRef(0);
  const [appearance,setAppearance] = useState<AppAppearance>({...DEFAULT_APPEARANCE,theme:params.get("theme")==="light"?"light":"dark"});
  const [active, setActive] = useState(true);
  const [instance, setInstance] = useState(0);
  (mock as any).setActive = setActive;
  (mock as any).selectInstance = setInstance;
  const panel = (index: number) => <MultiAgentControl active={active && index === instance} defaultCwd="/fixture/todo-app" onOpenSession={(id, original) => { calls.push({ command: "fixture_open_session", args: { sessionId: id, original } }); }} onOpenProjectSettings={(id,target) => { calls.push({ command: "fixture_open_settings", args: { projectId: id, target } }); if(id&&target==="tasks")setTaskRequest({projectId:id,requestId:++taskSequence.current}); }} />;
  (mock as any).openTasks = (projectId:string) => setTaskRequest({projectId,requestId:++taskSequence.current});
  return <main className="conversation-fixture-frame"><header className="conversation-fixture-resource"><span><Robot size={21} />多智能体<XMark /></span><small>隔离示例 · 无真实 Agent 调用</small><CornersOut size={19} /></header>{params.get("header")==="1"&&<div className="conversation-shell-header" aria-hidden="true"/>}{params.get("instances") === "2" ? [0,1].map(index => <div style={{ display: index === instance ? "flex" : "none", flex: 1, minHeight: 0 }} key={index} data-instance={index}>{panel(index)}</div>) : panel(0)}{taskRequest && <SettingsDialog initialSection="collaboration" initialCollaborationProjectId={taskRequest.projectId} initialCollaborationTasksRequest={taskRequest} sessions={[]} archivedSessions={[]} projects={[]} appearance={appearance} resolvedAppearanceTheme={params.get("theme")==="light"?"light":"dark"} petSettings={DEFAULT_PET_SETTINGS} notificationSettings={DEFAULT_NOTIFICATION_SETTINGS} onAppearanceChange={setAppearance} onPetSettingsChange={()=>{}} onNotificationSettingsChange={()=>{}} onRestoreArchivedSession={()=>{}} onDeleteArchivedSession={()=>{}} onViewArchivedTranscript={()=>{}} onClose={()=>setTaskRequest(undefined)}/>}</main>;
}
function XMark() { return <X size={17} aria-hidden="true" style={{ marginLeft: 18 }} />; }
const style = document.createElement("style"); style.textContent = 'html,body,#root{height:100%;margin:0;overflow:hidden}.conversation-fixture-frame{height:100%;display:flex;flex-direction:column;background:var(--bg-elev);color:var(--text)}.conversation-fixture-resource{height:64px;flex:none;display:flex;align-items:center;gap:18px;padding:0 30px;border-bottom:1px solid var(--border);background:var(--bg)}.conversation-fixture-resource>span{white-space:nowrap;flex:none;display:flex;align-items:center;gap:9px;padding:8px 13px;border-radius:8px;background:var(--bg-hover);font:600 17px var(--display-font)}.conversation-fixture-resource>small{margin-left:auto;color:var(--text-dim);font:11px var(--display-font)}@media(max-width:480px){.conversation-fixture-resource{padding:0 14px;gap:8px}.conversation-fixture-resource>small{font-size:9px}}'; document.head.append(style);
const hot = import.meta.hot as { data: { root?: Root } } | undefined;
const root = hot?.data.root ?? createRoot(document.getElementById("root")!); if (hot) hot.data.root = root; root.render(<Fixture />);
