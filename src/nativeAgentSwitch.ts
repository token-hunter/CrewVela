import type { SessionRecord } from "./types";

export type NativeAgentKind = "deepseek_harness" | "claude_chat" | "codex_chat" | "kimi_chat" | "opencode_chat" | "pi_chat" | "zcode";
export interface NativeAgentSwitchProps {
  managedToolbar?: boolean;
  active?: boolean;
  switching?: boolean;
  enabledAgents?: Record<NativeAgentKind, boolean>;
  onSwitchAgent?: (kind: NativeAgentKind) => void;
  onHandoffAgent?: () => void;
  onSwitchCli?: () => void;
}

export const NATIVE_AGENT_CREATE_COMMANDS: Record<NativeAgentKind, string> = {
  deepseek_harness: "create_deepseek_harness_session", claude_chat: "create_claude_chat_session",
  codex_chat: "create_codex_chat_session", kimi_chat: "create_kimi_chat_session",
  opencode_chat: "create_opencode_chat_session", pi_chat: "create_pi_chat_session", zcode: "create_zcode_session",
};

type NativeAgentCreate = (command: string, args: {
  cwd: string;
  projectId: string | null;
  sourceSessionId?: string;
}) => Promise<SessionRecord>;

function nativeAgentDirectory(source: SessionRecord) {
  const directory = { cwd: source.cwd, projectId: source.projectId ?? null };
  return source.worktree || source.worktreeSourceSessionId
    ? { ...directory, sourceSessionId: source.id }
    : directory;
}

/** 新建入口始终创建独立聊天，包括与来源相同的 Agent。 */
export async function createNativeAgentConversation(source: SessionRecord, kind: NativeAgentKind,
  create: NativeAgentCreate,
  select: (session: SessionRecord) => void): Promise<void> {
  const target = await create(NATIVE_AGENT_CREATE_COMMANDS[kind], nativeAgentDirectory(source));
  select(target);
}

/** 标签只保存逻辑位置；交接后的实际会话由当前投影解析。 */
export function nativeChatTabSessions(sessions: SessionRecord[], slots: string[], slotOf: (id: string) => string): SessionRecord[] {
  return slots.flatMap(slot => {
    const session = sessions.find(item => slotOf(item.id) === slot);
    return session ? [session] : [];
  });
}

/** 打开独立聊天，不收集交接资料，也不停止来源执行。 */
export async function openNativeAgentConversation(sessions: SessionRecord[], source: SessionRecord, kind: NativeAgentKind,
  workspaceId: string, tabOf: (id: string) => string,
  create: NativeAgentCreate,
  select: (session: SessionRecord, created: boolean) => Promise<void>): Promise<void> {
  if (source.agentRuntimeKind === kind) return;
  const existing = nativeAgentCounterpart(sessions, source, kind, workspaceId, tabOf);
  const target = existing ?? await create(NATIVE_AGENT_CREATE_COMMANDS[kind], nativeAgentDirectory(source));
  await select(target, !existing);
}

function normalizedCwd(path: string): string {
  const normalized = path.replace(/\\/g, "/").replace(/\/+$/, "");
  return /^[a-z]:\//i.test(normalized) || normalized.startsWith("//") ? normalized.toLowerCase() : normalized;
}

/** 普通导航辅助：只查找独立会话。执行接力必须使用 session_switch，不能用此函数替代交接。 */
export function nativeAgentCounterpart(sessions: SessionRecord[], source: SessionRecord, kind: NativeAgentKind, workspaceId: string, tabOf: (id: string) => string): SessionRecord | undefined {
  return sessions.filter(item => item.id !== source.id && item.kind === "terminal"
    && item.agentRuntimeKind === kind && item.providerId === `local-runtime-${kind}`
    && !item.multiAgentProjectId && !item.multiAgentAgentId && !item.archivedAt
    && tabOf(item.id) === workspaceId && normalizedCwd(item.cwd) === normalizedCwd(source.cwd))
    .sort((a, b) => b.updatedAt - a.updatedAt)[0];
}
