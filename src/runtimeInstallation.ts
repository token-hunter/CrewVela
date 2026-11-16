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
  dependencyMessage?: string;
}
export const supportsRuntimeInstallation = (kind: AgentRuntimeKind) => ["claude_code", "codex", "opencode", "kimi_code", "pi"].includes(kind);
export function runtimeInstallationInfo(kind: AgentRuntimeKind) {
  return {
    method: kind === "claude_code" ? "官方原生程序" : "官方原生平台包",
    checksum: kind === "codex" || kind === "opencode" ? "官方 npm 平台包 SHA512" : kind === "claude_code" ? "官方发行清单 SHA256" : "官方 GitHub 发行包 SHA256",
    channel: kind === "opencode" ? "OpenCode 1.x 兼容渠道" : kind === "kimi_code" ? "MoonshotAI/kimi-code" : kind === "pi" ? "earendil-works/pi" : null,
    loginHint: kind === "pi" ? "进入 Pi 后输入 /login，选择需要登录的 Provider。" : "完成原生 CLI 登录后，点击“重新检查”。",
    loginLabel: kind === "pi" ? "打开 Pi 登录引导" : "打开 CLI 登录",
  };
}
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
