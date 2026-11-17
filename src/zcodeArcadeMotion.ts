import type { SpriteMascotMotion } from "./spriteMascotMotion";

export const ZCODE_ARCADE_MOTIONS = [
  { name: "摇杆漂移", duration: 2800, frames: [0, 0, 1, 2, 2, 1, 3, 3] },
  { name: "手柄翻接", duration: 2200, frames: [0, 1, 2, 3], stops: [700, 1000, 1400, 2200] },
  { name: "蹲身闪避", duration: 2000, frames: [0, 1, 2, 2, 1, 3] },
  { name: "扳机瞄准", duration: 2800, frames: [0, 1, 2, 2, 2, 3] },
  { name: "蓄力出招", duration: 3200, frames: [0, 1, 1, 2, 2, 3] },
  { name: "耳麦报点", duration: 2600, frames: [0, 1, 2, 1, 2, 3] },
  { name: "节奏踏步", duration: 2800, frames: [0, 1, 2, 1, 2, 1, 2, 3] },
  { name: "通关亮牌", duration: 3200, frames: [0, 1, 2, 2, 2, 3] },
] as const satisfies readonly SpriteMascotMotion[];
