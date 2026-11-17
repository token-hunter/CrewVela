import type { SpriteMascotPose } from "./spriteMascotMotion";

export interface SpriteMascotFrameLayout { columns: number; rows: number; count: number; framePixels: number }
export interface SpriteMascotRigidGuide { word: number[][]; bounds: number[]; kind?: string; maskTile?: number }

/** Column-major inverse of the card's affine text plane (a rectangle in source
 * artwork). Interpolating corresponding corners preserves its rigid lettering. */
export function spriteMascotRigidTransform(points: readonly number[][]) {
  const [origin, right, , bottom] = points;
  const a = right[0] - origin[0], b = bottom[0] - origin[0];
  const c = right[1] - origin[1], d = bottom[1] - origin[1], det = a * d - b * c;
  if (!Number.isFinite(det) || Math.abs(det) < 1e-8) throw new Error("Invalid mascot rigid prop plane");
  return new Float32Array([d / det, -c / det, 0, -b / det, a / det, 0,
    (b * origin[1] - d * origin[0]) / det, (c * origin[0] - a * origin[1]) / det, 1]);
}
export interface SpriteMascotTransitionData {
  routes: Record<string, number[]>;
  layouts: SpriteMascotFrameLayout[];
  samples: number[];
  routeStops?: Record<string, number[]>;
  segmentStops?: Record<string, number[]>;
}

/** Resolve an authored route without changing the action timeline. Every
 * interval is still continuously interpolated; no endpoint is hard-switched. */
export function spriteMascotTransition(pose: SpriteMascotPose, data: SpriteMascotTransitionData) {
  const row = Math.floor(pose.from / 4);
  const a = pose.from % 4, b = pose.to % 4;
  const key = `${row}:${Math.min(a, b)}:${Math.max(a, b)}`;
  const authored = a === b ? [a === 3 ? 0 : a, a === 3 ? 0 : a] : data.routes[key];
  if (!authored || authored.length < 2) throw new Error(`Missing mascot pose route ${key}`);
  const route = a > b ? [...authored].reverse() : authored;
  const authoredStops = data.routeStops?.[key] ?? authored.map((_, i) => i / (authored.length - 1));
  const stops = a > b ? [...authoredStops].reverse().map(stop => 1 - stop) : authoredStops;
  const value = Math.max(0, Math.min(1, pose.mix));
  const following = stops.findIndex(stop => stop > value);
  const index = following < 0 ? route.length - 2 : Math.max(0, following - 1);
  const from = route[index], to = route[index + 1];
  const phase = (value - stops[index]) / (stops[index + 1] - stops[index]);
  const fieldKey = `${row}:${Math.min(from, to)}:${Math.max(from, to)}`;
  const distance = from > to ? 1 - phase : phase;
  const arc = data.segmentStops?.[fieldKey] ?? data.samples;
  const arcUpper = arc.findIndex(stop => stop > distance);
  const arcIndex = arcUpper < 0 ? arc.length - 2 : Math.max(0, arcUpper - 1);
  const fraction = (distance - arc[arcIndex]) / (arc[arcIndex + 1] - arc[arcIndex]);
  const time = data.samples[arcIndex] + (data.samples[arcIndex + 1] - data.samples[arcIndex]) * fraction;
  const mix = from > to ? 1 - time : time;
  const upper = data.samples.findIndex(sample => sample > time);
  const sample = upper < 0 ? data.samples.length - 2 : Math.max(0, upper - 1);
  const start = data.samples[sample], end = data.samples[sample + 1];
  return { row, from, to, mix, fieldKey,
    sample, sampleMix: (time - start) / (end - start), layout: data.layouts[row] };
}
