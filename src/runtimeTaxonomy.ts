import type { AgentRuntimeKind } from "./types";

/** CLI 终端 Agent：终端格子里的 TUI/CLI 会话，同时也是协作项目的执行 Runtime。 */
export const TERMINAL_RUNTIMES: readonly AgentRuntimeKind[] = [
  "claude_code",
  "codex",
  "kimi_code",
  "pi",
  "opencode",
  "zcode_cli",
  "antigravity",
];

/** 原生聊天 Agent：自绘 UI 的原生聊天会话（原生聊天切换栏成员）。 */
export const CHAT_RUNTIMES: readonly AgentRuntimeKind[] = [
  "claude_chat",
  "codex_chat",
  "kimi_chat",
  "opencode_chat",
  "pi_chat",
  "deepseek_harness",
  "zcode",
];

/** 新协作成员使用的 CLI 执行引擎；会话展示分类独立，dsh 仍保留原生聊天界面。 */
export const COLLABORATION_RUNTIMES: readonly AgentRuntimeKind[] = [
  ...TERMINAL_RUNTIMES,
  "deepseek_harness",
];

/** Existing members retain their original execution path; never create or silently remap one. */
export function isLegacyCollaborationRuntime(kind: AgentRuntimeKind): boolean { return kind === "zcode"; }

export type RuntimeCategory = "terminal" | "chat";

/** Historical kinds remain readable but can never be enabled or launched. */
export function isRetiredRuntime(kind: AgentRuntimeKind | null | undefined): boolean {
  return kind != null && !TERMINAL_RUNTIMES.includes(kind) && !CHAT_RUNTIMES.includes(kind);
}

export function runtimeCategory(kind: AgentRuntimeKind): RuntimeCategory {
  return CHAT_RUNTIMES.includes(kind) ? "chat" : "terminal";
}
