export const PET_ACTIONS = [
  "idle", "blink", "sleep", "stretch", "walk", "run", "jump", "chase", "read",
  "coffee", "computer", "sing", "eat", "music", "celebrate", "think", "remind", "tumble",
] as const;

export type PetAction = (typeof PET_ACTIONS)[number];
export type PetDirection = "left" | "right";
export interface PetPoint { x: number; y: number }
export interface PetBounds { width: number; height: number }

export interface PetAnimationFrame {
  column: number;
  row: number;
  durationMs: number;
}

export const MOVING_PET_ACTIONS: readonly PetAction[] = ["walk", "run", "jump", "chase"];

const ACTION_DURATION_MS: Record<PetAction, number> = {
  idle: 4200, blink: 900, sleep: 9000, stretch: 2400, walk: 5200, run: 3600,
  jump: 1800, chase: 4300, read: 7200, coffee: 4800, computer: 8000, sing: 6200,
  eat: 5600, music: 6600, celebrate: 3200, think: 5000, remind: 3600, tumble: 2600,
};

const ACTION_ANIMATION_LAYOUT: Record<PetAction, { column: number; row: number; durationMs: number }> = {
  idle: { column: 0, row: 0, durationMs: 420 },
  sleep: { column: 4, row: 0, durationMs: 620 },
  stretch: { column: 7, row: 0, durationMs: 240 },
  walk: { column: 0, row: 1, durationMs: 170 },
  run: { column: 4, row: 1, durationMs: 95 },
  jump: { column: 7, row: 1, durationMs: 150 },
  chase: { column: 0, row: 2, durationMs: 90 },
  read: { column: 3, row: 2, durationMs: 380 },
  coffee: { column: 6, row: 2, durationMs: 260 },
  computer: { column: 0, row: 3, durationMs: 170 },
  sing: { column: 3, row: 3, durationMs: 145 },
  eat: { column: 7, row: 3, durationMs: 210 },
  music: { column: 0, row: 4, durationMs: 165 },
  celebrate: { column: 3, row: 4, durationMs: 135 },
  think: { column: 7, row: 4, durationMs: 340 },
  remind: { column: 0, row: 5, durationMs: 220 },
  tumble: { column: 3, row: 5, durationMs: 115 },
  blink: { column: 7, row: 5, durationMs: 105 },
};

export const PET_ANIMATION_FRAME_COUNT = 3;
export const PET_ANIMATION_COLUMNS = 10;
export const PET_ANIMATION_ROWS = 6;

export function petActionIndex(action: PetAction): number { return PET_ACTIONS.indexOf(action); }
export function petActionDuration(action: PetAction): number { return ACTION_DURATION_MS[action]; }
export function isMovingPetAction(action: PetAction): boolean { return MOVING_PET_ACTIONS.includes(action); }
export function petAnimationFrame(action: PetAction, frame: number): PetAnimationFrame {
  const layout = ACTION_ANIMATION_LAYOUT[action];
  const normalizedFrame = ((Math.floor(frame) % PET_ANIMATION_FRAME_COUNT) + PET_ANIMATION_FRAME_COUNT)
    % PET_ANIMATION_FRAME_COUNT;
  return {
    column: layout.column + normalizedFrame,
    row: layout.row,
    durationMs: layout.durationMs,
  };
}

export function clampPetPoint(point: PetPoint, viewport: PetBounds, petSize: number): PetPoint {
  return {
    x: Math.max(8, Math.min(point.x, viewport.width - petSize - 8)),
    y: Math.max(8, Math.min(point.y, viewport.height - petSize - 8)),
  };
}

export function createPetActionDeck(random: () => number = Math.random): PetAction[] {
  const deck = [...PET_ACTIONS];
  for (let index = deck.length - 1; index > 0; index -= 1) {
    const swapIndex = Math.floor(random() * (index + 1));
    [deck[index], deck[swapIndex]] = [deck[swapIndex], deck[index]];
  }
  return deck;
}

export function createMovementTarget(
  action: PetAction,
  current: PetPoint,
  viewport: PetBounds,
  petSize: number,
  random: () => number = Math.random,
): PetPoint {
  const bounds = {
    width: Math.max(petSize + 16, viewport.width),
    height: Math.max(petSize + 16, viewport.height),
  };
  const maxX = Math.max(8, bounds.width - petSize - 8);
  const maxY = Math.max(8, bounds.height - petSize - 8);

  if (action === "jump") {
    const direction = random() < 0.5 ? -1 : 1;
    return clampPetPoint(
      { x: current.x + direction * Math.max(42, petSize * 0.7), y: current.y }, bounds, petSize,
    );
  }

  const minimumTravel = Math.min(Math.max(100, petSize * 1.25), Math.max(100, maxX - 8));
  let targetX = 8 + random() * Math.max(1, maxX - 8);
  if (Math.abs(targetX - current.x) < minimumTravel) targetX = current.x < bounds.width / 2 ? maxX : 8;
  const verticalRange = action === "chase" ? Math.min(180, bounds.height * 0.24) : 52;
  return clampPetPoint(
    { x: targetX, y: Math.min(current.y + (random() * 2 - 1) * verticalRange, maxY) }, bounds, petSize,
  );
}

export function interpolatePetPosition(
  action: PetAction,
  start: PetPoint,
  target: PetPoint,
  progress: number,
  petSize: number,
): PetPoint {
  const t = Math.max(0, Math.min(progress, 1));
  const eased = action === "run" || action === "chase"
    ? 1 - Math.pow(1 - t, 2)
    : t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2;
  const x = start.x + (target.x - start.x) * eased;
  const baseY = start.y + (target.y - start.y) * eased;
  const jumpHeight = action === "jump" ? Math.max(42, petSize * 0.65) : 0;
  return { x, y: baseY - Math.sin(Math.PI * t) * jumpHeight };
}
