import orangeSpark from "./assets/pets/orange-spark.png";
import orangeSleepy from "./assets/pets/orange-sleepy.png";
import orangeCoder from "./assets/pets/orange-coder.png";
import orangeAstro from "./assets/pets/orange-astro.png";
import banana from "./assets/pets/banana.png";
import apple from "./assets/pets/apple.png";
import grapes from "./assets/pets/grapes.png";
import strawberry from "./assets/pets/strawberry.png";
import orangeSparkActions from "./assets/pets/animations/orange-spark-animation.png";
import orangeSleepyActions from "./assets/pets/animations/orange-sleepy-animation.png";
import orangeCoderActions from "./assets/pets/animations/orange-coder-animation.png";
import orangeAstroActions from "./assets/pets/animations/orange-astro-animation.png";
import bananaActions from "./assets/pets/animations/banana-animation.png";
import appleActions from "./assets/pets/animations/apple-animation.png";
import grapesActions from "./assets/pets/animations/grapes-animation.png";
import strawberryActions from "./assets/pets/animations/strawberry-animation.png";

export const PET_SETTINGS_STORAGE_KEY = "crewvela-pet-settings-v1";

export interface PetDefinition {
  id: string;
  name: string;
  description: string;
  src: string;
  actionAtlas?: string;
  custom?: boolean;
}

export interface PetSettings {
  enabled: boolean;
  selectedId: string;
  size: number;
  customPets: PetDefinition[];
  position: { x: number; y: number } | null;
}

export const BUILTIN_PETS: readonly PetDefinition[] = [
  { id: "orange-spark", name: "元气橙", description: "元气满满的默认伙伴", src: orangeSpark, actionAtlas: orangeSparkActions },
  { id: "orange-sleepy", name: "困困橙", description: "安静陪伴，适合专注时刻", src: orangeSleepy, actionAtlas: orangeSleepyActions },
  { id: "orange-coder", name: "码农橙", description: "和 Agent 一起敲代码", src: orangeCoder, actionAtlas: orangeCoderActions },
  { id: "orange-astro", name: "星航橙", description: "探索新项目的小宇航员", src: orangeAstro, actionAtlas: orangeAstroActions },
  { id: "banana", name: "香蕉仔", description: "轻快活泼的工作搭子", src: banana, actionAtlas: bananaActions },
  { id: "apple", name: "苹果派", description: "稳定完成每一次交付", src: apple, actionAtlas: appleActions },
  { id: "grapes", name: "葡萄团", description: "最懂多 Agent 协作", src: grapes, actionAtlas: grapesActions },
  { id: "strawberry", name: "草莓莓", description: "热情回应每一个新任务", src: strawberry, actionAtlas: strawberryActions },
] as const;

export const DEFAULT_PET_SETTINGS: PetSettings = {
  enabled: false,
  selectedId: BUILTIN_PETS[0].id,
  size: 112,
  customPets: [],
  position: null,
};

export function clampPetSize(size: number): number {
  return Math.max(72, Math.min(192, Math.round(size)));
}

export function loadPetSettings(): PetSettings {
  try {
    const parsed = JSON.parse(window.localStorage.getItem(PET_SETTINGS_STORAGE_KEY) ?? "null") as
      | Partial<PetSettings>
      | null;
    if (!parsed) return DEFAULT_PET_SETTINGS;
    const customPets = Array.isArray(parsed.customPets)
      ? parsed.customPets.filter(
          (pet): pet is PetDefinition =>
            typeof pet?.id === "string" &&
            typeof pet?.name === "string" &&
            typeof pet?.src === "string" &&
            pet.src.startsWith("data:image/"),
        )
      : [];
    const knownIds = new Set([...BUILTIN_PETS.map((pet) => pet.id), ...customPets.map((pet) => pet.id)]);
    return {
      enabled: parsed.enabled === true,
      selectedId: knownIds.has(parsed.selectedId ?? "") ? parsed.selectedId! : BUILTIN_PETS[0].id,
      size: clampPetSize(parsed.size ?? DEFAULT_PET_SETTINGS.size),
      customPets,
      position:
        parsed.position && Number.isFinite(parsed.position.x) && Number.isFinite(parsed.position.y)
          ? { x: parsed.position.x, y: parsed.position.y }
          : null,
    };
  } catch {
    return DEFAULT_PET_SETTINGS;
  }
}

export function savePetSettings(settings: PetSettings): void {
  window.localStorage.setItem(PET_SETTINGS_STORAGE_KEY, JSON.stringify(settings));
}

export function resolvePet(settings: PetSettings): PetDefinition {
  return (
    BUILTIN_PETS.find((pet) => pet.id === settings.selectedId) ??
    settings.customPets.find((pet) => pet.id === settings.selectedId) ??
    BUILTIN_PETS[0]
  );
}
