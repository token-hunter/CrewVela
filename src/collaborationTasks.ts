import type { AgentRun, MultiAgentSnapshot, MultiAgentTask } from "./types";

export type CollaborationSettingsTarget = "tasks" | "team";

/** A request ID allows repeated navigation to the same project. */
export interface CollaborationSettingsRequest { projectId: string; requestId: number; target: CollaborationSettingsTarget; }

/** Matches multi_agent/conversation.rs: discussion turns are internal executor tasks. */
export function visibleProjectTasks<T extends Pick<MultiAgentTask, "id">>(tasks: readonly T[]): T[] {
  return tasks.filter(task => !task.id.startsWith("cli-message-conversation-"));
}

export const TASK_RUN_STATUS: Record<AgentRun["status"], string> = {
  queued: "已排队", starting: "启动中", running: "执行中", waiting_input: "等待输入",
  waiting_approval: "等待授权", succeeded: "已完成", failed: "失败", cancelled: "已取消",
  timed_out: "已超时", interrupted: "已中断",
};

export function orderedTaskRuns(snapshot: MultiAgentSnapshot | null, taskId?: string): AgentRun[] {
  if (!snapshot || !taskId) return [];
  return snapshot.runs.filter(run => run.projectId === snapshot.project.id && run.taskId === taskId)
    .sort((a, b) => a.attempt - b.attempt || a.createdAt - b.createdAt);
}
