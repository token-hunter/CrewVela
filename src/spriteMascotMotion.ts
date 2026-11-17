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

/** Global atlas pose indices, and the continuous displacement between them. */
export interface SpriteMascotPose { from: number; to: number; mix: number }

const ease = (t: number) => { const v = Math.max(0, Math.min(1, t)); return v * v * (3 - 2 * v); };

export function spriteMascotPose(motions: readonly SpriteMascotMotion[], row: number, elapsed: number): SpriteMascotPose {
  const entry = motions[row];
  const time = Math.max(0, Math.min(elapsed, entry.duration));
  const ends = entry.stops ?? entry.frames.map((_, i) => (i + 1) * entry.duration / entry.frames.length);
  const found = ends.findIndex(end => time < end);
  const index = found < 0 ? entry.frames.length - 1 : found;
  const start = index ? ends[index - 1] : 0;
  const end = ends[index];
  const from = row * 4 + entry.frames[index];
  const to = index + 1 < entry.frames.length ? row * 4 + entry.frames[index + 1] : ((row + 1) % motions.length) * 4 + motions[(row + 1) % motions.length].frames[0];
  // Long blink setup is an intentional rest, not a four-second slow eye closure.
  const transitionStart = entry.stops ? Math.max(start, end - 180) : start;
  return { from, to, mix: ease((time - transitionStart) / (end - transitionStart)) };
}

/** Time advances only while the owner is visible/active. No React updates per frame. */
export class SpriteMascotTimeline {
  motion = 0;
  elapsed = 0;
  private bridge: { previous: SpriteMascotPose; elapsed: number } | null = null;
  constructor(readonly motions: readonly SpriteMascotMotion[]) {}
  sample(): SpriteMascotPose {
    if (!this.bridge) return spriteMascotPose(this.motions, this.motion, this.elapsed);
    const { previous, elapsed } = this.bridge;
    // Finish the interrupted limb movement before moving into the new rest pose.
    // This also works for rapid repeated clicks; never quantize a mid-pose to a keyframe.
    if (elapsed < 100) return { ...previous, mix: previous.mix + (1 - previous.mix) * ease(elapsed / 100) };
    return { from: previous.to, to: this.motion * 4 + this.motions[this.motion].frames[0], mix: ease((elapsed - 100) / 180) };
  }
  next() {
    const previous = this.sample();
    this.motion = (this.motion + 1) % this.motions.length;
    this.elapsed = 0;
    this.bridge = { previous, elapsed: 0 };
  }
  advance(delta: number) {
    let remaining = Math.max(0, delta);
    if (this.bridge) {
      const step = Math.min(remaining, 280 - this.bridge.elapsed);
      this.bridge.elapsed += step; remaining -= step;
      if (this.bridge.elapsed < 280) return;
      this.bridge = null;
    }
    this.elapsed += remaining;
    while (this.elapsed >= this.motions[this.motion].duration) {
      this.elapsed -= this.motions[this.motion].duration;
      this.motion = (this.motion + 1) % this.motions.length;
    }
  }
}
