import type { AgentRuntimeKind } from "./types";
import claudeCodeAgentLogo from "./assets/agent-claude-code.png";
import codexAgentLogo from "./assets/agent-codex.png";
import openCodeAgentLogo from "./assets/agent-opencode.png";
import piAgentLogo from "./assets/agent-pi.png";

const kimiCodeAgentLogo = new URL("./assets/agent-kimi-code.svg", import.meta.url).href;
const retiredRuntimeLogo = new URL("./assets/agent-avatars/compass.svg", import.meta.url).href;

/** Agent 运行时的彩色 logo（协作目标菜单 / 协作工作台 / 终端格标题共用，保证视觉统一）。 */
export const RUNTIME_LOGOS: Record<AgentRuntimeKind, string> = {
  claude_code: claudeCodeAgentLogo,
  codex: codexAgentLogo,
  kimi_code: kimiCodeAgentLogo,
  minimax_code: retiredRuntimeLogo,
  pi: piAgentLogo,
  oh_my_pi: piAgentLogo,
  unsupported: retiredRuntimeLogo,
  opencode: openCodeAgentLogo,
  zcode: new URL("./assets/model-zhipu.svg", import.meta.url).href,
  zcode_cli: new URL("./assets/model-zhipu.svg", import.meta.url).href,
  deepseek_harness: new URL("./assets/model-deepseek.svg", import.meta.url).href,
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
