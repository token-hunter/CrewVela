import type { SettingsSection } from "./components/SettingsDialog";

// 左侧栏「快捷入口」可固定的设置功能。分组与顺序跟随设置页导航。
export interface SidebarShortcutOption {
  id: SettingsSection;
  label: string;
  group: "通用" | "智能体" | "集成" | "数据";
}

export const SIDEBAR_SHORTCUT_OPTIONS: SidebarShortcutOption[] = [
  { id: "generalSettings", label: "常规", group: "通用" },
  { id: "git", label: "Git", group: "通用" },
  { id: "notifications", label: "通知", group: "通用" },
  { id: "appearance", label: "外观", group: "通用" },
  { id: "companionship", label: "陪伴", group: "通用" },
  { id: "providers", label: "模型设置", group: "智能体" },
  { id: "collaboration", label: "协作中心", group: "智能体" },
  { id: "sharedMemory", label: "共享记忆", group: "智能体" },
  { id: "automation", label: "自动化", group: "智能体" },
  { id: "browserControl", label: "浏览器控制", group: "集成" },
  { id: "computerControl", label: "电脑控制", group: "集成" },
  { id: "pluginMarket", label: "插件市场", group: "集成" },
  { id: "skills", label: "Skill 中心", group: "集成" },
  { id: "mcp", label: "MCP", group: "集成" },
  { id: "archivedSessions", label: "归档会话", group: "数据" },
];

export const DEFAULT_SIDEBAR_SHORTCUTS: SettingsSection[] = [
  "automation",
];

export const SIDEBAR_SHORTCUTS_STORAGE_KEY = "crewvela-sidebar-shortcuts";

const KNOWN_IDS = new Set(
  SIDEBAR_SHORTCUT_OPTIONS.map((option) => option.id),
);

// 去重、丢弃未知 id，并按设置页导航顺序输出，保证渲染顺序稳定。
export function sanitizeSidebarShortcuts(value: unknown): SettingsSection[] {
  if (!Array.isArray(value)) return [];
  const seen = new Set<SettingsSection>();
  for (const item of value) {
    if (typeof item === "string" && KNOWN_IDS.has(item as SettingsSection)) {
      seen.add(item as SettingsSection);
    }
  }
  return SIDEBAR_SHORTCUT_OPTIONS.map((option) => option.id).filter((id) =>
    seen.has(id),
  );
}

export function loadSidebarShortcuts(): SettingsSection[] {
  try {
    const raw = window.localStorage.getItem(SIDEBAR_SHORTCUTS_STORAGE_KEY);
    if (raw === null) return [...DEFAULT_SIDEBAR_SHORTCUTS];
    return sanitizeSidebarShortcuts(JSON.parse(raw));
  } catch {
    return [...DEFAULT_SIDEBAR_SHORTCUTS];
  }
}

export function saveSidebarShortcuts(ids: SettingsSection[]): void {
  try {
    window.localStorage.setItem(
      SIDEBAR_SHORTCUTS_STORAGE_KEY,
      JSON.stringify(sanitizeSidebarShortcuts(ids)),
    );
  } catch {
    // 存储不可用时保持当前会话内的选择即可。
  }
}
