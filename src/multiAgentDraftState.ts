import type {
  AgentRuntimeKind,
  CoordinatorHistoryMessage,
  CoordinatorPlan,
  RuntimeProbe,
} from "./types";

export interface MultiAgentDraftState {
  projectPath: string;
  coordinatorProjectId: string | null;
  coordinatorDraft: string;
  mentionBindings: MentionBinding[];
  coordinatorMessages: CoordinatorHistoryMessage[];
  pendingPlan: CoordinatorPlan | null;
  pendingPlanContext: PendingPlanContext | null;
}

export interface MentionBinding {
  token: string;
  agentId: string;
}

export interface PendingPlanContext {
  projectId: string | null;
  projectPath: string;
  autoStart?: boolean;
  requestedRuntimes?: AgentRuntimeKind[];
}

export type RuntimeHealth = "installed" | "warning" | "missing";

const EMPTY_DRAFT: MultiAgentDraftState = {
  projectPath: "",
  coordinatorProjectId: null,
  coordinatorDraft: "",
  mentionBindings: [],
  coordinatorMessages: [],
  pendingPlan: null,
  pendingPlanContext: null,
};

const MAX_COORDINATOR_HISTORY = 200;

const NULL_LIKE_PLAN_TEXT = new Set([
  "null",
  "undefined",
  "none",
  "n/a",
  "未命名",
  "未命名项目",
]);
const COORDINATOR_INTENTS = new Set([
  "plan_project",
  "add_tasks",
  "start_project",
  "pause_project",
  "resume_project",
  "cancel_project",
  "inspect_project",
  "retry_task",
  "reassign_task",
  "approve_task",
  "unknown",
]);
const RUNTIME_KINDS = new Set([
  "claude_code",
  "codex",
  "kimi_code",
  "pi",
  "oh_my_pi",
  "opencode",
  "deepseek_harness",
  "zcode",
  "zcode_cli",
]);
const AGENT_ROLES = new Set([
  "orchestrator",
  "worker",
  "reviewer",
  "integrator",
]);
const PERMISSION_PROFILES = new Set(["default", "auto", "yolo"]);
const WORKSPACE_MODES = new Set(["read_only", "worktree", "shared_write"]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isCoordinatorMessage(value: unknown): value is CoordinatorHistoryMessage {
  return (
    isRecord(value) &&
    (value.role === "user" || value.role === "assistant") &&
    typeof value.content === "string"
  );
}

function isMentionBinding(value: unknown): value is MentionBinding {
  return (
    isRecord(value) &&
    typeof value.token === "string" &&
    value.token.startsWith("@") &&
    typeof value.agentId === "string" &&
    value.agentId.length > 0
  );
}

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isPlannedAgent(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.key === "string" &&
    typeof value.name === "string" &&
    typeof value.runtimeKind === "string" &&
    RUNTIME_KINDS.has(value.runtimeKind) &&
    typeof value.role === "string" &&
    AGENT_ROLES.has(value.role) &&
    typeof value.description === "string" &&
    isNullableString(value.model) &&
    typeof value.permissionProfile === "string" &&
    PERMISSION_PROFILES.has(value.permissionProfile) &&
    (value.existingAgentId === undefined || isNullableString(value.existingAgentId))
  );
}

function isPlannedTask(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.key === "string" &&
    Number.isInteger(value.stage) &&
    typeof value.title === "string" &&
    typeof value.description === "string" &&
    typeof value.acceptanceCriteria === "string" &&
    isNullableString(value.verificationCommand) &&
    typeof value.assigneeKey === "string" &&
    isNullableString(value.reviewerKey) &&
    Array.isArray(value.dependsOn) &&
    value.dependsOn.every((item) => typeof item === "string") &&
    typeof value.workspaceMode === "string" &&
    WORKSPACE_MODES.has(value.workspaceMode) &&
    Number.isInteger(value.priority)
  );
}

function isCoordinatorPlan(value: unknown): value is CoordinatorPlan {
  return (
    isRecord(value) &&
    typeof value.intent === "string" &&
    COORDINATOR_INTENTS.has(value.intent) &&
    typeof value.reply === "string" &&
    isNullableString(value.projectName) &&
    isNullableString(value.goal) &&
    (value.rootPath === undefined || isNullableString(value.rootPath)) &&
    isNullableString(value.targetTaskId) &&
    isNullableString(value.targetAgentId) &&
    Array.isArray(value.agents) &&
    value.agents.every(isPlannedAgent) &&
    Array.isArray(value.tasks) &&
    value.tasks.every(isPlannedTask) &&
    (value.requestedRuntimes === undefined ||
      (Array.isArray(value.requestedRuntimes) &&
        value.requestedRuntimes.every(
          (runtime) => typeof runtime === "string" && RUNTIME_KINDS.has(runtime),
        ))) &&
    typeof value.requiresConfirmation === "boolean"
  );
}

