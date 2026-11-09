import type { SessionStatus } from "../types";

export type PetWorkPhase = "idle" | "typing" | "busy" | "overheated";

const BUSY_AFTER_MS = 3 * 60 * 1000;
const OVERHEATED_AFTER_MS = 8 * 60 * 1000;

export function getPetWorkPhase(
  status: SessionStatus,
  elapsedMs: number,
): PetWorkPhase {
  if (status !== "running") return "idle";

  const safeElapsedMs = Math.max(0, elapsedMs);
  if (safeElapsedMs >= OVERHEATED_AFTER_MS) return "overheated";
  if (safeElapsedMs >= BUSY_AFTER_MS) return "busy";
  return "typing";
}
