export type DeepSkinId =
  | "deep-orange-machine"
  | "deep-rain-office"
  | "deep-navigation-room"
  | "deep-paper-lab"
  | "deep-midnight-records"
  | "deep-snow-station";

export const DEEP_SKIN_IDS = [
  "deep-orange-machine",
  "deep-rain-office",
  "deep-navigation-room",
  "deep-paper-lab",
  "deep-midnight-records",
  "deep-snow-station",
] as const satisfies readonly DeepSkinId[];

export const DEFAULT_DEEP_SKIN_ID: DeepSkinId = "deep-orange-machine";

const LEGACY_SKIN_MIGRATION: Readonly<Record<string, DeepSkinId>> = {
  "native-dark": "deep-orange-machine",
  "warm-orange": "deep-orange-machine",
  "city-rain": "deep-rain-office",
  "moon-study": "deep-paper-lab",
  "retro-terminal": "deep-midnight-records",
  "morning-light": "deep-snow-station",
};

export function isDeepSkinId(value: unknown): value is DeepSkinId {
  return DEEP_SKIN_IDS.some((id) => id === value);
}

export function migrateLegacySkinId(value: unknown): DeepSkinId | undefined {
  if (isDeepSkinId(value)) return value;
  return typeof value === "string" ? LEGACY_SKIN_MIGRATION[value] : undefined;
}
