import type {
  AppAppearance,
  AppearanceAccent,
  AppearanceBackground,
  AppearanceTheme,
  AppTheme,
  ArchivedSessionsSettingsStyle,
  AutomationSettingsStyle,
  BrowserControlSettingsStyle,
  GeneralSettingsStyle,
  GitSettingsStyle,
  NotificationSettingsStyle,
  PrivacySettingsStyle,
  ProjectManagementStyle,
  RightSidebarStartStyle,
  WorktreeSettingsStyle,
} from "./types";

export const APPEARANCE_STORAGE_KEY = "crewvela-appearance";
export const LEGACY_THEME_STORAGE_KEY = "crewvela-theme";
export const RIGHT_SIDEBAR_LAMP_STORAGE_KEY = "crewvela-right-sidebar-lamp-on";
export const RIGHT_SIDEBAR_BREEZE_STORAGE_KEY = "crewvela-right-sidebar-breeze-on";
export const PROJECT_GALLERY_DENSITY_STORAGE_KEY = "crewvela-project-gallery-density";
export const PROJECT_PINBOARD_NEAT_STORAGE_KEY = "crewvela-project-pinboard-neat";
export const MULTI_AGENT_RAIL_COLLAPSED_STORAGE_KEY = "crewvela-multi-agent-rail-collapsed";

export const DEFAULT_RIGHT_SIDEBAR_START_STYLE: RightSidebarStartStyle = "minimal";

export const RIGHT_SIDEBAR_START_STYLES: RightSidebarStartStyle[] = [
  "minimal",
  "lamp",
  "clothesline",
  "cassette",
];

export const DEFAULT_PROJECT_MANAGEMENT_STYLE: ProjectManagementStyle = "classic";

export const PROJECT_MANAGEMENT_STYLES: ProjectManagementStyle[] = [
  "classic",
  "gallery",
  "vinyl",
  "pinboard",
  "studio",
];

export const DEFAULT_GENERAL_SETTINGS_STYLE: GeneralSettingsStyle = "classic";
export const GENERAL_SETTINGS_STYLES: GeneralSettingsStyle[] = ["classic", "coffee"];

export const DEFAULT_GIT_SETTINGS_STYLE: GitSettingsStyle = "classic";
export const GIT_SETTINGS_STYLES: GitSettingsStyle[] = ["classic", "railway"];

export const DEFAULT_WORKTREE_SETTINGS_STYLE: WorktreeSettingsStyle = "classic";
export const WORKTREE_SETTINGS_STYLES: WorktreeSettingsStyle[] = ["classic", "greenhouse"];

export const DEFAULT_NOTIFICATION_SETTINGS_STYLE: NotificationSettingsStyle = "classic";
export const NOTIFICATION_SETTINGS_STYLES: NotificationSettingsStyle[] = [
  "classic",
  "mixer",
];

export const DEFAULT_AUTOMATION_SETTINGS_STYLE: AutomationSettingsStyle = "classic";
export const AUTOMATION_SETTINGS_STYLES: AutomationSettingsStyle[] = ["classic", "sundial"];

export const DEFAULT_BROWSER_CONTROL_SETTINGS_STYLE: BrowserControlSettingsStyle = "classic";
export const BROWSER_CONTROL_SETTINGS_STYLES: BrowserControlSettingsStyle[] = [
  "classic",
  "patchbay",
];

export const DEFAULT_ARCHIVED_SESSIONS_SETTINGS_STYLE: ArchivedSessionsSettingsStyle = "classic";
export const ARCHIVED_SESSIONS_SETTINGS_STYLES: ArchivedSessionsSettingsStyle[] = [
  "classic",
  "tea",
];

export const DEFAULT_PRIVACY_SETTINGS_STYLE: PrivacySettingsStyle = "classic";
export const PRIVACY_SETTINGS_STYLES: PrivacySettingsStyle[] = ["classic", "vault"];

export type ProjectGalleryDensity = "large" | "medium" | "compact";

