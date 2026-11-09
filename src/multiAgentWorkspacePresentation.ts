import type { MultiAgentSnapshot, SessionRecord } from "./types";

/** 当前任务和真实终端共同决定参与者；历史结束状态不会变成新的待处理项。 */
export function workspaceParticipation(snapshot: MultiAgentSnapshot, sessions: SessionRecord[]) {
  const participating = new Set<string>();
  const executing = new Set<string>();
  const pending = new Set<string>();
  const appSessions = new Map(sessions.map((item) => [item.id, item]));
  for (const linked of snapshot.agentSessions) {
    const actual = linked.appSessionId ? appSessions.get(linked.appSessionId) : null;
    if (actual?.ptyId != null && !["stopped", "error"].includes(actual.status)) participating.add(linked.agentId);
  }
  for (const task of snapshot.tasks) {
    if (["ready", "blocked", "claimed", "running", "waiting_approval", "in_review", "changes_requested"].includes(task.status) && task.assigneeAgentId) {
      participating.add(task.assigneeAgentId);
      if (["claimed", "running"].includes(task.status)) executing.add(task.assigneeAgentId);
      if (["waiting_approval", "in_review", "changes_requested"].includes(task.status)) pending.add(task.assigneeAgentId);
    }
    if (task.status === "in_review" && task.reviewerAgentId) { participating.add(task.reviewerAgentId); pending.add(task.reviewerAgentId); }
  }
  for (const run of snapshot.runs) {
    if (["queued", "starting", "running", "waiting_input", "waiting_approval"].includes(run.status)) participating.add(run.agentId);
    if (["starting", "running"].includes(run.status)) executing.add(run.agentId);
    if (["waiting_input", "waiting_approval"].includes(run.status)) pending.add(run.agentId);
  }
  return { participating, executing, pending };
}