function isPendingPlanContext(value: unknown): value is PendingPlanContext {
  return (
    isRecord(value) &&
    isNullableString(value.projectId) &&
    typeof value.projectPath === "string" &&
    (value.autoStart === undefined || typeof value.autoStart === "boolean") &&
    (value.requestedRuntimes === undefined ||
      (Array.isArray(value.requestedRuntimes) &&
        value.requestedRuntimes.every(
          (runtime) => typeof runtime === "string" && RUNTIME_KINDS.has(runtime),
        )))
  );
}

export function isUsablePlanText(value: string | null | undefined): boolean {
  const normalized = value?.trim() ?? "";
  return (
    normalized.length > 0 &&
    !NULL_LIKE_PLAN_TEXT.has(normalized.toLocaleLowerCase())
  );
}

function normalizePlanText(value: string | null): string | null {
  return isUsablePlanText(value) ? value!.trim() : null;
}

export function sanitizeGeneratedPlan(plan: CoordinatorPlan): CoordinatorPlan {
  return {
    ...plan,
    projectName: normalizePlanText(plan.projectName),
    goal: normalizePlanText(plan.goal),
    rootPath: normalizePlanText(plan.rootPath ?? null),
    agents: plan.agents.map((agent) => ({
      ...agent,
      existingAgentId: agent.existingAgentId ?? null,
    })),
    requestedRuntimes: plan.requestedRuntimes ?? [],
  };
}

export function planIsLowRiskCommunication(plan: CoordinatorPlan): boolean {
  if (plan.intent !== "plan_project" && plan.intent !== "add_tasks") {
    return false;
  }
  if (plan.agents.length === 0 || plan.tasks.length === 0) {
    return false;
  }
  if (!plan.agents.every((agent) => agent.role === "orchestrator")) {
    return false;
  }
  return plan.tasks.every((task) => {
    const contract = `${task.description}\n${task.acceptanceCriteria}`;
    return (
      !task.verificationCommand &&
      task.workspaceMode === "read_only" &&
      contract.includes("CREWVELA_CLI") &&
      (contract.includes("sessions") || contract.includes("send --to"))
    );
  });
}

export function getRuntimeHealth(
  runtime: Pick<RuntimeProbe, "installed" | "error">,
): RuntimeHealth {
  if (!runtime.installed) return "missing";
  return runtime.error ? "warning" : "installed";
}

export function decodeMultiAgentDraft(raw: string | null): MultiAgentDraftState {
  if (!raw) return { ...EMPTY_DRAFT };
  try {
    const value: unknown = JSON.parse(raw);
    if (!isRecord(value)) return { ...EMPTY_DRAFT };
    const projectPath =
      typeof value.projectPath === "string" ? value.projectPath : "";
    const coordinatorProjectId =
      typeof value.coordinatorProjectId === "string"
        ? value.coordinatorProjectId
        : null;
    const pendingPlan = isCoordinatorPlan(value.pendingPlan)
      ? sanitizeGeneratedPlan(value.pendingPlan)
      : null;
    const pendingPlanContext =
      pendingPlan && isPendingPlanContext(value.pendingPlanContext)
        ? value.pendingPlanContext
        : pendingPlan
          ? {
              projectId:
                pendingPlan.intent === "add_tasks"
                  ? coordinatorProjectId
                  : null,
              projectPath,
            }
          : null;
    return {
      projectPath,
      coordinatorProjectId,
      coordinatorDraft:
        typeof value.coordinatorDraft === "string"
          ? value.coordinatorDraft
          : "",
      mentionBindings: Array.isArray(value.mentionBindings)
        ? value.mentionBindings.filter(isMentionBinding).slice(-100)
        : [],
      coordinatorMessages: Array.isArray(value.coordinatorMessages)
        ? value.coordinatorMessages
            .filter(isCoordinatorMessage)
            .slice(-MAX_COORDINATOR_HISTORY)
        : [],
      pendingPlan,
      pendingPlanContext,
    };
  } catch {
    return { ...EMPTY_DRAFT };
  }
}

export function encodeMultiAgentDraft(state: MultiAgentDraftState): string {
  return JSON.stringify(state);
}