export const DEFAULT_APPEARANCE: AppAppearance = {
  theme: "dark",
  background: "solid",
  accent: "clay",
  followSystem: false,
  terminalTransparent: false,
  terminalOpacity: 0.65,
  communicationMotion: false,
};

const APPEARANCE_THEMES: AppearanceTheme[] = [
  "dark",
  "light",
  "ocean",
  "purple",
  "sand",
  "aurora",
];
const APPEARANCE_BACKGROUNDS: AppearanceBackground[] = [
  "solid",
  "soft",
  "texture",
];
const APPEARANCE_ACCENTS: AppearanceAccent[] = [
  "clay",
  "none",
  "purple",
  "cyan",
  "orange",
  "pink",
  "coral",
  "gray",
];

export function migrateAppearanceAccent(value: unknown): unknown {
  return value === "blue" || value === "green" ? "none" : value;
}

function includesValue<T extends string>(values: T[], value: unknown): value is T {
  return typeof value === "string" && values.includes(value as T);
}

export function loadAppearance(): AppAppearance {
  try {
    const raw = window.localStorage.getItem(APPEARANCE_STORAGE_KEY);
    if (raw) {
      const stored = JSON.parse(raw) as Partial<AppAppearance>;
      const accent = migrateAppearanceAccent(stored.accent);
      return {
        theme: includesValue(APPEARANCE_THEMES, stored.theme)
          ? stored.theme
          : DEFAULT_APPEARANCE.theme,
        background: includesValue(APPEARANCE_BACKGROUNDS, stored.background)
          ? stored.background
          : DEFAULT_APPEARANCE.background,
        accent: includesValue(APPEARANCE_ACCENTS, accent)
          ? accent
          : DEFAULT_APPEARANCE.accent,
        followSystem: stored.followSystem === true,
        ...(stored.communicationMotion === true ? { communicationMotion: true } : {}),
        terminalTransparent: stored.terminalTransparent === true,
        terminalOpacity:
          typeof stored.terminalOpacity === "number" &&
          Number.isFinite(stored.terminalOpacity)
            ? Math.min(0.9, Math.max(0, stored.terminalOpacity))
            : DEFAULT_APPEARANCE.terminalOpacity,
        ...(includesValue(RIGHT_SIDEBAR_START_STYLES, stored.rightSidebarStartStyle)
          ? { rightSidebarStartStyle: stored.rightSidebarStartStyle }
          : {}),
        ...(includesValue(PROJECT_MANAGEMENT_STYLES, stored.projectManagementStyle)
          ? { projectManagementStyle: stored.projectManagementStyle }
          : {}),
        ...(includesValue(GENERAL_SETTINGS_STYLES, stored.generalSettingsStyle)
          ? { generalSettingsStyle: stored.generalSettingsStyle }
          : {}),
        ...(includesValue(GIT_SETTINGS_STYLES, stored.gitSettingsStyle)
          ? { gitSettingsStyle: stored.gitSettingsStyle }
          : {}),
        ...(includesValue(WORKTREE_SETTINGS_STYLES, stored.worktreeSettingsStyle)
          ? { worktreeSettingsStyle: stored.worktreeSettingsStyle }
          : {}),
        ...(includesValue(NOTIFICATION_SETTINGS_STYLES, stored.notificationSettingsStyle)
          ? { notificationSettingsStyle: stored.notificationSettingsStyle }
          : {}),
        ...(includesValue(AUTOMATION_SETTINGS_STYLES, stored.automationSettingsStyle)
          ? { automationSettingsStyle: stored.automationSettingsStyle }
          : {}),
        ...(includesValue(BROWSER_CONTROL_SETTINGS_STYLES, stored.browserControlSettingsStyle)
          ? { browserControlSettingsStyle: stored.browserControlSettingsStyle }
          : {}),
        ...(includesValue(ARCHIVED_SESSIONS_SETTINGS_STYLES, stored.archivedSessionsSettingsStyle)
          ? { archivedSessionsSettingsStyle: stored.archivedSessionsSettingsStyle }
          : {}),
        ...(includesValue(PRIVACY_SETTINGS_STYLES, stored.privacySettingsStyle)
          ? { privacySettingsStyle: stored.privacySettingsStyle }
          : {}),
      };
    }
    const legacyTheme = window.localStorage.getItem(LEGACY_THEME_STORAGE_KEY);
    return {
      ...DEFAULT_APPEARANCE,
      theme: legacyTheme === "light" ? "light" : "dark",
    };
  } catch {
    return DEFAULT_APPEARANCE;
  }
}

