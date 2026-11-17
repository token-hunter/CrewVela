export interface SpriteMascotMotion {
  name: string;
  duration: number;
  frames: readonly number[];
  stops?: readonly number[];
}

export function spriteMascotFrame(entry: SpriteMascotMotion, elapsed: number): number {
  const time = Math.max(0, Math.min(elapsed, entry.duration - 1));
  if (entry.stops) return entry.frames[entry.stops.findIndex(end => time < end)];
  return entry.frames[Math.min(entry.frames.length - 1, Math.floor(time / entry.duration * entry.frames.length))];
}
