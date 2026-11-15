import type { AgentRuntimeKind, ProviderConfig } from "./types";

export interface ModelSelection { connectionId: string | null; model: string | null }
export interface ResolvedModelSelection { connectionId: string; model: string | null; source: string }
export interface ModelAssignmentRow {
  id: string; scope: "runtime" | "member" | "session"; targetId: string; projectId: string | null; projectName: string | null;
  name: string; runtime: AgentRuntimeKind; raw: ModelSelection; effective: ResolvedModelSelection; version: string;
  editable: boolean; connectionEditable: boolean; note: string;
}
export interface ModelAssignmentSnapshot { rows: ModelAssignmentRow[]; projects: { id: string; name: string }[] }
export function assignmentDraft(row: ModelAssignmentRow): ModelSelection {
  return row.effective.source === "legacy_connection" ? { connectionId: row.effective.connectionId, model: row.effective.model } : { ...row.raw };
}
export const ASSIGNMENT_RUNTIME_LABELS: Record<string, string> = { claude_code: "Claude Code", codex: "Codex", kimi_code: "Kimi Code", pi: "Pi", opencode: "OpenCode", zcode: "ZCode", zcode_cli: "ZCode CLI", deepseek_harness: "DeepSeek Harness", claude_chat: "Claude 原生聊天", codex_chat: "Codex 原生聊天", kimi_chat: "Kimi 原生聊天", pi_chat: "Pi 原生聊天", opencode_chat: "OpenCode 原生聊天" };
export function connectionLabel(id: string, runtime: string, providers: ProviderConfig[]): string {
  if (id === "local") return `${ASSIGNMENT_RUNTIME_LABELS[runtime] ?? runtime} 本机配置`;
  return providers.find(provider => provider.id === id)?.name ?? `连接已缺失：${id}`;
}
export function assignmentSource(row: ModelAssignmentRow): string {
  return ({ runtime_default: "Agent 默认", member: "成员覆盖", session: "会话覆盖", session_connection: "启动时连接", legacy_connection: "历史服务选择", unrecorded: "启动配置未记录" } as Record<string, string>)[row.effective.source] ?? "配置来源未知";
}
export function filterAssignments(rows: ModelAssignmentRow[], project: string, scope: string, query: string): ModelAssignmentRow[] {
  const keyword = query.trim().toLocaleLowerCase();
  return rows.filter(row => (!project || row.scope === "runtime" || row.projectId === project) &&
    (scope === "all" || row.scope === scope) && (!keyword || [row.name, row.projectName, row.runtime, row.effective.model, row.effective.connectionId, assignmentSource(row)].join("\n").toLocaleLowerCase().includes(keyword)));
}
export function modelSuggestions(rows: ModelAssignmentRow[], providers: ProviderConfig[], runtime: string, connectionId: string | null): string[] {
  const provider = providers.find(provider => provider.id === connectionId);
  const ids = rows.filter(row => row.runtime === runtime).flatMap(row => [row.effective.model, row.raw.model]);
  if (provider) ids.push(...Object.entries(provider.env).filter(([key]) => key.includes("MODEL") && !key.endsWith("_NAME")).map(([, value]) => value));
  return [...new Set(ids.filter((value): value is string => !!value))];
}
export function assignmentPreview(): ModelAssignmentSnapshot {
  const rows: ModelAssignmentRow[] = ([ ["claude_code", "Claude Code", "glm", "glm-5.3"], ["codex", "Codex", "local", "gpt-6.1-sol"], ["kimi_code", "Kimi Code", "local", "kimi-for-coding"], ["pi", "Pi", "local", null], ["opencode", "OpenCode", "local", null] ] as const).map(([runtime, name, connectionId, model]) => ({ id: `runtime:${runtime}`, scope: "runtime", targetId: runtime, projectId: null, projectName: null, name, runtime, raw: { connectionId, model }, effective: { connectionId, model, source: "runtime_default" }, version: "preview", editable: true, connectionEditable: runtime === "claude_code", note: "" }));
  for (const [id, name, model, source] of [["lead", "项目总控", "gpt-6.1-sol", "runtime_default"], ["frontend", "前端工程师", "gpt-6.1-terra", "member"]]) rows.push({ id: `member:${id}`, scope: "member", targetId: id, projectId: "preview-project", projectName: "CrewVela", name, runtime: "codex", raw: { connectionId: null, model: source === "member" ? model : null }, effective: { connectionId: "local", model, source }, version: "preview", editable: true, connectionEditable: false, note: "" });
  rows.push({ id: "session:preview-session", scope: "session", targetId: "preview-session", projectId: "preview-project", projectName: "CrewVela", name: "前端页面实现", runtime: "codex", raw: { connectionId: null, model: "gpt-6.1-sol" }, effective: { connectionId: "local", model: "gpt-6.1-sol", source: "session" }, version: "preview", editable: false, connectionEditable: false, note: "仅显示启动配置；运行中请在原会话切换模型" });
  return { rows, projects: [{ id: "preview-project", name: "CrewVela" }] };
}