export function resolveAppearanceTheme(
  appearance: AppAppearance,
  prefersDark: boolean,
): AppearanceTheme {
  if (!appearance.followSystem) return appearance.theme;
  return prefersDark ? "dark" : "light";
}

export function resolveBaseTheme(theme: AppearanceTheme): AppTheme {
  // Keep the persisted aurora identifier compatible with its warm porcelain replacement.
  return theme === "light" || theme === "sand" || theme === "aurora" ? "light" : "dark";
}

export function applyAppearanceToRoot(
  appearance: AppAppearance,
  resolvedTheme: AppearanceTheme,
): void {
  const root = document.documentElement;
  const transparent = appearance.terminalTransparent === true ? "true" : "false";
  root.dataset.theme = resolveBaseTheme(resolvedTheme);
  root.dataset.appearanceTheme = resolvedTheme;
  root.dataset.appearanceBackground = appearance.background;
  root.dataset.appearanceAccent = appearance.accent;
  root.dataset.terminalTransparent = transparent;
  root.dataset.globalTransparent = transparent;
  root.style.setProperty(
    "--terminal-pane-opacity",
    `${Math.round((appearance.terminalOpacity ?? 0.65) * 100)}%`,
  );
}

export function saveAppearance(
  appearance: AppAppearance,
  baseTheme: AppTheme,
): void {
  try {
    window.localStorage.setItem(
      APPEARANCE_STORAGE_KEY,
      JSON.stringify(appearance),
    );
    window.localStorage.setItem(LEGACY_THEME_STORAGE_KEY, baseTheme);
  } catch {
    // 持久化不可用时，当前窗口仍可正常切换外观。
  }
}

export function resolveRightSidebarStartStyle(
  appearance: AppAppearance,
): RightSidebarStartStyle {
  return includesValue(RIGHT_SIDEBAR_START_STYLES, appearance.rightSidebarStartStyle)
    ? appearance.rightSidebarStartStyle
    : DEFAULT_RIGHT_SIDEBAR_START_STYLE;
}

export function resolveProjectManagementStyle(
  appearance: AppAppearance,
): ProjectManagementStyle {
  return includesValue(PROJECT_MANAGEMENT_STYLES, appearance.projectManagementStyle)
    ? appearance.projectManagementStyle
    : DEFAULT_PROJECT_MANAGEMENT_STYLE;
}

export function resolveGeneralSettingsStyle(
  appearance: AppAppearance,
): GeneralSettingsStyle {
  return includesValue(GENERAL_SETTINGS_STYLES, appearance.generalSettingsStyle)
    ? appearance.generalSettingsStyle
    : DEFAULT_GENERAL_SETTINGS_STYLE;
}

export function resolveGitSettingsStyle(
  appearance: AppAppearance,
): GitSettingsStyle {
  return includesValue(GIT_SETTINGS_STYLES, appearance.gitSettingsStyle)
    ? appearance.gitSettingsStyle
    : DEFAULT_GIT_SETTINGS_STYLE;
}

export function resolveWorktreeSettingsStyle(
  appearance: AppAppearance,
): WorktreeSettingsStyle {
  return includesValue(WORKTREE_SETTINGS_STYLES, appearance.worktreeSettingsStyle)
    ? appearance.worktreeSettingsStyle
    : DEFAULT_WORKTREE_SETTINGS_STYLE;
}

export function resolveNotificationSettingsStyle(
  appearance: AppAppearance,
): NotificationSettingsStyle {
  return includesValue(NOTIFICATION_SETTINGS_STYLES, appearance.notificationSettingsStyle)
    ? appearance.notificationSettingsStyle
    : DEFAULT_NOTIFICATION_SETTINGS_STYLE;
}

