import { runtimeCategory } from "./runtimeTaxonomy";
import type { AgentRuntimeKind } from "./types";

// 新建会话的可选 Agent 与具体选择界面分开，空工作区和启动弹窗共用。
export const NEW_SESSION_AGENTS = [
  "claude_code", "codex", "kimi_code", "pi", "opencode", "deepseek_harness",
  "claude_chat", "codex_chat", "kimi_chat", "opencode_chat", "pi_chat", "zcode", "zcode_cli", "antigravity",
] as const satisfies readonly AgentRuntimeKind[];
export type NewSessionAgent = (typeof NEW_SESSION_AGENTS)[number];
export const TERMINAL_AGENTS = NEW_SESSION_AGENTS.filter(runtime => runtimeCategory(runtime) === "terminal");
export const CHAT_AGENTS = NEW_SESSION_AGENTS.filter(runtime => runtimeCategory(runtime) === "chat");
