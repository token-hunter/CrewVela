import type { SessionRecord } from "./types";
import { isNativeChatSession, isSessionActive } from "./sessionCapabilities.ts";
import { isRetiredRuntime } from "./runtimeTaxonomy.ts";

export type AttentionKind = "approval" | "error";
export interface SessionAttention { session: SessionRecord; kind: AttentionKind; key: string }
export const ATTENTION_DISMISSED_KEY = "crewvela:sidebar-attention-dismissed.v1";

export function sessionAttentionKind(session: SessionRecord): AttentionKind | null {
  if (session.archivedAt != null || session.retiredAgentRuntime || isRetiredRuntime(session.agentRuntimeKind)) return null;
  if (session.status === "error") return "error";
  // A generic waiting/idle state is not evidence of an approval request.
  // Native managers enter Waiting only for an interactive permission request;
  // their reason is a tool title (e.g. "Read file"), not an approval keyword.
  if (session.status === "waiting" && isSessionActive(session)
    && (isNativeChatSession(session) || /permission|approval|授权|审批|批准/i.test(session.waitingReason ?? ""))) return "approval";
  return null;
}

export function attentionKey(session: SessionRecord, kind: AttentionKind): string {
  return JSON.stringify([session.id, kind, kind === "approval" ? session.waitingSince ?? session.updatedAt : session.updatedAt]);
}

export function sessionAttentionItems(sessions: readonly SessionRecord[], dismissed: readonly string[] = []): SessionAttention[] {
  const hidden = new Set(dismissed);
  const seen = new Set<string>();
  return sessions.flatMap(session => {
    const kind = sessionAttentionKind(session);
    if (!kind || seen.has(session.id)) return [];
    seen.add(session.id);
    const key = attentionKey(session, kind);
    // Only failed records may be ignored. Pending approvals remain visible.
    return kind === "error" && hidden.has(key) ? [] : [{ session, kind, key }];
  }).sort((a, b) => (a.kind === b.kind ? b.session.updatedAt - a.session.updatedAt : a.kind === "approval" ? -1 : 1));
}

export function attentionSummary(items: readonly SessionAttention[]): string {
  const approvals = items.filter(item => item.kind === "approval").length;
  const errors = items.length - approvals;
  return [approvals ? `待授权 ${approvals}` : "", errors ? `执行失败 ${errors}` : ""].filter(Boolean).join(" · ");
}

export function attentionOpenMode(session: SessionRecord, kind: AttentionKind): "history" | "session" {
  return kind === "error" || !isSessionActive(session) || session.status !== "waiting" ? "history" : "session";
}

export function readDismissedAttention(raw: string | null): string[] {
  const parsed: unknown = JSON.parse(raw ?? "[]");
  if (!Array.isArray(parsed) || parsed.some(value => typeof value !== "string")) throw new Error("待处理偏好格式无效");
  return [...new Set(parsed)].slice(-500);
}
