import type { ControlWorkspaceTrustStatus } from "./types";

/** 有会话正在启动或等待工作区信任确认时保持快速响应。 */
export const TRUST_POLL_ACTIVE_MS = 250;
/** 其余时间放缓，避免每秒 4 次无效 IPC 与整个总控侧栏重渲染。 */
export const TRUST_POLL_IDLE_MS = 1000;

export function nextTrustPollDelay(status: ControlWorkspaceTrustStatus | null): number {
  return status && (status.startingCount > 0 || status.awaitingCount > 0)
    ? TRUST_POLL_ACTIVE_MS
    : TRUST_POLL_IDLE_MS;
}

export function sameTrustStatus(a: ControlWorkspaceTrustStatus, b: ControlWorkspaceTrustStatus): boolean {
  return a.batchCount === b.batchCount
    && a.awaitingCount === b.awaitingCount
    && a.startingCount === b.startingCount
    && a.missingCount === b.missingCount;
}
