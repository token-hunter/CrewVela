export type ComposerItem = { id: string; kind: "skills" | "plugins" | "commands"; name: string; label: string; description: string; canonical: string; disabled?: boolean; appAction?: "new-side-session" };
export type ComposerCatalog = Record<"skills" | "plugins" | "commands", { items: Record<string, unknown>[]; error?: string; notice?: string }>;
const labelEscape = (value: string) => value.replace(/\\/g, "\\\\").replace(/\[/g, "\\[").replace(/\]/g, "\\]");
const destinationEscape = (value: string) => value.replace(/\\/g, "\\\\").replace(/>/g, "\\>");
export function normalizeCatalog(value: unknown): ComposerCatalog {
  if (!value || typeof value !== "object") throw new Error("ZCode Desktop 能力目录格式不兼容");
  const raw = value as Record<string, unknown>;
  const result = {} as ComposerCatalog;
  for (const kind of ["skills", "plugins", "commands"] as const) {
    const group = raw[kind] as { items?: unknown; error?: unknown; notice?: unknown } | null;
    if (!group || !Array.isArray(group.items) || group.items.some(item => !item || typeof item !== "object" || Array.isArray(item))) throw new Error(`ZCode Desktop ${kind} 目录格式不兼容`);
    result[kind] = { items: group.items, ...(typeof group.error === "string" ? { error: group.error } : {}), ...(typeof group.notice === "string" ? { notice: group.notice } : {}) };
  }
  return result;
}
export function composerItems(catalog: ComposerCatalog): ComposerItem[] {
  const items: ComposerItem[] = [];
  const names = new Set<string>();
  const priority: Record<string, number> = { workspace: 0, plugin: 1, user: 2 };
  for (const skill of [...catalog.skills.items].sort((a, b) => (priority[String(a.scope)] ?? 3) - (priority[String(b.scope)] ?? 3))) {
    if (!skill.enabled || typeof skill.name !== "string" || typeof skill.path !== "string" || !/^(?:[A-Za-z]:[\\/]|\/|\\\\)/.test(skill.path)) continue;
    const key = skill.name.toLowerCase(); if (names.has(key)) continue; names.add(key);
    items.push({ id: String(skill.id), kind: "skills", name: skill.name, label: skill.name, description: String(skill.description ?? ""), canonical: `[${labelEscape(`$${skill.name}`)}](${destinationEscape(skill.path)})` });
  }
  for (const plugin of catalog.plugins.items) {
    if (!plugin.enabled || typeof plugin.name !== "string" || typeof plugin.pluginId !== "string" || !/^[A-Za-z0-9][A-Za-z0-9._-]*@[A-Za-z0-9][A-Za-z0-9._-]*$/.test(plugin.pluginId)) continue;
    const display = plugin.displayNameI18n as Record<string, string> | undefined;
    items.push({ id: plugin.pluginId, kind: "plugins", name: plugin.name, label: display?.["zh-CN"] || String(plugin.displayName || plugin.name), description: String(plugin.description ?? ""), canonical: `[${labelEscape(`@${plugin.name}`)}](plugin://${destinationEscape(plugin.pluginId)})`, disabled: Array.isArray(plugin.conflictingPluginIds) && plugin.conflictingPluginIds.length > 0 });
  }
  for (const command of catalog.commands.items) {
    if (typeof command.name !== "string") continue;
    const name = command.name.replace(/^\//, "");
    // 仅 side/btw 是 App 合成入口（见 appSideCommands），不作为可发送命令重复出现；
    // goal/compact/plan/init 等内置命令随消息文本交给 runtime 执行（原生 composer 同款）。
    if (["side", "btw"].includes(name.toLowerCase())) continue;
    items.push({ id: `slash:${name}`, kind: "commands", name, label: `/${name}`, description: String(command.description ?? ""), canonical: `/${name}` });
  }
  return items;
}
export function composeMessage(text: string, references: ComposerItem[]): string {
  const trimmed = text.trim();
  const typedCommand = /^\/[A-Za-z0-9._-]+(?=\s|$)/.exec(trimmed)?.[0];
  const commands = references.filter(item => item.kind === "commands");
  const body = typedCommand && !commands.length ? trimmed.slice(typedCommand.length).trimStart() : trimmed;
  return [typedCommand && !commands.length ? typedCommand : "", ...commands.map(item => item.canonical), ...references.filter(item => item.kind !== "commands").map(item => item.canonical), body].filter(Boolean).join(" ");
}
export function rejectedSubmissionDisposition(error: unknown, hasNewDraft: boolean): "review" | "preserve" | "restore" {
  if (!String(error).startsWith("ZCODE_INPUT_REJECTED:")) return "review";
  return hasNewDraft ? "preserve" : "restore";
}
export function submissionOutcome(snapshot: { connected: boolean; busy: boolean; error: string | null; rows: { rowId: number; kind: string }[] }, previousRows: Set<number>): "accepted" | "restore" | "pending" {
  if (snapshot.rows.some(row => row.kind === "userInput" && !previousRows.has(row.rowId))) return "accepted";
  if (!snapshot.busy && (snapshot.error || !snapshot.connected)) return "restore";
  return "pending";
}

/** App 层合成命令（对齐 ZCode 原生 composer 的 /side、/btw）：
 * 不进入消息正文，选择时触发新建辅助会话动作。 */
export type AppSlashAction = "new-side-session";

/** 技能转 / 命令展示：技能在 / 面板以 /name 形态可见，选择即引用该技能（$ 语义）。 */
export function skillSlashCommands(catalog: ComposerCatalog, reservedNames: Set<string>): ComposerItem[] {
  const items: ComposerItem[] = [];
  const seen = new Set<string>();
  for (const skill of catalog.skills.items) {
    if (!skill.enabled || typeof skill.name !== "string") continue;
    const name = skill.name.replace(/^\//, "");
    const key = name.toLowerCase();
    if (!key || seen.has(key) || reservedNames.has(key)) continue;
    seen.add(key);
    if (!/^(?:[A-Za-z]:[\/]|\/|\\)/.test(String(skill.path ?? ""))) continue;
    items.push({
      id: `slash-skill:${skill.id}`,
      kind: "commands",
      name,
      label: `/${name}`,
      description: String(skill.description ?? "") + "（引用技能）",
      canonical: `[${labelEscape(`$${skill.name}`)}](${destinationEscape(String(skill.path))})`,
    });
  }
  return items;
}

/** App 合成的 /side、/btw 入口；与既有命令重名时让位。 */
export function appSideCommands(reservedNames: Set<string>): ComposerItem[] {
  const description = "新建并打开一个辅助对话";
  return ["side", "btw"].filter(name => !reservedNames.has(name)).map(name => ({
    id: `app:${name}`,
    kind: "commands" as const,
    name,
    label: `/${name}`,
    description,
    canonical: "",
    appAction: "new-side-session" as const,
  }));
}

/** 原生桌面在发送前消费的可见斜杠命令（packages/ui v4 slashCommands.ts 同语义移植）：
 * 命中项走类型化 v4/command 信封（sendGoalCommand/compact/resumeGoal），未命中项
 * 保持普通 sendText 由 runtime 解析。/plan 是模式快捷方式：随 sendText 声明 planEnabled。 */
export type VisibleSlashCommand =
  | { kind: "compact"; displayText: string }
  | { kind: "planShortcut"; task: string; displayText: string }
  | { kind: "unsupportedPlanShortcut"; task: string; displayText: string }
  | { kind: "sendGoalCommand"; objective: string; displayText: string }
  | { kind: "resumeGoal"; displayText: string }
  | { kind: "emptyGoal"; displayText: string }
  | { kind: "unsupportedGoal"; action: string; displayText: string };

export function parseVisibleSlashCommand(content: string, contextAttachmentCount = 0): VisibleSlashCommand | null {
  const displayText = content.trim();
  if (!displayText.startsWith("/")) return null;
  const match = /^\/([^\s]+)(?:\s+([\s\S]*))?$/.exec(displayText);
  if (!match) return null;
  const commandName = (match[1] ?? "").toLowerCase();
  const args = (match[2] ?? "").trim();
  if (commandName === "plan") {
    // 与原生一致：/plan 不接受附件/引用上下文，否则会退化成普通 prompt 绕过产品边界。
    return contextAttachmentCount > 0
      ? { kind: "unsupportedPlanShortcut", task: args, displayText }
      : { kind: "planShortcut", task: args, displayText };
  }
  if (contextAttachmentCount > 0) return null;
  if (commandName === "compact" || commandName === "compress") return { kind: "compact", displayText };
  if (commandName !== "goal" && commandName !== "target") return null;
  if (!args) return { kind: "emptyGoal", displayText };
  const action = args.split(/\s+/, 1)[0]?.toLowerCase() ?? "";
  if (action === "resume") return { kind: "resumeGoal", displayText };
  if (action === "pause" || action === "clear" || action === "show") return { kind: "unsupportedGoal", action, displayText };
  const objective = action === "replace" ? args.replace(/^replace\s*/i, "").trim() : args;
  if (!objective) return { kind: "emptyGoal", displayText };
  return { kind: "sendGoalCommand", objective, displayText };
}
