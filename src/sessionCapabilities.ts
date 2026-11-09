import type { SessionRecord } from "./types";

export function isStandaloneHarnessSession(session: SessionRecord): boolean {
  return session.kind === "terminal"
    && session.agentRuntimeKind === "deepseek_harness"
    && session.providerId === "local-runtime-deepseek_harness"
    && !session.multiAgentProjectId
    && !session.multiAgentAgentId;
}

export function isStandaloneZCodeSession(session: SessionRecord): boolean {
  return session.kind === "terminal" && session.agentRuntimeKind === "zcode"
    && session.providerId === "local-runtime-zcode" && !session.multiAgentProjectId && !session.multiAgentAgentId;
}
export function isStandaloneClaudeChatSession(session: SessionRecord): boolean {
  return session.kind === "terminal" && session.agentRuntimeKind === "claude_chat"
    && session.providerId === "local-runtime-claude_chat" && !session.multiAgentProjectId && !session.multiAgentAgentId;
}
export function isStandaloneCodexChatSession(session: SessionRecord): boolean {
  return session.kind === "terminal" && session.agentRuntimeKind === "codex_chat"
    && session.providerId === "local-runtime-codex_chat" && !session.multiAgentProjectId && !session.multiAgentAgentId;
}
export function isStandaloneKimiChatSession(session: SessionRecord): boolean {
  return session.kind === "terminal" && session.agentRuntimeKind === "kimi_chat"
    && session.providerId === "local-runtime-kimi_chat" && !session.multiAgentProjectId && !session.multiAgentAgentId;
}
export function isStandaloneOpenCodeChatSession(session: SessionRecord): boolean {
  return session.kind === "terminal" && session.agentRuntimeKind === "opencode_chat"
    && session.providerId === "local-runtime-opencode_chat" && !session.multiAgentProjectId && !session.multiAgentAgentId;
}
export function isStandalonePiChatSession(session: SessionRecord): boolean {
  return session.kind === "terminal" && session.agentRuntimeKind === "pi_chat"
    && session.providerId === "local-runtime-pi_chat" && !session.multiAgentProjectId && !session.multiAgentAgentId;
}
export function isNativeChatSession(session: SessionRecord): boolean {
  return isStandaloneHarnessSession(session) || isStandaloneClaudeChatSession(session) || isStandaloneCodexChatSession(session) || isStandaloneKimiChatSession(session) || isStandaloneOpenCodeChatSession(session) || isStandalonePiChatSession(session) || isStandaloneZCodeSession(session);
}
export function isSessionActive(session: SessionRecord): boolean {
  return session.ptyId !== null
    || (isNativeChatSession(session)
      && (session.status === "running" || session.status === "waiting"));
}

export function isPtySession(session: SessionRecord): boolean {
  return !isNativeChatSession(session) && session.ptyId !== null;
}

/** 兼容旧会话的默认名称，仅调整显示，不改写历史或自定义标题。 */
export function sessionDisplayTitle(session: SessionRecord): string {
  if (session.agentRuntimeKind === "zcode") {
    if (session.title === "ZCode") return "ZCode Desktop";
    if (session.title.startsWith("ZCode · ")) return `ZCode Desktop${session.title.slice(5)}`;
  }
  return session.title;
}
export function sessionDisplayProvider(session: SessionRecord): string {
  return session.agentRuntimeKind === "zcode" && session.providerId === "local-runtime-zcode"
    ? "ZCode Desktop" : session.providerName;
}
