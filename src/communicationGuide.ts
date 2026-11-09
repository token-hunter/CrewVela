import type { AgentProfile, SessionRecord } from "./types";

export type CommunicationScope = "sessions" | "project";
export interface CommunicationGuideRequest { scope?: CommunicationScope; projectId?: string | null }
export interface CommunicationParticipant { id: string; name: string; runtime: string; family: string }
export const COMMUNICATION_GUIDE_EVENT = "crewvela-open-communication-guide";
export const COMMUNICATION_SEEN_EVENT = "crewvela-communication-guide-seen";
export const COMMUNICATION_SEEN_KEY = "crewvela-communication-discovery-v1";

const families: Record<string, string> = { claude_chat: "claude_code", codex_chat: "codex", kimi_chat: "kimi_code", pi_chat: "pi", opencode_chat: "opencode" };
const labels: Record<string, string> = { claude_code: "Claude Code", codex: "Codex", kimi_code: "Kimi Code", pi: "Pi", opencode: "OpenCode", deepseek_harness: "DeepSeek Harness", zcode: "ZCode Desktop", zcode_cli: "ZCode CLI" };
export function communicationSessions(sessions: readonly SessionRecord[]): CommunicationParticipant[] {
  return sessions.flatMap(session => {
    if (session.runtimeCommandHost || session.retiredAgentRuntime || session.archivedAt != null || session.multiAgentProjectId || session.multiAgentAgentId || !["running", "waiting"].includes(session.status)) return [];
    const kind = session.agentRuntimeKind ?? (session.kind === "claude" ? "claude_code" : "");
    const family = families[kind] ?? kind;
    if (!["claude_code", "codex", "kimi_code", "pi", "opencode"].includes(family)) return [];
    const native = Boolean(families[kind]) && session.providerId === `local-runtime-${kind}`;
    if (session.ptyId == null && !native) return [];
    return [{ id: session.id, name: session.title.trim() || labels[family], runtime: labels[family], family }];
  });
}
export function communicationMembers(agents: readonly AgentProfile[], projectId: string): CommunicationParticipant[] {
  return agents.filter(agent => agent.projectId === projectId && agent.enabled && Boolean(labels[agent.runtimeKind]))
    .map(agent => ({ id: agent.id, name: agent.name, runtime: labels[agent.runtimeKind], family: agent.runtimeKind }));
}
export function communicationExample(scope: CommunicationScope, participants: readonly CommunicationParticipant[], sourceId: string, targetId: string): string | null {
  const source = participants.find(item => item.id === sourceId), target = participants.find(item => item.id === targetId);
  if (!source || !target || source.id === target.id) return null;
  const duplicate = participants.filter(item => item.name === target.name).length > 1;
  const name = `「${target.name}」${duplicate ? `（${scope === "project" ? "成员" : "会话"} ID：${target.id}）` : ""}`;
  return scope === "project" ? `请把这次修改的要点发给本项目的${name}，发送前核对项目成员列表。` : `请先核对当前已打开的会话，给${name}发一句问候，只联系这个会话。`;
}
export function requestCommunicationGuide(request: CommunicationGuideRequest = {}): void {
  window.dispatchEvent(new CustomEvent(COMMUNICATION_GUIDE_EVENT, { detail: request }));
}
export function resolveCommunicationGuideRequest(request: CommunicationGuideRequest, workspaceProjectId: string | null): { scope: CommunicationScope; projectId: string | null } {
  const projectId = "projectId" in request ? request.projectId ?? null : workspaceProjectId;
  const scope = request.scope === "sessions" ? "sessions" : request.scope === "project" || projectId ? "project" : "sessions";
  return { scope, projectId: scope === "project" ? projectId : null };
}
export function communicationGuideSeen(): boolean {
  try { return window.localStorage.getItem(COMMUNICATION_SEEN_KEY) === "1"; } catch { return false; }
}
export function markCommunicationGuideSeen(): void {
  // Same-window listeners must hide the hint even when preference storage fails.
  window.dispatchEvent(new Event(COMMUNICATION_SEEN_EVENT));
  window.localStorage.setItem(COMMUNICATION_SEEN_KEY, "1");
}
