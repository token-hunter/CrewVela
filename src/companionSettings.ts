import type { AppAppearance } from "./types";
import { DEFAULT_DEEP_SKIN_ID, isDeepSkinId, migrateLegacySkinId, type DeepSkinId } from "./skinIds";

export interface CompanionSettings {
  skinPresetId: DeepSkinId;
  customSkinId: string | null;
  appearanceBeforeSkin: AppAppearance | null;
  skinMotionEnabled: boolean;
  skinSoundEnabled: boolean;
  skinSoundVolume: number;
}

export const COMPANION_STORAGE_KEY = "crewvela-companion-settings-v1";

export const DEFAULT_COMPANION_SETTINGS: CompanionSettings = {
  skinPresetId: DEFAULT_DEEP_SKIN_ID,
  customSkinId: null,
  appearanceBeforeSkin: null,
  skinMotionEnabled: true,
  skinSoundEnabled: false,
  skinSoundVolume: 32,
};

export function loadCompanionSettings(): CompanionSettings {
  try {
    const parsed = JSON.parse(localStorage.getItem(COMPANION_STORAGE_KEY) ?? "null") as Partial<CompanionSettings> | null;
    if (!parsed) return DEFAULT_COMPANION_SETTINGS;
    const linkedSkinId = migrateLegacySkinId(parsed.skinPresetId) ?? DEFAULT_DEEP_SKIN_ID;
    const storedCustomSkinId = typeof parsed.customSkinId === "string" ? parsed.customSkinId : null;
    // 六套旧内置壁纸已移除，加载历史配置时不能继续激活其失效 ID。
    const customSkinId = storedCustomSkinId && !isDeepSkinId(storedCustomSkinId)
      ? storedCustomSkinId
      : null;
    return {
      ...DEFAULT_COMPANION_SETTINGS,
      skinPresetId: isDeepSkinId(storedCustomSkinId) ? storedCustomSkinId : linkedSkinId,
      customSkinId,
      appearanceBeforeSkin: parsed.appearanceBeforeSkin && typeof parsed.appearanceBeforeSkin === "object"
        ? parsed.appearanceBeforeSkin as AppAppearance
        : null,
      skinMotionEnabled: parsed.skinMotionEnabled !== false,
      skinSoundEnabled: parsed.skinSoundEnabled === true,
      skinSoundVolume: Number.isFinite(Number(parsed.skinSoundVolume))
        ? Math.max(0, Math.min(100, Number(parsed.skinSoundVolume)))
        : DEFAULT_COMPANION_SETTINGS.skinSoundVolume,
    };
  } catch {
    return DEFAULT_COMPANION_SETTINGS;
  }
}

export function saveCompanionSettings(settings: CompanionSettings): void {
  try {
    localStorage.setItem(COMPANION_STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // 存储空间不足时仍保留本次运行中的状态。
  }
}
