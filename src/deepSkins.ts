import type { AppAppearance } from "./types";
import type { DeepSkinId } from "./skinIds";
export type { DeepSkinId } from "./skinIds";

export interface DeepSkinDefinition {
  id: DeepSkinId;
  name: string;
  description: string;
  background: string;
  appearance: AppAppearance;
  atmosphere?: "rain" | "navigation" | "records";
}

// 内置壁纸已移除。这里保留类型与查询函数，兼容旧配置的安全迁移。
export const DEEP_SKINS: DeepSkinDefinition[] = [];

export function findDeepSkin(id: string | null | undefined): DeepSkinDefinition | undefined {
  return DEEP_SKINS.find((skin) => skin.id === id);
}
