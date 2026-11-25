import type { Health } from "./components/CollaborationSupportPanel";
import type { MultiAgentSnapshot } from "./types";

export const HANDOFF_LABELS: Record<string, string> = { preview: "待确认", stopping: "停止中 / 待核验", source_stopped: "原会话已停止，待入队", queued: "目标排队中", transferred: "负责人已变更，项目暂停", cancelled: "已取消" };
export type CheckState = "正常" | "忙碌" | "等待授权" | "已停止" | "异常" | "未验证";
export function sessionCheckState(session: Health["sessions"][number]): CheckState {
  if (session.archivedAt || session.status === "stopped") return "已停止";
  if (session.status === "error") return "异常";
  if (session.commandHost) return "未验证";
  if (!session.ptyId || !["claude_code", "codex", "kimi_code", "pi", "opencode"].includes(session.runtime)) return "未验证";
  if (session.processRunning && session.status === "waiting" && /permission|approval|授权|审批|批准/i.test(session.waitingReason ?? "")) return "等待授权";
  if (session.processRunning && session.ready) return "正常";
  if (session.processRunning && session.status === "running") return "忙碌";
  if (!session.processRunning && session.status !== "pending") return "已停止";
  return "未验证";
}

export function scopeHealth(health: Health, snapshot: MultiAgentSnapshot | null, global: boolean, enabled: readonly string[]): Health {
  const ids = new Set(snapshot?.agentSessions.flatMap((session) => session.appSessionId ? [session.appSessionId] : []) ?? []);
  const sessions = health.sessions.filter((session) => enabled.includes(session.runtime) && (global || Boolean(snapshot && (session.projectId === snapshot.project.id || ids.has(session.id)))));
  const sessionIds = new Set(sessions.map((session) => session.id));
  const kinds = global ? enabled : enabled.filter((kind) => snapshot?.agents.some((agent) => agent.enabled && agent.runtimeKind === kind));
  return { ...health, scopeLabel: global ? "全局" : "当前项目", runtimes: health.runtimes.filter((runtime) => kinds.includes(runtime.runtime)).map((runtime) => ({ ...runtime,
    skills: runtime.skills.filter((skill) => global || (skill.globalRoot !== false && !skill.projectIds?.length) || Boolean(snapshot && (skill.projectIds?.includes(snapshot.project.id) || skill.sessionIds?.some((id) => ids.has(id))))) })), sessions,
    messages: health.messages.filter((message) => sessionIds.has(message.toSessionId) || Boolean(message.fromSessionId && sessionIds.has(message.fromSessionId))),
    handoffs: health.handoffs.filter((item) => global || Boolean(snapshot && (item.projectId === snapshot.project.id || sessionIds.has(item.sourceId) || sessionIds.has(item.targetId)))) };
}

export function diagnosticChecks(health: Health) {
  return [
    { label: "协作 CLI", state: (health.cliPathExists ? "正常" : "异常") as CheckState },
    ...health.runtimes.map((runtime) => ({ label: runtime.label, state: (!runtime.supported ? "未验证" : !runtime.installed || !runtime.skills.length || runtime.skills.some((skill) => !skill.matches) ? "异常" : "正常") as CheckState })),
    ...health.sessions.map((session) => ({ label: session.title, state: sessionCheckState(session) })),
  ];
}

export function summarizeDiagnosticChecks(checks: ReadonlyArray<{ state: CheckState }>, t?: (key: string, values?: Record<string, string | number>) => string): string {
  return (["异常", "等待授权", "已停止", "未验证", "忙碌", "正常"] as const).flatMap((state) => {
    const count = checks.filter((check) => check.state === state).length;
    return count ? [t ? t("{count} 项{state}", { count, state: t(state) }) : `${count} 项${state}`] : [];
  }).join(" · ") || (t ? t("0 项检查") : "0 项检查");
}

const ACTIVITY_LABELS: Record<string, string> = { "conversation.cleared": "对话已清空", "run.status_changed": "执行状态更新", "task.execution_completed": "任务执行结束", "task.created": "任务已创建", "task.status_changed": "任务状态更新", "artifact.created": "已提交交付物", "review.created": "审核已提交", "project.status_changed": "项目状态更新" };
export function activityLabel(type: string): string { return ACTIVITY_LABELS[type] ?? (/[\u3400-\u9fff]/.test(type) ? type : "协作记录更新"); }
export function taskPriority(status: string): number { return ["failed", "blocked", "changes_requested", "waiting_approval", "in_review", "running", "ready", "pending", "cancelled", "done"].indexOf(status) === -1 ? 8 : ["failed", "blocked", "changes_requested", "waiting_approval", "in_review", "running", "ready", "pending", "cancelled", "done"].indexOf(status); }
export function taskMatchesFilter(status: string, filter: string): boolean {
  if (filter === "all") return true;
  if (filter === "attention") return ["failed", "blocked", "changes_requested", "waiting_approval", "in_review"].includes(status);
  if (filter === "in_review") return ["waiting_approval", "in_review"].includes(status);
  return status === filter;
}
export function handoffRecoveryHistory(health: Health | null, projectId?: string) {
  return health?.handoffs.filter((item) => item.mode === "session" || Boolean(projectId && item.projectId === projectId)) ?? [];
}
