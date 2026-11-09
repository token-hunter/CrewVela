import type { CoordinatorHistoryMessage, MultiAgentSnapshot } from "./types";

export function canAutoContinueProject(snapshot: MultiAgentSnapshot, projectId: string): boolean {
  return snapshot.project.id === projectId && snapshot.project.status !== "paused"
    && snapshot.project.status !== "cancelled";
}

// Retain local failures after polling, without repeating a user message already persisted.
export function mergeFailedCollaborationMessages(
  persisted: CoordinatorHistoryMessage[],
  failed: CoordinatorHistoryMessage[],
): CoordinatorHistoryMessage[] {
  const counts = new Map<string, number>();
  const key = (message: CoordinatorHistoryMessage) => `${message.role}\u0000${message.content}`;
  for (const message of persisted) counts.set(key(message), (counts.get(key(message)) ?? 0) + 1);
  const retained = failed.filter((message) => {
    const remaining = counts.get(key(message)) ?? 0;
    if (!remaining) return true;
    counts.set(key(message), remaining - 1);
    return false;
  });
  return [...persisted, ...retained];
}

export function autoApprovalTaskIds(snapshot: MultiAgentSnapshot): string[] {
  return snapshot.tasks.filter((task) => {
    if (task.status !== "in_review" || task.reviewerAgentId) return false;
    if (snapshot.approvals.some((approval) => approval.taskId === task.id && approval.status === "pending"
      && approval.approvalType !== "task_review")) return false;
    const latest = snapshot.runs.filter((run) => run.taskId === task.id)
      .sort((left, right) => right.attempt - left.attempt)[0];
    return latest?.status === "succeeded";
  }).map((task) => task.id);
}
