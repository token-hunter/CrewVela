import type { AgentRun, ApprovalRecord, ConversationMessage, RuntimeEventRecord } from "./types";

export type ActivityPhase = "queued" | "waiting" | "analysis" | "reply" | "read" | "search" | "web" | "command" | "edit" | "tool" | "tool_pending" | "approval" | "stopping" | "unconfirmed" | "completed" | "failed" | "cancelled" | "paused";
export interface ActivityStep { id: string; at: number; label: string; status: "completed" | "running" | "pending" | "failed" | "stopped" }
export interface MemberActivity {
  phase: ActivityPhase; label: string; elapsed: number; lastActivityAt: number | null;
  stale: boolean; heartbeat: "normal" | "stale" | "unknown"; live: boolean; steps: ActivityStep[];
}
const LABELS: Record<ActivityPhase, string> = { queued: "等待发起", waiting: "等待响应", analysis: "正在分析", reply: "正在生成回复", read: "正在读取文件", search: "正在检索", web: "正在检索网页", command: "正在运行命令", edit: "正在编辑文件", tool: "正在调用工具", tool_pending: "等待工具执行", approval: "等待你确认", stopping: "正在停止", unconfirmed: "停止状态待核实", completed: "已完成", failed: "回复失败", cancelled: "已取消", paused:"已暂停" };
const TOOL_LABELS: Record<string,string> = {read:"读取",search:"检索",web:"检索网页",command:"运行命令",edit:"编辑",tool:"调用工具"};
const TERMINAL_RUNS = new Set(["succeeded", "failed", "timed_out", "cancelled", "interrupted"]);
const TERMINAL_MESSAGES = new Set(["completed", "failed", "cancelled", "paused"]);
export const secondsSince = (now: number, at: number) => Math.max(0, Math.floor((now - at) / 1000));
export const activityDuration = (seconds: number) => seconds < 60 ? `${seconds}秒` : seconds < 3600 ? `${Math.floor(seconds/60)}分${seconds%60}秒` : `${Math.floor(seconds/3600)}小时${Math.floor(seconds%3600/60)}分`;

/** Correlate exact run identity. Scheduler/session timestamps never imply thought. */
export function conversationMemberActivity(message: ConversationMessage, candidate: AgentRun | undefined, events: RuntimeEventRecord[], approvals: ApprovalRecord[], now: number): MemberActivity {
  const run = candidate?.id === message.runId && candidate.projectId === message.projectId && candidate.agentId === message.agentId && (!message.taskId || candidate.taskId === message.taskId) ? candidate : undefined;
  const own = events.filter(event => event.eventType === "agent_activity" && event.projectId === message.projectId && event.runId === message.runId && event.agentId === message.agentId && (!message.taskId || event.taskId === message.taskId))
    .sort((a,b) => a.createdAt - b.createdAt || a.seq - b.seq);
  const start = run?.startedAt ?? run?.createdAt ?? message.createdAt;
  const unconfirmed = message.status === "unconfirmed" || run?.sessionLocator?.stopUnverified === true;
  const terminal = !unconfirmed && (TERMINAL_MESSAGES.has(message.status) || (!!run && TERMINAL_RUNS.has(run.status)));
  const live = !terminal && (unconfirmed || ["running", "queued", "waiting_approval", "needs_decision", "stopping", "unconfirmed"].includes(message.status));
  const end = terminal ? run?.finishedAt ?? message.updatedAt : now;
  let phase: ActivityPhase = message.status === "queued" ? "queued" : "waiting";
  const steps: ActivityStep[] = [{id:"sent",at:start,label:message.status === "queued" && !run ? "等待发起请求" : "请求已发送",status:message.status === "queued" && !run ? "pending" : "completed"}];
  let lastActivityAt: number | null = null;
  const tools = new Map<string, ActivityStep & { operation: ActivityPhase; path?: string }>();
  for (const event of own) {
    const payload = event.payload;
    if (payload.kind === "tool" && typeof payload.toolId === "string") {
      const operation = typeof payload.operation === "string" && Object.prototype.hasOwnProperty.call(TOOL_LABELS,payload.operation) ? payload.operation as ActivityPhase : undefined;
      let tool = tools.get(payload.toolId);
      const op = operation ?? tool?.operation ?? "tool";
      const path = typeof payload.path === "string" && payload.path.length <= 512 && !/[\r\n\x00]/.test(payload.path) && !payload.path.includes("://") ? payload.path : undefined;
      if (!tool) {
        tool = {id:`tool:${payload.toolId}`,at:event.createdAt,label:TOOL_LABELS[op],status:"pending",operation:op};
        tools.set(payload.toolId,tool); steps.push(tool);
      }
      tool.operation = op;
      if (path) tool.path = path;
      tool.label = `${TOOL_LABELS[op]}${tool.path ? ` ${tool.path}` : ""}`;
      if (["completed","failed","pending","running"].includes(String(payload.status))) tool.status = payload.status as ActivityStep["status"];
      // A late completion of an older tool must not replace newer active work.
      const activeTool = [...tools.values()].reverse().find(item => ["running","pending"].includes(item.status));
      phase = activeTool?.status === "pending" ? "tool_pending" : activeTool?.operation ?? "waiting";
      lastActivityAt = event.createdAt;
    } else if (payload.kind === "analysis" || payload.kind === "reply") {
      phase = payload.kind;
      const id = `${payload.kind}:${event.seq}`;
      const label = phase === "analysis" ? "收到分析活动信号" : "正在生成回复";
      if (steps[steps.length-1]?.label !== label) steps.push({id,at:event.createdAt,label,status:"completed"});
      lastActivityAt = event.createdAt;
    }
  }
  if (run?.status === "waiting_approval" || run?.status === "waiting_input" || ["waiting_approval","needs_decision"].includes(message.status) || approvals.some(item => item.projectId === message.projectId && item.runId === message.runId && item.status === "pending" && !!message.runId)) phase = "approval";
  if (message.status === "stopping") phase = "stopping";
  if (unconfirmed) phase = "unconfirmed";
  else if (terminal) {
    phase = message.status === "failed" || run?.status === "failed" || run?.status === "timed_out" ? "failed" : message.status === "paused" ? "paused" : message.status === "cancelled" || run?.status === "cancelled" || run?.status === "interrupted" ? "cancelled" : ["needs_decision","waiting_approval"].includes(message.status) ? "approval" : "completed";
    steps.forEach(step => { if (step.status === "running" || step.status === "pending") step.status = "stopped"; });
    steps.push({id:"end",at:end,label:LABELS[phase],status:phase === "failed" ? "failed" : "completed"});
  }
  const stale = live && !["queued","approval","stopping","unconfirmed"].includes(phase) && secondsSince(now,lastActivityAt ?? start) >= 45;
  const heartbeat = run?.lastHeartbeatAt == null ? "unknown" : secondsSince(now,run.lastHeartbeatAt) <= 45 ? "normal" : "stale";
  return {phase,label:LABELS[phase],elapsed:secondsSince(end,start),lastActivityAt,stale,heartbeat,live,steps:steps.slice(-14)};
}
