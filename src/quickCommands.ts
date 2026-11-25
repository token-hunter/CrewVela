import type { AgentRuntimeKind, Project, ProviderConfig, RuntimeProbe, SessionRecord } from "./types";
import { TERMINAL_RUNTIMES, CHAT_RUNTIMES } from "./runtimeTaxonomy.ts";

export type QuickAction = "send" | "launch" | "stop" | "delete";
export type QuickScope = "workspace" | "project" | "all" | "selected";
export interface QuickCommand {
  id: string;
  title: string;
  action: QuickAction;
  body: string;
  scope: QuickScope;
  runtimeKind?: AgentRuntimeKind;
  providerId?: string;
  cwd?: string;
  count?: number;
}
export interface QuickCommandSettings {
  version: 1;
  commands: QuickCommand[];
  favorites: string[];
  recent: string[];
  defaultScope: QuickScope;
}
export interface QuickContext {
  workspaceSessionIds?: readonly string[];
  projectId?: string | null;
  cwd?: string | null;
  projects?: readonly Project[];
}
export const QUICK_COMMAND_STORAGE_KEY = "crewvela-quick-commands-v1";
export const QUICK_COMMAND_CHANGED = "crewvela-quick-commands-changed";
export const SCOPE_LABELS: Record<QuickScope, string> = {
  workspace: "当前工作区", project: "当前项目", all: "全部运行会话", selected: "选中会话",
};
export const ACTION_LABELS: Record<QuickAction, string> = {
  send: "发送消息", launch: "启动 Agent 会话", stop: "停止会话", delete: "从列表删除会话",
};
export const RUNTIME_LABELS: Partial<Record<AgentRuntimeKind, string>> = {
  claude_code: "Claude Code", codex: "Codex CLI", kimi_code: "Kimi Code", pi: "Pi", opencode: "OpenCode", zcode_cli: "ZCode CLI",
};
export function emptyQuickSettings(): QuickCommandSettings {
  return { version: 1, commands: [], favorites: [], recent: [], defaultScope: "workspace" };
}
export function validateQuickCommand(value: unknown): QuickCommand {
  const c = value as QuickCommand;
  if (!c || typeof c.id !== "string" || !c.id || c.id.length > 160 ||
    typeof c.title !== "string" || !c.title.trim() || c.title.length > 100 ||
    !Object.prototype.hasOwnProperty.call(ACTION_LABELS, c.action) || !Object.prototype.hasOwnProperty.call(SCOPE_LABELS, c.scope) ||
    typeof c.body !== "string" || Array.from(c.body).length > 32000 ||
    (c.runtimeKind !== undefined && !TERMINAL_RUNTIMES.includes(c.runtimeKind)) ||
    (c.providerId !== undefined && (typeof c.providerId !== "string" || !c.providerId || c.providerId.length > 160)) ||
    (c.providerId !== undefined && c.runtimeKind !== "claude_code") ||
    (c.cwd !== undefined && (typeof c.cwd !== "string" || c.cwd.length > 4096)) ||
    (c.count !== undefined && (!Number.isInteger(c.count) || c.count < 1 || c.count > 12))) throw new Error("快捷指令格式无效，请检查名称、动作和目标配置");
  return { id: c.id, title: c.title.trim(), action: c.action, scope: c.scope, body: c.body,
    ...(c.runtimeKind ? { runtimeKind: c.runtimeKind } : {}), ...(c.providerId ? { providerId: c.providerId } : {}),
    ...(c.action === "launch" && c.cwd !== undefined ? { cwd: c.cwd } : {}),
    ...(c.action === "launch" && c.count !== undefined ? { count: c.count } : {}) };
}
export function readQuickSettings(storage: Pick<Storage, "getItem">): QuickCommandSettings {
  const raw = storage.getItem(QUICK_COMMAND_STORAGE_KEY);
  if (raw === null) return emptyQuickSettings();
  const data = JSON.parse(raw) as QuickCommandSettings;
  if (!data || data.version !== 1 || !Array.isArray(data.commands) || data.commands.length > 100 ||
    !Array.isArray(data.favorites) || !Array.isArray(data.recent) || !Object.prototype.hasOwnProperty.call(SCOPE_LABELS, data.defaultScope) ||
    [...data.favorites, ...data.recent].some(id => typeof id !== "string" || id.length > 160) ||
    data.favorites.length > 300 || data.recent.length > 20) throw new Error("快捷指令配置损坏，原配置已保留");
  const commands = data.commands.map(validateQuickCommand);
  if (new Set(commands.map(c => c.id)).size !== commands.length) throw new Error("快捷指令 ID 重复，原配置已保留");
  return { version: 1, commands, favorites: [...new Set(data.favorites)], recent: [...new Set(data.recent)], defaultScope: data.defaultScope };
}
export function writeQuickSettings(storage: Pick<Storage, "setItem">, data: QuickCommandSettings): void {
  // Round-trip validation also strips accidental fields such as API credentials.
  const clean = readQuickSettings({ getItem: () => JSON.stringify(data) });
  storage.setItem(QUICK_COMMAND_STORAGE_KEY, JSON.stringify(clean));
}
export function sessionRuntime(s: SessionRecord): AgentRuntimeKind | null {
  return s.agentRuntimeKind ?? (s.kind === "claude" ? "claude_code" : null);
}
export function isQuickAgentSession(s: SessionRecord): boolean {
  const kind = sessionRuntime(s);
  return !s.archivedAt && !s.retiredAgentRuntime && !s.runtimeCommandHost && kind !== null &&
    [...TERMINAL_RUNTIMES, ...CHAT_RUNTIMES].includes(kind);
}
export function isQuickSessionLive(s: SessionRecord): boolean {
  // Native chat processes use no PTY. Completed means idle, not stopped.
  return isQuickAgentSession(s) && s.status !== "stopped" && s.status !== "error" &&
    (s.ptyId !== null || ["running", "completed", "waiting"].includes(s.status));
}
export function scopeSessions(sessions: readonly SessionRecord[], scope: QuickScope, context: QuickContext, action: QuickAction): SessionRecord[] {
  const candidates = sessions.filter(action === "delete" ? isQuickAgentSession : isQuickSessionLive);
  if (scope === "workspace") return context.workspaceSessionIds === undefined ? [] : candidates.filter(s => context.workspaceSessionIds!.includes(s.id));
  if (scope === "project") return context.projectId ? candidates.filter(s => s.projectId === context.projectId) : [];
  return candidates;
}
export function quickCommandCatalog(probes: RuntimeProbe[], providers: ProviderConfig[], keys: Record<string, boolean>, enabled: (kind: AgentRuntimeKind) => boolean, t: (key: string, values?: Record<string, string | number>) => string = (key, values) => values ? key.replace("{agent}", String(values.agent)) : key): QuickCommand[] {
  const commands: QuickCommand[] = [
    { id: "builtin:send", title: t("向会话发送消息"), action: "send", body: t("检查当前项目并给出下一步建议"), scope: "workspace" },
  ];
  for (const kind of TERMINAL_RUNTIMES) {
    if (!enabled(kind) || !probes.some(p => p.kind === kind && p.installed && !p.error)) continue;
    commands.push({ id: `runtime:${kind}`, title: t("启动 {agent}", { agent: RUNTIME_LABELS[kind] ?? kind }), action: "launch", runtimeKind: kind, scope: "workspace", body: "" });
    if (kind === "codex") commands.push({ id: "builtin:review", title: t("请 Codex 审查当前改动"), action: "send", runtimeKind: "codex", scope: "workspace", body: t("请审查当前工作目录的未提交改动，按严重程度列出有依据的问题、文件位置及验证建议。不要修改代码。") });
    if (kind === "claude_code") for (const provider of providers.filter(p => !p.hidden && keys[p.id])) commands.push({
      id: `provider:${provider.id}`, title: `${t("启动 {agent}", { agent: "Claude Code" })} · ${provider.name}`, action: "launch", runtimeKind: kind, providerId: provider.id, scope: "workspace", body: "",
    });
  }
  commands.push({ id: "builtin:stop", title: t("批量停止会话"), action: "stop", body: "", scope: "workspace" },
    { id: "builtin:delete", title: t("批量从列表删除会话"), action: "delete", body: "", scope: "workspace" });
  return commands;
}
export function targetIdentity(s: SessionRecord): string {
  return JSON.stringify([s.id, s.kind, s.agentRuntimeKind ?? null, s.providerId, s.cwd, s.projectId ?? null, s.multiAgentProjectId ?? null, s.multiAgentAgentId ?? null]);
}
