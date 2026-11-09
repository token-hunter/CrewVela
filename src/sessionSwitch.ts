import type { AgentRuntimeKind, SessionRecord } from "./types";
import { isRetiredRuntime } from "./runtimeTaxonomy.ts";

export interface SessionSwitchRecord {
  id: string;
  source: SessionRecord;
  sourcePosition: number;
  sourcePty: number | null;
  targetId: string | null;
  threadId: string | null;
  targetRuntime?: AgentRuntimeKind;
  logicalId?: string | null;
  /** New handoffs keep the stopped source visible; omitted on legacy records. */
  preserveSource?: boolean;
  state: string;
  historyMode: "official" | "handoff";
  context: string;
  error: string | null;
}

export const SWITCH_RUNTIME_LABELS: Partial<Record<AgentRuntimeKind, string>> = { claude_code: "Claude Code", codex: "Codex CLI", kimi_code: "Kimi Code", pi: "Pi", opencode: "OpenCode", zcode_cli: "ZCode CLI", deepseek_harness: "DeepSeek Harness", claude_chat: "Claude Code Chat", codex_chat: "Codex Chat", kimi_chat: "Kimi Chat", opencode_chat: "OpenCode Chat", pi_chat: "Pi Chat", zcode: "ZCode Desktop" };
export const CLI_SWITCH_RUNTIMES: AgentRuntimeKind[] = ["claude_code", "codex", "kimi_code", "pi", "opencode", "zcode_cli"];
export const CHAT_SWITCH_RUNTIMES: AgentRuntimeKind[] = ["deepseek_harness", "claude_chat", "codex_chat", "kimi_chat", "opencode_chat", "pi_chat", "zcode"];
export function switchRuntime(session: SessionRecord): AgentRuntimeKind | null {
  return session.agentRuntimeKind ?? (session.kind === "claude" ? "claude_code" : null);
}
export function switchTargets(session: SessionRecord): AgentRuntimeKind[] {
  const runtime = switchRuntime(session);
  if (isRetiredRuntime(runtime)) return [];
  return (runtime && CLI_SWITCH_RUNTIMES.includes(runtime) ? CLI_SWITCH_RUNTIMES : CHAT_SWITCH_RUNTIMES).filter(kind => kind !== runtime);
}
export function sessionSwitchReason(session: SessionRecord): string | null {
  if (session.archivedAt || session.retiredAgentRuntime || session.multiAgentProjectId || session.multiAgentAgentId || session.runtimeCommandHost)
    return "仅支持普通 Agent 会话；协作任务请使用原有交接功能";
  const runtime = switchRuntime(session);
  return runtime && [...CLI_SWITCH_RUNTIMES, ...CHAT_SWITCH_RUNTIMES].includes(runtime) ? null : "Shell 和协作宿主不能进行 Agent 接力";
}
/** 逻辑位置追溯到最初会话，实际操作始终使用当前物理 ID。 */
export function switchSlotId(sessionId: string, records: SessionSwitchRecord[]): string {
  let id = sessionId;
  const seen = new Set<string>();
  while (!seen.has(id)) {
    seen.add(id);
    const record = [...records].reverse().find(item => item.targetId === id && item.state !== "cancelled");
    if (!record || record.preserveSource) return id;
    if (record.logicalId) return record.logicalId;
    id = record.source.id;
  }
  return sessionId;
}
export function switchedHistory(sessionId: string, records: SessionSwitchRecord[]): SessionRecord | null {
  const separate = [...records].reverse().find(item => item.preserveSource && item.targetId === sessionId && item.state !== "cancelled");
  if (separate) return separate.source;
  const id = switchSlotId(sessionId, records);
  return records.find(item => item.source.id === id && item.state !== "cancelled")?.source ?? records.find(item => item.targetId === sessionId)?.source ?? null;
}
export function projectSwitchSessions(sessions: SessionRecord[], records: SessionSwitchRecord[]): SessionRecord[] {
  const legacy = records.filter(item => !item.preserveSource);
  const active = legacy.filter(item => item.state !== "cancelled" && item.targetId && sessions.some(session => session.id === item.targetId));
  const activeRoots = new Set(active.map(item => switchSlotId(item.targetId!, records)));
  const replaced = new Set(legacy.filter(item => item.state !== "cancelled" && item.targetId && activeRoots.has(switchSlotId(item.source.id, records))).map(item => item.source.id));
  const targets = new Set(active.map(item => item.targetId));
  const projected = sessions.filter(session => !replaced.has(session.id) && !targets.has(session.id));
  const leaves = active.filter(item => !replaced.has(item.targetId!));
  for (const record of leaves.sort((a, b) => a.sourcePosition - b.sourcePosition)) {
    const target = sessions.find(session => session.id === record.targetId)!;
    const root = switchedHistory(target.id, records) ?? record.source;
    projected.splice(Math.min(record.sourcePosition, projected.length), 0, { ...target, createdAt: root.createdAt });
  }
  for (const record of records.filter(item => item.preserveSource && item.state !== "cancelled" && item.targetId)) {
    const target = projected.find(item => item.id === record.targetId);
    if (!target || !projected.some(item => item.id === record.source.id)) continue;
    projected.splice(projected.indexOf(target), 1);
    projected.splice(projected.findIndex(item => item.id === record.source.id) + 1, 0, target);
  }
  return projected;
}

export function retainedSwitchSource(id: string, records: SessionSwitchRecord[]): SessionSwitchRecord | null {
  return [...records].reverse().find(item => item.preserveSource && item.source.id === id && !["preview", "cancelled"].includes(item.state)) ?? null;
}
export function isStructuredZCodeCli(session: SessionRecord, records: SessionSwitchRecord[]): boolean {
  return session.agentRuntimeKind === "zcode_cli" && records.some(item => item.targetId === session.id && item.targetRuntime === "zcode_cli");
}

export function needsSwitchRecovery(item: SessionSwitchRecord, sessions: SessionRecord[], availableSources?: SessionRecord[]): boolean {
  if (availableSources && !availableSources.some((source) => source.id === item.source.id)) return false;
  return item.state !== "cancelled" && (Boolean(item.error) || !["preview", "queued"].includes(item.state)
    || (item.state === "queued" && !sessions.some((session) => session.id === item.targetId)));
}

export function switchSelectionAfterSnapshot(selected: string | null, sessions: SessionRecord[], records: SessionSwitchRecord[], activeSourceId: string | null): string | null {
  if (selected === null || sessions.some((session) => session.id === selected)) return selected;
  const pending = records.find((item) => item.source.id === selected && item.state !== "cancelled");
  if (!pending || (pending.state === "preview" && activeSourceId !== selected)) return null;
  if (pending.targetId && sessions.some((session) => session.id === pending.targetId)) return pending.targetId;
  return selected;
}

export function switchWorkspaceAssignment(id: string, assignments: Record<string, string>, records: SessionSwitchRecord[], fallbackTabId: string): string {
  const seen = new Set<string>();
  while (!seen.has(id)) {
    seen.add(id);
    const assigned = assignments[switchSlotId(id, records)] ?? assignments[id];
    if (assigned) return assigned;
    const incoming = [...records].reverse().find(item => item.targetId === id && item.state !== "cancelled");
    if (!incoming) break;
    id = incoming.source.id;
  }
  return fallbackTabId;
}
