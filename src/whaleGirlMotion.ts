export const WHALE_GIRL_MOTIONS = [
  { name: "眨眼", duration: 3000, frames: [0, 1, 2, 3], stops: [2000, 2140, 2300, 3000] },
  { name: "摆尾", duration: 2000, frames: [0, 1, 2, 3, 0, 1, 2, 3] },
  { name: "挥手", duration: 1800, frames: [0, 1, 2, 3, 2, 1, 0] },
  { name: "轻跳", duration: 1400, frames: [0, 1, 2, 3] },
  { name: "歪头", duration: 2000, frames: [0, 1, 2, 3, 2, 0] },
  { name: "思考", duration: 2400, frames: [0, 1, 2, 3] },
  { name: "伸懒腰", duration: 2400, frames: [0, 1, 2, 2, 3] },
  { name: "点头", duration: 1800, frames: [0, 1, 1, 2, 3] },
] as const;

export function whaleGirlFrame(motion: number, elapsed: number): number {
  const entry = WHALE_GIRL_MOTIONS[motion];
  const time = Math.max(0, Math.min(elapsed, entry.duration - 1));
  if ("stops" in entry) return entry.frames[entry.stops.findIndex(end => time < end)];
  return entry.frames[Math.min(entry.frames.length - 1, Math.floor(time / entry.duration * entry.frames.length))];
}
