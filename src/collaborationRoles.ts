import type { AgentRole } from "./types";

export const COLLABORATION_ROLE_LABELS: Record<AgentRole, string> = {
  worker: "开发执行",
  reviewer: "独立审查",
  integrator: "联调交付",
  orchestrator: "任务规划",
};
