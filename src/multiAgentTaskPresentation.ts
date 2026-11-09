import type {
  MultiAgentSnapshot,
  MultiAgentTask,
  MultiAgentTaskStatus,
} from "./types";

export type TaskStatusTone =
  | "neutral"
  | "active"
  | "waiting"
  | "approval"
  | "failed"
  | "completed"
  | "cancelled";

export interface TaskStatusPresentation {
  taskId: string;
  status: MultiAgentTaskStatus;
  tone: TaskStatusTone;
  label: string;
  title: string;
  agentName: string;
  detail: string;
}

const STATUS_PRESENTATION: Record<
  MultiAgentTaskStatus,
  { label: string; tone: TaskStatusTone; priority: number }
> = {
  in_review: { label: "等待审批", tone: "approval", priority: 0 },
  waiting_approval: { label: "等待确认", tone: "waiting", priority: 1 },
  blocked: { label: "等待依赖", tone: "waiting", priority: 2 },
  failed: { label: "执行失败", tone: "failed", priority: 3 },
  running: { label: "执行中", tone: "active", priority: 4 },
  claimed: { label: "正在启动", tone: "active", priority: 5 },
  changes_requested: { label: "等待修改", tone: "waiting", priority: 6 },
  ready: { label: "等待调度", tone: "neutral", priority: 7 },
  done: { label: "已完成", tone: "completed", priority: 8 },
  cancelled: { label: "已取消", tone: "cancelled", priority: 9 },
  draft: { label: "草稿", tone: "neutral", priority: 10 },
};

function latestTaskRun(snapshot: MultiAgentSnapshot, taskId: string) {
  return snapshot.runs
    .filter((run) => run.taskId === taskId)
    .sort((left, right) => right.createdAt - left.createdAt)[0] ?? null;
}

function unresolvedDependencyNames(
  snapshot: MultiAgentSnapshot,
  task: MultiAgentTask,
): string[] {
  const tasksById = new Map(snapshot.tasks.map((item) => [item.id, item]));
  return snapshot.dependencies
    .filter((dependency) => dependency.taskId === task.id)
    .map((dependency) => tasksById.get(dependency.dependsOnTaskId))
    .filter(
      (dependency): dependency is MultiAgentTask =>
        Boolean(dependency && dependency.status !== "done"),
    )
    .map((dependency) => dependency.title);
}

function dependencyDetail(names: string[]): string {
  if (names.length === 0) return "等待前置任务完成后自动开始";
  const visible = names.slice(0, 2).join("、");
  return names.length > 2
    ? `等待：${visible} 等 ${names.length} 项任务`
    : `等待：${visible}`;
}

function taskDetail(
  snapshot: MultiAgentSnapshot,
  task: MultiAgentTask,
): string {
  const latestRun = latestTaskRun(snapshot, task.id);
  switch (task.status) {
    case "blocked":
      return dependencyDetail(unresolvedDependencyNames(snapshot, task));
    case "waiting_approval":
      return "Agent 会话正在等待权限确认或用户输入";
    case "in_review":
      return "交付已生成，等待你审批后继续后续任务";
    case "failed":
      return latestRun?.error ?? latestRun?.outputSummary ?? "执行遇到错误，可在详情中查看日志并重试";
    case "running":
      return latestRun?.outputSummary ?? "Agent 正在处理任务";
    case "claimed":
      return "任务已派发，Agent 终端正在启动";
    case "changes_requested":
      return "审查已提出修改，等待 Agent 继续处理";
    case "ready":
      return "前置依赖已满足，等待调度执行";
    case "done":
      return latestRun?.outputSummary ?? "任务已完成，可在协作工作台查看交付结果";
    case "cancelled":
      return "任务已取消，不再继续执行";
    default:
      return "任务尚未进入执行队列";
  }
}

export function buildTaskStatusPresentations(
  snapshot: MultiAgentSnapshot | null,
  limit = 5,
): TaskStatusPresentation[] {
  if (!snapshot || limit <= 0) return [];
  const agentsById = new Map(snapshot.agents.map((agent) => [agent.id, agent]));
  return [...snapshot.tasks]
    .sort((left, right) => {
      const priority =
        STATUS_PRESENTATION[left.status].priority -
        STATUS_PRESENTATION[right.status].priority;
      return priority || right.updatedAt - left.updatedAt;
    })
    .slice(0, limit)
    .map((task) => {
      const presentation = STATUS_PRESENTATION[task.status];
      return {
        taskId: task.id,
        status: task.status,
        tone: presentation.tone,
        label: presentation.label,
        title: task.title,
        agentName:
          (task.assigneeAgentId
            ? agentsById.get(task.assigneeAgentId)?.name
            : null) ?? "未分配 Agent",
        detail: taskDetail(snapshot, task),
      };
    });
}

export function countActiveTasks(snapshot: MultiAgentSnapshot | null): number {
  if (!snapshot) return 0;
  return snapshot.tasks.filter((task) =>
    ["claimed", "running", "waiting_approval", "in_review"].includes(
      task.status,
    ),
  ).length;
}

const ATTENTION_TASK_STATUSES = new Set<MultiAgentTaskStatus>([
  "failed",
  "waiting_approval",
  "in_review",
]);

export function shouldExpandTaskProgress(
  snapshot: MultiAgentSnapshot | null,
): boolean {
  return Boolean(
    snapshot?.tasks.some((task) => ATTENTION_TASK_STATUSES.has(task.status)),
  );
}

export function buildTaskProgressSummary(
  snapshot: MultiAgentSnapshot | null,
): string {
  if (!snapshot || snapshot.tasks.length === 0) return "";
  const total = snapshot.tasks.length;
  const done = snapshot.tasks.filter((task) => task.status === "done").length;
  const attention = snapshot.tasks.filter((task) =>
    ATTENTION_TASK_STATUSES.has(task.status),
  ).length;
  if (attention > 0) return `${done}/${total} · ${attention} 项需处理`;
  const active = countActiveTasks(snapshot);
  if (active > 0) return `${done}/${total} · ${active} 项进行中`;
  if (done === total) return `${done}/${total} · 已完成`;
  return `${done}/${total} · 等待调度`;
}

export interface TaskStageGroup {
  stage: number;
  tasks: MultiAgentTask[];
  done: number;
  total: number;
  needsAttention: number;
  /** 组内存在人工卡点任务（停在等待审批的交付）。 */
  awaitingGate: boolean;
  allDone: boolean;
}

/** 工作流模板视角的任务分组：按阶段升序，组内按状态优先级排序。 */
export function groupTasksByStage(
  snapshot: MultiAgentSnapshot | null,
): TaskStageGroup[] {
  if (!snapshot) return [];
  const byStage = new Map<number, MultiAgentTask[]>();
  for (const task of snapshot.tasks) {
    const group = byStage.get(task.stage) ?? [];
    group.push(task);
    byStage.set(task.stage, group);
  }
  return [...byStage.entries()]
    .sort((left, right) => left[0] - right[0])
    .map(([stage, tasks]) => {
      const sorted = [...tasks].sort(
        (left, right) =>
          STATUS_PRESENTATION[left.status].priority -
          STATUS_PRESENTATION[right.status].priority,
      );
      const done = sorted.filter((task) => task.status === "done").length;
      return {
        stage,
        tasks: sorted,
        done,
        total: sorted.length,
        needsAttention: sorted.filter((task) =>
          ATTENTION_TASK_STATUSES.has(task.status),
        ).length,
        awaitingGate: sorted.some((task) => task.status === "in_review"),
        allDone: done === sorted.length,
      };
    });
}
