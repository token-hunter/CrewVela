import type { ApprovalRecord, ConversationMessage, ConversationSnapshot, MultiAgentSnapshot } from "./types";
import { latestConversationRound } from "./collaborationConversation.ts";
import { visibleProjectTasks } from "./collaborationTasks.ts";

export interface ConversationWorkRecord {
  projectId: string;
  conversationId: string;
  topicTitle: string | null;
  kind: "plan" | "round" | "queued" | "origin";
  id: string;
  status: string;
  messageId: string;
  request: string;
  summary: string | null;
  updatedAt: number;
}

export interface CollaborationWorkItem {
  id: string;
  projectId: string;
  group: "attention" | "waiting";
  label: string;
  title: string;
  dependencies?: string[];
  updatedAt: number;
  agentId?: string | null;
  origin?: ConversationWorkRecord;
  taskId?: string;
  approval?: ApprovalRecord;
}

const taskLabels: Record<string, string> = {
  ready: "等待调度", claimed: "成员正在启动", running: "成员执行中", blocked: "等待依赖",
  changes_requested: "等待修改", waiting_approval: "等待确认", in_review: "等待交付审批",
};

export function buildCollaborationWorkQueue(snapshot: MultiAgentSnapshot | null, chat: ConversationSnapshot | null): CollaborationWorkItem[] {
  if (!snapshot || !chat || chat.conversation.projectId !== snapshot.project.id) return [];
  const projectId = snapshot.project.id;
  const topicId = chat.conversation.id;
  const messages = chat.messages.filter(message => message.projectId === projectId && message.conversationId === topicId);
  const byId = new Map(messages.map(message => [message.id, message]));
  const rounds = chat.rounds.filter(round => round.projectId === projectId && round.conversationId === topicId);
  const requests = new Map(rounds.map(round => [round.id, byId.get(round.userMessageId)]));
  const topicTitle = chat.conversation.title || messages.find(message => message.senderType === "user" && message.kind !== "queued_message")?.body.trim().slice(0, 80) || null;
  const makeRecord = (kind: ConversationWorkRecord["kind"], id: string, status: string, messageId: string, request: ConversationMessage | undefined, updatedAt: number, summary: string | null = null): ConversationWorkRecord =>
    ({ kind, id, status, projectId, conversationId: topicId, topicTitle, messageId, request: request?.body ?? "", updatedAt, summary });
  const records = (chat.workItems ?? []).filter(record => record.projectId === projectId);
  const origins = new Map(records.filter(record => record.kind === "origin").map(record => [record.id, record]));
  const topicWork = records.filter(record => record.kind !== "origin" && record.conversationId !== topicId);
  const round = latestConversationRound(rounds);
  const request = round && byId.get(round.userMessageId);
  if (round && request) topicWork.push(makeRecord("round", round.id, round.status, request.id, request, round.updatedAt, round.stopReason));
  for (const plan of chat.executionPlans) {
    const request = requests.get(plan.roundId);
    if (!request || plan.projectId !== projectId) continue;
    const record = makeRecord("plan", plan.id, plan.status, plan.messageId, request, plan.createdAt, plan.summary);
    if (plan.status === "pending" && !plan.managed) topicWork.push(record);
    for (const id of plan.taskIds) origins.set(id, record);
  }
  const replyByRun = new Map<string, ConversationWorkRecord>();
  for (const message of messages) {
    if (message.queuedInput) topicWork.push(makeRecord("queued", message.id, message.status, message.id, message, message.updatedAt));
    if (message.senderType !== "agent" || !message.roundId || !message.runId) continue;
    const request = requests.get(message.roundId);
    replyByRun.set(message.runId, makeRecord("origin", message.taskId ?? message.runId, message.status, message.id, request, message.updatedAt));
  }

  const items: CollaborationWorkItem[] = [];
  const coveredTasks = new Set<string>();
  const attentionTopics = new Set<string>();
  const tasks = new Map(visibleProjectTasks(snapshot.tasks.filter(task => task.projectId === projectId)).map(task => [task.id, task]));
  const runs = new Map(snapshot.runs.filter(run => run.projectId === projectId).map(run => [run.id, run]));
  for (const approval of snapshot.approvals) {
    if (approval.projectId !== projectId || approval.status !== "pending") continue;
    const run = approval.runId ? runs.get(approval.runId) : undefined;
    const task = approval.taskId ? tasks.get(approval.taskId) : undefined;
    const candidate = approval.runId ? replyByRun.get(approval.runId) : undefined;
    const reply = candidate && (!approval.taskId || candidate.id === approval.taskId || (candidate.id === run?.id && run.taskId === approval.taskId)) ? candidate : undefined;
    const origin = reply ?? (approval.taskId ? origins.get(approval.taskId) : undefined);
    const target = task?.id;
    items.push({ id: `approval:${approval.id}`, projectId, group: "attention", approval, origin, taskId: target,
      title: approval.summary.split("\n")[0] || task?.title || "", label: approval.approvalType === "task_review" ? "等待交付审批" : "请求授权",
      updatedAt: approval.createdAt, agentId: run?.agentId ?? task?.assigneeAgentId });
    if (approval.taskId) coveredTasks.add(approval.taskId);
    if (origin && !target) attentionTopics.add(origin.conversationId);
  }
  for (const run of runs.values()) {
    if (run.status !== "waiting_input" || coveredTasks.has(run.taskId)) continue;
    const task = tasks.get(run.taskId);
    const origin = replyByRun.get(run.id) ?? origins.get(run.taskId);
    if (!origin && !task) continue;
    items.push({ id: `input:${run.id}`, projectId, group: "attention", origin, taskId: task?.id,
      title: run.outputSummary || task?.title || origin?.request || "", label: "成员需要补充信息", updatedAt: run.lastHeartbeatAt ?? run.createdAt, agentId: run.agentId });
    coveredTasks.add(run.taskId);
    if (origin && !task) attentionTopics.add(origin.conversationId);
  }
  for (const task of tasks.values()) {
    const label = taskLabels[task.status];
    if (!label || coveredTasks.has(task.id)) continue;
    const attention = task.status === "waiting_approval" || (task.status === "in_review" && !task.reviewerAgentId);
    const dependencies = task.status === "blocked" ? snapshot.dependencies.filter(edge => edge.taskId === task.id)
      .map(edge => tasks.get(edge.dependsOnTaskId)).flatMap(parent => parent && parent.status !== "done" ? [parent.title] : []) : undefined;
    items.push({ id: `task:${task.id}`, projectId, group: attention ? "attention" : "waiting", taskId: task.id, origin: origins.get(task.id),
      title: task.title, label, dependencies, updatedAt: task.updatedAt, agentId: task.assigneeAgentId });
  }
  for (const origin of topicWork) {
    const { status, kind } = origin;
    if (kind === "plan" && status !== "pending") continue;
    const attention = kind === "plan" || ["needs_decision", "unconfirmed", "waiting_approval"].includes(status);
    if (!attention && !["queued", "running", "stopping", "paused"].includes(status)) continue;
    if (kind === "round" && (status === "paused" || (attention && attentionTopics.has(origin.conversationId)))) continue;
    const label = kind === "plan" ? "分工待确认" : kind === "queued" ? attention ? "排队消息需确认" : status === "paused" ? "排队消息已暂停" : "等待发送"
      : status === "stopping" ? "正在停止" : status === "unconfirmed" ? "结果待核对" : attention ? "本轮需要处理" : "等待成员回复";
    items.push({ id: `${kind}:${origin.id}`, projectId, group: attention ? "attention" : "waiting", origin,
      title: origin.summary || origin.request, label, updatedAt: origin.updatedAt });
  }
  return items.sort((a, b) => b.updatedAt - a.updatedAt || a.id.localeCompare(b.id));
}
