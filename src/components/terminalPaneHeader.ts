import type { AgentRuntimeKind, SessionKind } from "../types";

interface TerminalPaneHeaderInput {
  title: string;
  providerName: string;
  kind: SessionKind;
  agentRuntimeKind?: AgentRuntimeKind | null;
  multiAgentProjectId?: string | null;
  multiAgentAgentId?: string | null;
  relaySession?: boolean;
  cwd: string;
}

export interface TerminalPaneHeaderPresentation {
  title: string;
  runtimeKind: AgentRuntimeKind | null;
  showShellIcon: boolean;
  subtitle: string | null;
  agentBadge: "relay" | "collaboration" | null;
}

const normalizedLabel = (value: string) => value.trim().toLocaleLowerCase();

export function terminalPaneHeaderPresentation({
  title,
  providerName,
  kind,
  agentRuntimeKind,
  multiAgentProjectId,
  multiAgentAgentId,
  relaySession = false,
  cwd,
}: TerminalPaneHeaderInput): TerminalPaneHeaderPresentation {
  const runtimeKind = agentRuntimeKind ?? (kind === "claude" ? "claude_code" : null);
  const displayTitle = kind === "claude" && !agentRuntimeKind && title === "新会话"
    ? "Claude Code"
    : title;
  const showShellIcon = kind === "terminal" && !runtimeKind;
  const candidateSubtitle = showShellIcon ? cwd : providerName;
  const subtitle = normalizedLabel(candidateSubtitle) === normalizedLabel(displayTitle)
    ? null
    : candidateSubtitle;
  const agentBadge = relaySession
    ? "relay"
    : multiAgentProjectId && multiAgentAgentId
      ? "collaboration"
      : null;

  return { title: displayTitle, runtimeKind, showShellIcon, subtitle, agentBadge };
}
