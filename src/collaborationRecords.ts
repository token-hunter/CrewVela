import type { DeliveryRecord, MultiAgentSnapshot } from "./types";

export interface CollaborationRecord {
  id: string;
  at: number;
  kind: string;
  title: string;
  body: string;
  details?: string;
  agentId: string | null;
}

const STATUS_LABELS: Record<string, string> = {
  idle: "待命",
  starting: "启动中",
  running: "执行中",
  waiting: "等待输入",
  completed: "已完成",
  failed: "失败",
  stopped: "已停止",
  draft: "草稿",
  ready: "可执行",
  blocked: "等待依赖",
  claimed: "已领取",
  waiting_approval: "待批准",
  in_review: "审查中",
  changes_requested: "需修改",
  done: "已完成",
  cancelled: "已取消",
};

const EVENT_LABELS: Record<string, string> = {
  "run.claimed": "任务已派发",
  "run.status_changed": "执行状态更新",
  "task.execution_completed": "Agent 已交付",
  "task.delivery_updated": "交付已更新",
  "task.approved": "审查已通过",
  "task.changes_requested": "审查要求修改",
  "review.submitted": "Reviewer 已提交结论",
  "review.approved": "自动审核已通过",
  "review.changes_requested": "自动审核退回修改",
  "review.loop_exhausted": "审核修复次数已用尽",
  "task.retry_requested": "任务重新执行",
  "task.reassigned": "任务重新派发",
  "task.dependency_added": "新增协作依赖",
  "agent.control_changed": "控制权已切换",
  "project.status_changed": "项目状态更新",
  "task.capability_routed": "按能力重新派发",
  "run.retry_scheduled": "执行失败，已安排重试",
  "run.failed": "执行已失败",
  "message.sent": "Agent 发送消息",
  "artifact.published": "Agent 发布产物",
  "approval.requested": "Agent 请求审批",
  "memory.recorded": "Agent 记录经验",
};

/** Same project-bound sources previously shown in the workspace record rail. */
export function buildCollaborationRecords(snapshot: MultiAgentSnapshot, deliveryRecords: DeliveryRecord[]): CollaborationRecord[] {
  const taskById = new Map(snapshot.tasks.map(task => [task.id, task]));
  const runById = new Map(snapshot.runs.map(run => [run.id, run]));
  const appSessionToAgent = new Map(
    snapshot.agentSessions
      .filter((session) => session.appSessionId)
      .map((session) => [session.appSessionId as string, session.agentId]),
  );
  const agentNameBySession = (sessionId: string | null): string | null => {
    if (!sessionId) return null;
    const agentId = appSessionToAgent.get(sessionId);
    return agentId
      ? snapshot.agents.find((agent) => agent.id === agentId)?.name ?? null
      : null;
  };

  return [
    ...snapshot.messages.map((message) => ({
      id: `message-${message.id}`,
      at: message.createdAt,
      kind: "消息",
      title: `${message.senderId ? snapshot.agents.find((agent) => agent.id === message.senderId)?.name ?? "Agent" : "总控"} → ${message.targetAgentId ? snapshot.agents.find((agent) => agent.id === message.targetAgentId)?.name ?? "Agent" : "项目"}`,
      body: message.body,
      agentId: message.targetAgentId ?? message.senderId,
    })),
    // 即时通道投影：只显示收发双方至少一方属于本项目的记录
    ...deliveryRecords
      .filter(
        (record) =>
          (record.fromSessionId && appSessionToAgent.has(record.fromSessionId)) ||
          appSessionToAgent.has(record.toSessionId),
      )
      .map((record) => {
        const fromName =
          agentNameBySession(record.fromSessionId) ?? record.fromLabel ?? "总控";
        const toName =
          agentNameBySession(record.toSessionId) ?? record.toSessionId.slice(0, 8);
        return {
          id: `delivery-${record.id}`,
          at: record.at,
          kind:
            record.kind === "system"
              ? "系统"
              : record.kind === "broadcast"
                ? "直通·广播"
                : "直通",
          title: `${fromName} → ${toName}`,
          body: record.body,
          agentId:
            appSessionToAgent.get(record.toSessionId) ??
            (record.fromSessionId
              ? appSessionToAgent.get(record.fromSessionId) ?? null
              : null),
        };
      }),
    ...snapshot.artifacts.map((artifact) => ({
      id: `artifact-${artifact.id}`,
      at: artifact.createdAt,
      kind: "交付",
      title: taskById.get(artifact.taskId)?.title ?? "任务交付物",
      body: `${artifact.summary}${artifact.contentHash ? `\n哈希 ${artifact.contentHash}` : ""}`,
      agentId: taskById.get(artifact.taskId)?.assigneeAgentId ?? null,
    })),
    ...snapshot.events.map((event) => {
      const payloadTaskId = typeof event.payload.taskId === "string" ? event.payload.taskId : null;
      const run = event.entityType === "run" ? runById.get(event.entityId) : null;
      const taskId = event.entityType === "task" ? event.entityId : payloadTaskId ?? run?.taskId ?? null;
      const task = taskId ? taskById.get(taskId) : null;
      const actorIsAgent = event.actorId
        ? snapshot.agents.some((agent) => agent.id === event.actorId)
        : false;
      const agentId = actorIsAgent ? event.actorId : run?.agentId ?? task?.assigneeAgentId ?? null;
      const baseTitle = EVENT_LABELS[event.eventType] ?? "协作事件";
      const eventAgent = agentId ? snapshot.agents.find((agent) => agent.id === agentId) : null;
      const title = event.eventType === "task.execution_completed" && eventAgent?.role === "integrator"
        ? "集成任务已完成"
        : event.eventType === "task.execution_completed" && eventAgent?.role === "reviewer"
          ? "审查任务已完成"
          : baseTitle;
      const state = typeof event.payload.to === "string" ? ` · ${STATUS_LABELS[event.payload.to] ?? event.payload.to}` : "";
      return {
        id: `event-${event.seq}`,
        at: event.createdAt,
        kind: "事件",
        title: `${title}${state}`,
        body: task?.title ?? "项目级事件",
        details: Object.keys(event.payload).length ? JSON.stringify(event.payload, null, 2) : undefined,
        agentId,
      };
    }),
    ...snapshot.runtimeEvents
      .filter((event) => !["output", "heartbeat"].includes(event.eventType))
      .map((event) => ({
        id: `runtime-event-${event.seq}`,
        at: event.createdAt,
        kind: "运行",
        title: `Agent ${event.eventType}`,
        body: taskById.get(event.taskId)?.title ?? "运行事件",
        details: Object.keys(event.payload ?? {}).length ? JSON.stringify(event.payload, null, 2) : undefined,
        agentId: event.agentId,
      })),
    ...snapshot.memories.map((memory) => ({
      id: `memory-${memory.id}`,
      at: memory.updatedAt,
      kind: "经验",
      title: `[${memory.namespace}] ${memory.key}`,
      body: memory.value,
      agentId: memory.sourceTaskId
        ? taskById.get(memory.sourceTaskId)?.assigneeAgentId ?? null
        : null,
    })),
  ].sort((left, right) => right.at - left.at);

}
