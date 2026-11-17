import type { SpriteMascotMotion } from "./spriteMascotMotion";

export const CLAWD_MOTIONS = [
  { name: "眨眼", duration: 4500, frames: [0, 1, 2, 3], stops: [3700, 3860, 4020, 4500] },
  { name: "敲键盘", duration: 3000, frames: [0, 1, 2, 1, 0, 1, 2, 3] },
  { name: "歪头", duration: 2400, frames: [0, 1, 1, 0, 2, 2, 3] },
  { name: "扶眼镜", duration: 2200, frames: [0, 1, 2, 2, 3, 0] },
  { name: "思考", duration: 3600, frames: [0, 1, 2, 2, 2, 3] },
  // Place the keyboard down, stretch, reach for it, then lift it back into place.
  { name: "伸懒腰", duration: 3600, frames: [0, 1, 2, 2, 2, 3, 1, 0] },
  { name: "挥手", duration: 2200, frames: [0, 1, 2, 1, 2, 3] },
  { name: "点头确认", duration: 1800, frames: [0, 1, 2, 1, 3] },
] as const satisfies readonly SpriteMascotMotion[];
