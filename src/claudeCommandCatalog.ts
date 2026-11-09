import type { HarnessSessionSnapshot } from "./types";

/** Claude Code Chat 菜单里的一个命令条目（来自 ACP available_commands）。 */
export interface ClaudeMenuCommand {
  name: string;
  description?: string;
  inputHint?: string;
}

export type ClaudeCommandGroup = "builtin" | "skill" | "plugin";

export interface GroupedClaudeCommands {
  builtin: ClaudeMenuCommand[];
  skill: ClaudeMenuCommand[];
  plugin: ClaudeMenuCommand[];
}

/**
 * Claude Code 内置命令目录：仅用于菜单分组展示，不影响命令的实际发送与执行。
 * 来源：CrewVela 实测（claude CLI 2.1.285 经 SDK/ACP 报告的内置命令）+ 官方 TUI
 * 命令面板中可能进入无头通道的条目。目录没覆盖到的名字一律按「技能/自定义」展示，
 * 误分类只影响分组位置，不会让命令失效。
 */
const BUILTIN_COMMANDS = new Set([
  // 会话与上下文
  "compact", "context", "usage", "cost", "status", "clear", "resume", "rewind",
  "export", "help", "exit", "init", "add-dir", "rename", "recap", "notes",
  // 模型与执行控制
  "model", "effort", "fast", "think", "eta", "batch", "run", "loop", "orchestration",
  // 权限与安全
  "permissions", "sandbox", "security-review", "verify", "fewer-permission-prompts",
  "update-config", "config", "login", "logout", "install-github-app", "doctor",
  // 智能体与工具生态
  "agents", "mcp", "memory", "hooks", "review", "pr-comments", "todos",
  "output-style", "release-notes", "terminal-setup", "vim", "bug",
  // 2.1.x 新增/实验
  "deep-research", "simplify", "insights", "debug", "heapdump", "reload-skills",
  "color", "dataviz", "goal", "upgrade", "fetch", "slides", "split", "medical", "mood",
]);

/** 插件命令带命名空间（如 ponytail:ponytail-review），其余按目录判定内置。 */
export function classifyClaudeCommand(name: string): ClaudeCommandGroup {
  if (name.includes(":")) return "plugin";
  return BUILTIN_COMMANDS.has(name) ? "builtin" : "skill";
}

/**
 * 同名去重：保留先出现的一条；若先出现的没有描述而后者有，则用后者补位。
 * 分类按名字判定，重名条目分组必然一致，因此只需挑信息更全的一条。
 * （上游 parse_commands 已做一层首现去重；这里兜底处理历史持久化数据里的重复。）
 */
export function dedupeClaudeCommands(commands: ClaudeMenuCommand[]): ClaudeMenuCommand[] {
  const byName = new Map<string, ClaudeMenuCommand>();
  for (const command of commands) {
    const existing = byName.get(command.name);
    if (!existing || (!existing.description && command.description)) {
      byName.set(command.name, command);
    }
  }
  return [...byName.values()];
}

/** 按内置 / 技能 / 插件三段分组，组内保持上游字母序。 */
export function groupClaudeCommands(commands: ClaudeMenuCommand[]): GroupedClaudeCommands {
  const grouped: GroupedClaudeCommands = { builtin: [], skill: [], plugin: [] };
  for (const command of dedupeClaudeCommands(commands)) {
    grouped[classifyClaudeCommand(command.name)].push(command);
  }
  return grouped;
}

export const CLAUDE_COMMAND_GROUP_LABELS: Record<ClaudeCommandGroup, string> = {
  builtin: "内置命令",
  skill: "技能",
  plugin: "插件命令",
};

export function snapshotClaudeCommands(snapshot: Pick<HarnessSessionSnapshot, "commands"> | null | undefined): ClaudeMenuCommand[] {
  return (snapshot?.commands ?? []).map((command) => ({ name: command.name, description: command.description, inputHint: command.inputHint }));
}
