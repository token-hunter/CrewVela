import type { AgentRuntimeKind } from "./types";

export type InstallPhase = "checking" | "downloading" | "installing" | "verifying" | "ready" | "failed" | "cancelled";
export interface RuntimeInstallJob {
  id: string; kind: AgentRuntimeKind; phase: InstallPhase; message: string;
  downloaded: number; total: number | null; version: string | null; executable: string | null; error: string | null;
}
export interface RuntimeEnvironment {
  kind: AgentRuntimeKind; supported: boolean; executable: string | null;
  version: string | null; latestVersion: string | null; source: string; installDirectory: string;
  auth: "authenticated" | "login_required" | "unverified"; error: string | null; updateError: string | null;
}
export const supportsRuntimeInstallation = (kind: AgentRuntimeKind) => kind === "claude_code" || kind === "codex";
export const installationRunning = (job: RuntimeInstallJob | null) => !!job && !["ready", "failed", "cancelled"].includes(job.phase);
export function installationStep(job: RuntimeInstallJob | null): number {
  if (!job || job.phase === "checking") return 0;
  if (["downloading", "installing"].includes(job.phase)) return 1;
  if (job.phase === "verifying") return 2;
  return job.phase === "ready" ? 3 : 1;
}
export function downloadPercent(job: RuntimeInstallJob | null): number | undefined {
  if (!job?.total || job.total <= 0 || job.phase !== "downloading") return undefined;
  return Math.min(100, Math.max(0, Math.floor(job.downloaded / job.total * 100)));
}
