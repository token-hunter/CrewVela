import type {
  AppAppearance,
  AppearanceAccent,
  AppearanceBackground,
  AppearanceTheme,
} from "./types";

export interface CustomSkin {
  id: string;
  name: string;
  description: string;
  appearance: AppAppearance;
  importedAt: number;
  media?: DynamicSkinMedia;
}

export interface DynamicSkinMedia {
  kind: "video";
  videoPath: string;
  previewPath: string | null;
  audioPath: string | null;
  videoHasAudio: boolean;
  sourceFormat: string;
}

export const CUSTOM_SKINS_STORAGE_KEY = "crewvela-custom-skins-v1";

const THEMES: AppearanceTheme[] = ["dark", "light", "ocean", "purple", "sand", "aurora"];
const BACKGROUNDS: AppearanceBackground[] = ["solid", "soft", "texture"];
const ACCENTS: AppearanceAccent[] = ["blue", "purple", "cyan", "green", "orange", "pink", "coral", "gray"];

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function parseAppearance(value: unknown): AppAppearance {
  if (!isRecord(value)) throw new Error("皮肤缺少 appearance 配置");
  if (!THEMES.includes(value.theme as AppearanceTheme)) throw new Error("不支持的主题类型");
  if (!BACKGROUNDS.includes(value.background as AppearanceBackground)) throw new Error("不支持的背景类型");
  if (!ACCENTS.includes(value.accent as AppearanceAccent)) throw new Error("不支持的强调色");
  return {
    theme: value.theme as AppearanceTheme,
    background: value.background as AppearanceBackground,
    accent: value.accent as AppearanceAccent,
    followSystem: value.followSystem === true,
  };
}

export function parseCustomSkin(raw: string, fallbackName = "自定义皮肤"): CustomSkin {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new Error("文件不是有效的 JSON 皮肤包");
  }
  if (!isRecord(parsed)) throw new Error("皮肤包内容无效");
  const appearanceSource = isRecord(parsed.appearance) ? parsed.appearance : parsed;
  const name = typeof parsed.name === "string" && parsed.name.trim() ? parsed.name.trim().slice(0, 40) : fallbackName.slice(0, 40);
  const description = typeof parsed.description === "string" ? parsed.description.trim().slice(0, 100) : "导入的自定义皮肤";
  return {
    id: `skin-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    name,
    description,
    appearance: parseAppearance(appearanceSource),
    importedAt: Date.now(),
  };
}

export function loadCustomSkins(): CustomSkin[] {
  try {
    const parsed = JSON.parse(localStorage.getItem(CUSTOM_SKINS_STORAGE_KEY) ?? "[]") as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.flatMap((item) => {
      if (!isRecord(item) || typeof item.id !== "string" || typeof item.name !== "string") return [];
      try {
        const mediaSource = isRecord(item.media) ? item.media : null;
        const media = mediaSource
          && mediaSource.kind === "video"
          && typeof mediaSource.videoPath === "string"
          && mediaSource.videoPath.trim()
          ? {
              kind: "video" as const,
              videoPath: mediaSource.videoPath,
              previewPath: typeof mediaSource.previewPath === "string" ? mediaSource.previewPath : null,
              audioPath: typeof mediaSource.audioPath === "string" ? mediaSource.audioPath : null,
              videoHasAudio: mediaSource.videoHasAudio === true,
              sourceFormat: typeof mediaSource.sourceFormat === "string" ? mediaSource.sourceFormat : "PKGM0014",
            }
          : undefined;
        return [{
          id: item.id,
          name: item.name.slice(0, 40),
          description: typeof item.description === "string" ? item.description.slice(0, 100) : "导入的自定义皮肤",
          appearance: parseAppearance(item.appearance),
          importedAt: typeof item.importedAt === "number" ? item.importedAt : 0,
          ...(media ? { media } : {}),
        }];
      } catch {
        return [];
      }
    });
  } catch {
    return [];
  }
}

export function saveCustomSkins(skins: CustomSkin[]): void {
  localStorage.setItem(CUSTOM_SKINS_STORAGE_KEY, JSON.stringify(skins));
}