export function resolveAutomationSettingsStyle(
  appearance: AppAppearance,
): AutomationSettingsStyle {
  return includesValue(AUTOMATION_SETTINGS_STYLES, appearance.automationSettingsStyle)
    ? appearance.automationSettingsStyle
    : DEFAULT_AUTOMATION_SETTINGS_STYLE;
}

export function resolveBrowserControlSettingsStyle(
  appearance: AppAppearance,
): BrowserControlSettingsStyle {
  return includesValue(BROWSER_CONTROL_SETTINGS_STYLES, appearance.browserControlSettingsStyle)
    ? appearance.browserControlSettingsStyle
    : DEFAULT_BROWSER_CONTROL_SETTINGS_STYLE;
}

export function resolveArchivedSessionsSettingsStyle(
  appearance: AppAppearance,
): ArchivedSessionsSettingsStyle {
  return includesValue(
    ARCHIVED_SESSIONS_SETTINGS_STYLES,
    appearance.archivedSessionsSettingsStyle,
  )
    ? appearance.archivedSessionsSettingsStyle
    : DEFAULT_ARCHIVED_SESSIONS_SETTINGS_STYLE;
}

export function resolvePrivacySettingsStyle(
  appearance: AppAppearance,
): PrivacySettingsStyle {
  return includesValue(PRIVACY_SETTINGS_STYLES, appearance.privacySettingsStyle)
    ? appearance.privacySettingsStyle
    : DEFAULT_PRIVACY_SETTINGS_STYLE;
}

export function loadProjectGalleryDensity(): ProjectGalleryDensity {
  try {
    const raw = window.localStorage.getItem(PROJECT_GALLERY_DENSITY_STORAGE_KEY);
    return raw === "large" || raw === "compact" ? raw : "medium";
  } catch {
    return "medium";
  }
}

export function saveProjectGalleryDensity(density: ProjectGalleryDensity): void {
  try {
    window.localStorage.setItem(PROJECT_GALLERY_DENSITY_STORAGE_KEY, density);
  } catch {
    // 持久化不可用时保留当前会话内状态。
  }
}

export function loadProjectPinboardNeat(): boolean {
  try {
    return window.localStorage.getItem(PROJECT_PINBOARD_NEAT_STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

export function saveProjectPinboardNeat(neat: boolean): void {
  try {
    window.localStorage.setItem(PROJECT_PINBOARD_NEAT_STORAGE_KEY, neat ? "1" : "0");
  } catch {
    // 持久化不可用时保留当前会话内状态。
  }
}

export function loadRightSidebarLampOn(): boolean {
  try {
    return window.localStorage.getItem(RIGHT_SIDEBAR_LAMP_STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

export function saveRightSidebarLampOn(on: boolean): void {
  try {
    window.localStorage.setItem(RIGHT_SIDEBAR_LAMP_STORAGE_KEY, on ? "1" : "0");
  } catch {
    // 持久化不可用时保留当前会话内状态。
  }
}

export function loadRightSidebarBreezeOn(): boolean {
  try {
    const raw = window.localStorage.getItem(RIGHT_SIDEBAR_BREEZE_STORAGE_KEY);
    return raw === null ? true : raw === "1";
  } catch {
    return true;
  }
}

export function saveRightSidebarBreezeOn(on: boolean): void {
  try {
    window.localStorage.setItem(RIGHT_SIDEBAR_BREEZE_STORAGE_KEY, on ? "1" : "0");
  } catch {
    // 持久化不可用时保留当前会话内状态。
  }
}

export function loadMultiAgentRailCollapsed(): boolean {
  try {
    return window.localStorage.getItem(MULTI_AGENT_RAIL_COLLAPSED_STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

export function saveMultiAgentRailCollapsed(collapsed: boolean): void {
  try {
    window.localStorage.setItem(MULTI_AGENT_RAIL_COLLAPSED_STORAGE_KEY, collapsed ? "1" : "0");
  } catch {
    // 持久化不可用时保留当前会话内状态。
  }
}
