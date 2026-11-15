import type { Automation, AutomationRun } from "./types";

export const RUN_LABELS: Record<string, string> = {
  dispatching: "正在派发", queued: "已入队", submitted: "已提交·待回执", received: "已接收·待结果",
  running: "执行中", waiting: "等待处理", stopping: "正在停止", completed: "已完成", failed: "失败",
  cancelled: "已取消", unconfirmed: "结果未确认", acknowledged: "已核对·结果未知", skipped: "已跳过", idle: "尚未执行",
};
export type RunFilter = "all" | "active" | "completed" | "failed" | "attention";
export function runStage(run?: AutomationRun): string {
  // Old `ok` means dispatch succeeded. It must never manufacture completion.
  return run ? run.stage ?? (run.ok ? "unconfirmed" : "failed") : "idle";
}
export function activeRun(run?: AutomationRun): boolean {
  return ["dispatching", "queued", "submitted", "received", "running", "waiting", "stopping", "unconfirmed"].includes(runStage(run));
}
export function currentRun(item: Automation): AutomationRun | undefined {
  const runs = [...item.recentRuns].reverse().filter(r => runStage(r) !== "skipped");
  return runs.find(activeRun) ?? runs[0];
}
export function matchesRunFilter(item: Automation, filter: RunFilter): boolean {
  const stage = runStage(currentRun(item));
  return filter === "all" || (filter === "active" ? ["dispatching", "queued", "submitted", "received", "running", "stopping"].includes(stage)
    : filter === "attention" ? ["waiting", "unconfirmed"].includes(stage) : stage === filter);
}
export function planState(item: Automation): string {
  if (!item.enabled) return item.consecutiveFailures >= item.maxConsecutiveFailures ? "计划已熔断" : "计划已暂停";
  if (item.maxRuns !== null && item.runCount >= item.maxRuns) return "触发次数已用完";
  if (item.trigger.kind === "once" && item.runCount > 0) return "一次性触发已结束";
  return "计划已启用";
}
