import type {
  AppAppearance,
  AppearanceAccent,
  AppearanceBackground,
  AppearanceTheme,
  AppTheme,
} from "./types";

export const APPEARANCE_STORAGE_KEY = "crewvela-appearance";
export const LEGACY_THEME_STORAGE_KEY = "crewvela-theme";

export const DEFAULT_APPEARANCE: AppAppearance = {
  theme: "dark",
  background: "solid",
  accent: "blue",
  followSystem: false,
  terminalTransparent: false,
  terminalOpacity: 0.65,
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
  "blue",
  "purple",
  "cyan",
  "green",
  "orange",
  "pink",
  "coral",
  "gray",
];

function includesValue<T extends string>(values: T[], value: unknown): value is T {
  return typeof value === "string" && values.includes(value as T);
}

export function loadAppearance(): AppAppearance {
  try {
    const raw = window.localStorage.getItem(APPEARANCE_STORAGE_KEY);
    if (raw) {
      const stored = JSON.parse(raw) as Partial<AppAppearance>;
      return {
        theme: includesValue(APPEARANCE_THEMES, stored.theme)
          ? stored.theme
          : DEFAULT_APPEARANCE.theme,
        background: includesValue(APPEARANCE_BACKGROUNDS, stored.background)
          ? stored.background
          : DEFAULT_APPEARANCE.background,
        accent: includesValue(APPEARANCE_ACCENTS, stored.accent)
          ? stored.accent
          : DEFAULT_APPEARANCE.accent,
        followSystem: stored.followSystem === true,
        terminalTransparent: stored.terminalTransparent === true,
        terminalOpacity:
          typeof stored.terminalOpacity === "number" &&
          Number.isFinite(stored.terminalOpacity)
            ? Math.min(0.9, Math.max(0, stored.terminalOpacity))
            : DEFAULT_APPEARANCE.terminalOpacity,
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
  return theme === "light" || theme === "sand" ? "light" : "dark";
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
