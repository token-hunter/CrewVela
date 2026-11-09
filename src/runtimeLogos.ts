import type { AgentRuntimeKind } from "./types";
import { NEUTRAL_AGENT_ICONS } from "./neutralAgentAssets";

const claudeCodeAgentLogo = NEUTRAL_AGENT_ICONS.claude;
const codexAgentLogo = NEUTRAL_AGENT_ICONS.codex;
const openCodeAgentLogo = NEUTRAL_AGENT_ICONS.opencode;
const piAgentLogo = NEUTRAL_AGENT_ICONS.pi;
const kimiCodeAgentLogo = NEUTRAL_AGENT_ICONS.kimi;
const retiredRuntimeLogo = new URL("./assets/agent-avatars/compass.svg", import.meta.url).href;

/** Agent 运行时的中性会话图形（协作菜单、工作台和终端标题共用）。 */
export const RUNTIME_LOGOS: Record<AgentRuntimeKind, string> = {
  claude_code: claudeCodeAgentLogo,
  codex: codexAgentLogo,
  kimi_code: kimiCodeAgentLogo,
  minimax_code: retiredRuntimeLogo,
  pi: piAgentLogo,
  oh_my_pi: piAgentLogo,
  unsupported: retiredRuntimeLogo,
  opencode: openCodeAgentLogo,
  zcode: NEUTRAL_AGENT_ICONS.zcode,
  zcode_cli: NEUTRAL_AGENT_ICONS.zcode,
  deepseek_harness: NEUTRAL_AGENT_ICONS.deepseek,
  claude_chat: claudeCodeAgentLogo,
  codex_chat: codexAgentLogo,
  kimi_chat: kimiCodeAgentLogo,
  opencode_chat: openCodeAgentLogo,
  pi_chat: piAgentLogo,
};

export const RUNTIME_LABELS: Record<AgentRuntimeKind, string> = {
  claude_code: "Claude Code",
  codex: "Codex",
  kimi_code: "Kimi Code",
  minimax_code: "已停用的 CLI",
  pi: "Pi",
  oh_my_pi: "Oh My Pi（已移除）",
  unsupported: "已停用的运行时",
  opencode: "OpenCode",
  deepseek_harness: "DeepSeek Harness",
  claude_chat: "Claude Code Chat",
  codex_chat: "Codex Chat",
  kimi_chat: "Kimi Code Chat",
  opencode_chat: "OpenCode Chat",
  pi_chat: "Pi Chat",
  zcode: "ZCode Desktop",
  zcode_cli: "ZCode CLI",
};
