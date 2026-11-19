export interface ProviderConfig {
  id: string;
  name: string;
  baseUrl: string;
  authTokenRef: string;
  env: Record<string, string>;
  timeoutMs: number | null;
  disableNonessentialTraffic: boolean;
  builtin: boolean;
  hidden?: boolean;
  meta?: ProviderMeta;
}

export type ProviderApiFormat =
  | "anthropic"
  | "openai_chat"
  | "openai_responses"
  | "gemini_native";

export type ProviderApiKeyField =
  | "ANTHROPIC_AUTH_TOKEN"
  | "ANTHROPIC_API_KEY";

export type PricingModelSource = "inherit" | "request" | "response";

export interface ProviderMeta {
  presetId: string;
  schemaVersion: number;
  websiteUrl: string;
  apiKeyUrl: string;
  notes: string;
  icon: string;
  iconColor: string;
  category: string;
  apiFormat: ProviderApiFormat;
  apiKeyField: ProviderApiKeyField;
  isFullUrl: boolean;
  modelsUrl: string;
  endpointCandidates: string[];
  endpointAutoSelect: boolean;
  customUserAgent: string;
  requestHeaders: Record<string, string>;
  requestBody: Record<string, unknown>;
  settings: Record<string, unknown>;
  costMultiplier: number | null;
  pricingModelSource: PricingModelSource;
}

export function createDefaultProviderMeta(
  patch: Partial<ProviderMeta> = {},
): ProviderMeta {
  return {
    presetId: "",
    schemaVersion: 1,
    websiteUrl: "",
    apiKeyUrl: "",
    notes: "",
    icon: "",
    iconColor: "",
    category: "custom",
    apiFormat: "anthropic",
    apiKeyField: "ANTHROPIC_AUTH_TOKEN",
    isFullUrl: false,
    modelsUrl: "",
    endpointCandidates: [],
    endpointAutoSelect: true,
    customUserAgent: "",
    requestHeaders: {},
    requestBody: {},
    settings: {},
    costMultiplier: null,
    pricingModelSource: "inherit",
    ...patch,
  };
}

export function providerMeta(provider: ProviderConfig): ProviderMeta {
  return createDefaultProviderMeta(provider.meta ?? {});
}

/** 项目 = 一个本地目录（会话按 projectId 归属分组，参考 orca 裁剪版）。 */
export interface Project {
  id: string;
  name: string;
  path: string;
  badgeColor: string;
  createdAt: number;
  updatedAt: number;
}

/** 会话工作目录；临时目录不关联项目。 */
export type SessionDirectory = Pick<Project, "name" | "path"> & { id?: string };

/** 一键启动预设：固定的 provider + 目录组合 + 启动开关。 */
export interface LaunchPreset {
  id: string;
  name: string;
  providerId: string;
  /** 每个元素对应一个会话目录（空串 = home 默认）；会话数 = 数组长度。 */
  cwds: string[];
  yolo: boolean;
  autoMode: boolean;
  useWorktree: boolean;
  initialPrompt?: string | null;
  createdAt: number;
  updatedAt: number;
}

export type AppTheme = "dark" | "light";

export type AppearanceTheme =
  | "dark"
  | "light"
  | "ocean"
  | "purple"
  | "sand"
  | "aurora";
export type AppearanceBackground = "solid" | "soft" | "texture";
export type AppearanceAccent =
  | "clay"
  | "none"
  | "purple"
  | "cyan"
  | "orange"
  | "pink"
  | "coral"
  | "gray";

export interface AppAppearance {
  theme: AppearanceTheme;
  background: AppearanceBackground;
  accent: AppearanceAccent;
  followSystem: boolean;
  terminalTransparent?: boolean;
  terminalOpacity?: number;
}

export type SessionStatus =
  | "pending"
  | "running"
  | "waiting"
  | "completed"
  | "error"
  | "stopped";

export type SessionKind = "claude" | "terminal";

export type TerminalShell = "powershell" | "cmd" | "zsh" | "bash";

export type SessionTypeFilter = "all" | "claude" | "terminal";

export type PermissionMode =
  | "default"
  | "acceptEdits"
  | "plan"
  | "auto"
  | "dontAsk"
  | "bypassPermissions";

export type ApprovalPolicy = "ask" | "allow" | "deny";

export type ApprovalDecision = "allow" | "deny";

export interface ApprovalCard {
  sessionId: string;
  ptyId: number;
  title: string;
  providerName: string;
  cwd: string;
  projectId: string | null;
  multiAgentProjectId: string | null;
  waitingSince: number;
  waitingReason: string | null;
  approvalPolicy: ApprovalPolicy;
}

/** 群聊活跃度（Agent 入站互发的三档开关）。 */
export type AgentActivity = "active" | "lurk" | "muted";

export interface AgentActivityState {
  sessionId: string;
  activity: AgentActivity;
}

export interface SessionWorktree {
  repoRoot: string;
  path: string;
  branch: string;
}

export type AutomationTrigger =
  | { kind: "once"; at: number }
  | { kind: "interval"; minutes: number }
  | { kind: "daily"; hour: number; minute: number }
  | { kind: "weekly"; weekdays: number[]; hour: number; minute: number }
  | { kind: "cron"; expr: string }
  | { kind: "on_session_event"; event: "completed" | "failed" }
  | { kind: "on_session_event_filtered"; event: "completed" | "failed"; runtimeKind: AgentRuntimeKind | null; projectId: string | null; sessionId: string | null };
// Legacy actions remain readable; new forms persist Agent identity explicitly.

export type AutomationAction =
  | { kind: "run_agent"; runtimeKind: AgentRuntimeKind; providerId: string | null; model: string | null; cwd: string; prompt: string }
  | { kind: "send_agent"; runtimeKind: AgentRuntimeKind; sessionId: string; body: string; projectId: string | null; agentId: string | null }
  | { kind: "broadcast_agent"; runtimeKind: AgentRuntimeKind; body: string }
  | { kind: "launch"; providerId: string; cwd: string | null; prompt: string; projectId?: string | null; agentId?: string | null }
  | { kind: "message"; sessionId: string; body: string; projectId?: string | null; agentId?: string | null }
  | { kind: "broadcast"; body: string };

export interface AutomationRun {
  id?: string;
  origin?: string;
  retryOf?: string | null;
  updatedAt?: number;
  targets?: AutomationTarget[];
  stage?: "dispatching" | "queued" | "submitted" | "received" | "running" | "waiting" | "stopping" | "completed" | "failed" | "cancelled" | "unconfirmed" | "acknowledged" | "skipped" | null;
  at: number;
  ok: boolean;
  detail: string;
}

export interface AutomationTarget {
  sessionId: string;
  messageId: string;
  projectId: string | null;
  taskId: string | null;
  runId: string | null;
  stage: string;
  detail: string;
}

export interface Automation {
  requestReceipts?: Record<string, string>;
  overlapPolicy?: "skip" | "allow";
  activeSessionIds?: string[];
  id: string;
  name: string;
  enabled: boolean;
  trigger: AutomationTrigger;
  action: AutomationAction;
  maxRuns: number | null;
  maxConsecutiveFailures: number;
  runCount: number;
  consecutiveFailures: number;
  lastRunAt: number | null;
  nextRunAt: number | null;
  createdAt: number;
  recentRuns: AutomationRun[];
}

export interface NewAutomation {
  overlapPolicy?: "skip" | "allow";
  name: string;
  trigger: AutomationTrigger;
  action: AutomationAction;
  maxRuns: number | null;
  maxConsecutiveFailures: number | null;
}

export interface SessionRecord {
  /** Requested native CLI startup mode; not a live permission-mode observation. */
  cliLaunchMode?: string | null;
  /** Session-scoped model override; omitted for legacy/local-default sessions. */
  launchModel?: string | null;
  modelAssignment?: { connectionId: string; model: string | null; source: string } | null;
  /** 一次性执行日志宿主不能接收普通聊天或 Agent 接力。 */
  runtimeCommandHost?: boolean;
  id: string;
  claudeSessionId: string;
  kind: SessionKind;
  shell: TerminalShell | null;
  multiAgentProjectId?: string | null;
  multiAgentAgentId?: string | null;
  retiredAgentRuntime?: boolean;
  agentAvatarId?: string | null;
  agentRuntimeKind?: AgentRuntimeKind | null;
  projectId?: string | null;
  worktree?: SessionWorktree | null;
  providerId: string;
  providerName: string;
  cwd: string;
  title: string;
  status: SessionStatus;
  ptyId: number | null;
  yolo: boolean;
  autoMode: boolean;
  permissionMode: PermissionMode | null;
  approvalPolicy?: ApprovalPolicy;
  waitingReason?: string | null;
  waitingSince?: number | null;
  titleLocked: boolean;
  exitCode: number | null;
  lastError: string | null;
  archivedAt?: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface PtyChunk {
  appSessionId: string;
  data: string;
  running: boolean;
  code: number | null;
}

/** 会话累计 token 用量（解析 claude transcript 的 message.usage）。 */
export interface SessionUsage {
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
}

export interface GitSettings {
  branchPrefix: string;
  compareStrategy: "repository_default" | "branch_upstream";
  pullStrategy: "ff_only" | "rebase";
  commitInstruction: string;
}

export interface GitFileEntry {
  path: string;
  originalPath: string | null;
  status: string;
  staged: boolean;
  unstaged: boolean;
  untracked: boolean;
  conflicted: boolean;
  additions: number | null;
  deletions: number | null;
}

export interface GitCommittedEntry {
  path: string;
  status: string;
  additions: number | null;
  deletions: number | null;
}

export interface GitStatusSnapshot {
  cwd: string;
  repoRoot: string;
  branch: string | null;
  detached: boolean;
  head: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  mergeInProgress: boolean;
  defaultBase: string | null;
  compareBase: string | null;
  entries: GitFileEntry[];
  committedEntries: GitCommittedEntry[];
}

export interface GitActionReceipt {
  message: string;
  commit: string | null;
  warning: boolean;
  status: GitStatusSnapshot;
}

export interface GitBranchOption {
  name: string;
  remote: boolean;
}

export interface PollResult {
  workingCliPtyIds?: number[];
  chunks: Record<string, PtyChunk>;
  sessions: SessionRecord[];
}

export type ControlIntent =
  | "launch"
  | "broadcast"
  | "stop_all"
  | "delete_all"
  | "help"
  | "unknown";
export type ControlLaunchMode = "default" | "auto" | "yolo";

export interface ControlLaunch {
  providerId: string;
  count: number;
  mode: ControlLaunchMode;
}

export interface ControlPlan {
  intent: ControlIntent;
  reply: string;
  launches: ControlLaunch[];
  prompt: string | null;
}

export interface ControlHistoryMessage {
  role: "user" | "assistant";
  content: string;
}

export interface BroadcastTarget {
  sessionId: string;
  title: string;
  providerName: string;
}

export interface BroadcastReceipt {
  targetCount: number;
  targets: BroadcastTarget[];
}

export type DeliveryKind = "chat" | "broadcast" | "system" | "initial_prompt";

/** 即时通道的一条成功投递记录（协作工作台「协作记录」只读投影用，内存态重启清空）。 */
export interface DeliveryRecord {
  id: string;
  at: number;
  fromSessionId: string | null;
  fromLabel: string | null;
  toSessionId: string;
  body: string;
  kind: DeliveryKind;
  stage?: string;
  error?: string | null;
}

export interface PermissionModeReceipt {
  targetMode: PermissionMode;
  targetCount: number;
  alreadyCount: number;
  sentCount: number;
  awaitingConfirmationCount: number;
  queuedCount: number;
}

export interface ControlWorkspaceTrustStatus {
  batchCount: number;
  awaitingCount: number;
  startingCount: number;
  missingCount: number;
}

export interface ControlWorkspaceTrustReceipt {
  confirmedCount: number;
  skippedCount: number;
  failedSessionIds: string[];
}

export interface AcpCommand {
  name: string;
  description: string;
  inputHint: string | null;
}

export interface AcpStatus {
  connected: boolean;
  sessionId: string;
  cwd: string;
  agentName: string;
  agentVersion: string;
  fullPermissions: boolean;
  resumed: boolean;
  commands: AcpCommand[];
  configOptions: unknown[];
}

export interface HarnessChatMessage {
  id: string;
  role: "user" | "assistant";
  text: string;
  reasoning?: string;
  streamStatus?: "streaming" | "completed" | "failed" | "interrupted" | "abandoned";
  createdAt: number;
  attachments?: { path: string; uri: string; name: string }[];
}

export interface HarnessSessionSnapshot {
  codexGoal?: { objective: string; status: "active" | "paused" | "blocked" | "complete" | "limited"; tokenBudget?: number | null; tokensUsed: number; timeUsedSeconds: number; createdAt: number; updatedAt: number } | null;
  codexGoalSupported?: boolean;
  codexGoalAction?: string | null;
  contextUsage?: { used: number; size: number } | null;
  sessionId: string;
  connected: boolean;
  busy: boolean;
  history: HarnessChatMessage[];
  configOptions: unknown[];
  commands?: { name: string; description?: string; inputHint?: string }[];
  pendingPermission: unknown | null;
  toolCalls: unknown[];
  sequence: number;
  controlState?: {
    skills?: { name: string; description: string; invocation: { userInvocable: boolean; modelInvocable: boolean } }[];
    commands?: { name: string; description: string; input?: { hint?: string } }[];
    permission?: { currentValue: string; options: { value: string; name: string; description?: string }[] };
    goal?: { objective: string; phase: string; activation?: string; roundsStarted?: number; maxGoalRounds?: number } | null;
    agentStatus?: string;
    execution?: { commandId: string; result: { kind: "success" | "error"; text?: string } } | null;
  };
}

export interface AcpPromptResult {
  text: string;
  commands: AcpCommand[];
  cancelled: boolean;
}

export type AgentRuntimeKind =
  | "claude_code"
  | "codex"
  | "kimi_code"
  | "minimax_code" // Historical persistence sentinel; never offered as an active runtime.
  | "pi"
  | "oh_my_pi"
  | "unsupported"
  | "opencode"
  | "deepseek_harness"
  | "claude_chat"
  | "codex_chat"
  | "kimi_chat"
  | "opencode_chat"
  | "pi_chat"
  | "zcode"
  | "zcode_cli";

export type AgentRole = "orchestrator" | "worker" | "reviewer" | "integrator";
export type PermissionProfile = "default" | "auto" | "yolo";
/** 成员人格预设：只约束沟通风格与工作倾向；custom 时用 personaCustom 注入派单提示词。 */
export type AgentPersona = "efficient" | "concise" | "rigorous" | "innovative" | "custom";
export const AGENT_PERSONA_LABELS: Record<AgentPersona, string> = {
  efficient: "务实高效",
  concise: "简洁直接",
  rigorous: "严谨细致",
  innovative: "创新探索",
  custom: "自定义",
};
export const AGENT_PERSONA_DESCRIPTIONS: Record<AgentPersona, string> = {
  efficient: "直接推进任务，优先解决阻塞，避免过度设计与无关扩展",
  concise: "输出精炼，只保留必要信息，不做冗余铺垫",
  rigorous: "逐步验证假设，交叉核对关键结论，明确列出不确定性与风险",
  innovative: "在满足验收标准的前提下主动提出更优方案，并说明取舍",
  custom: "用一句自由文本描述该成员的工作风格（最多 120 字）",
};
export type MultiAgentProjectStatus =
  | "draft"
  | "active"
  | "paused"
  | "completed"
  | "cancelled";
export type MultiAgentTaskStatus =
  | "draft"
  | "ready"
  | "blocked"
  | "claimed"
  | "running"
  | "waiting_approval"
  | "in_review"
  | "changes_requested"
  | "done"
  | "failed"
  | "cancelled";
export type AgentRunStatus =
  | "queued"
  | "starting"
  | "running"
  | "waiting_input"
  | "waiting_approval"
  | "succeeded"
  | "failed"
  | "timed_out"
  | "cancelled"
  | "interrupted";
export type WorkspaceMode = "read_only" | "worktree" | "shared_write";

export interface RuntimeProbe {
  kind: AgentRuntimeKind;
  installed: boolean;
  executable: string | null;
  version: string | null;
  protocol: string;
  capabilities: string[];
  error: string | null;
  latestVersion?: string | null;
  updateAvailable?: boolean;
  updateStatus?: "unchecked" | "up_to_date" | "available" | "error";
  updateCommand?: string;
  canAutoUpdate?: boolean;
  updateError?: string | null;
}

export interface RuntimeUpgradeReceipt {
  kind: AgentRuntimeKind;
  output: string;
  runtime: RuntimeProbe;
}

export interface MultiAgentProject {
  id: string;
  name: string;
  goal: string;
  rootPath: string;
  defaultBranch: string;
  status: MultiAgentProjectStatus;
  createdAt: number;
  updatedAt: number;
}

export interface AgentProfile {
  avatarId?: string | null;
  id: string;
  projectId: string;
  name: string;
  runtimeKind: AgentRuntimeKind;
  role: AgentRole;
  description: string;
  persona: AgentPersona;
  personaCustom: string;
  model: string | null;
  permissionProfile: PermissionProfile;
  maxConcurrency: number;
  capabilities: string[];
  enabled: boolean;
  removedAt?: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface MultiAgentTask {
  id: string;
  projectId: string;
  parentId: string | null;
  stage: number;
  title: string;
  description: string;
  acceptanceCriteria: string;
  verificationCommand: string | null;
  status: MultiAgentTaskStatus;
  assigneeAgentId: string | null;
  reviewerAgentId: string | null;
  workspaceMode: WorkspaceMode;
  priority: number;
  requiredCapabilities: string[];
  maxAttempts: number;
  retryBackoffSeconds: number;
  nextRetryAt: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface AgentRun {
  id: string;
  projectId: string;
  taskId: string;
  agentId: string;
  attempt: number;
  status: AgentRunStatus;
  sessionLocator: Record<string, unknown> | null;
  leaseExpiresAt: number;
  lastHeartbeatAt: number | null;
  outputSummary: string | null;
  error: string | null;
  createdAt: number;
  startedAt: number | null;
  finishedAt: number | null;
}

export interface WakeupRequest {
  id: string;
  projectId: string;
  agentId: string;
  taskId: string | null;
  runId: string | null;
  reason: string;
  status: string;
  idempotencyKey: string;
  coalescedCount: number;
  requestedAt: number;
}

export interface MultiAgentActivityEvent {
  seq: number;
  projectId: string;
  entityType: string;
  entityId: string;
  eventType: string;
  actorType: string;
  actorId: string | null;
  payload: Record<string, unknown>;
  createdAt: number;
}

export interface TaskDependencyRecord {
  taskId: string;
  dependsOnTaskId: string;
}

export type AgentSessionStatus =
  | "idle"
  | "starting"
  | "running"
  | "waiting"
  | "completed"
  | "failed"
  | "stopped";
export type AgentControlMode = "auto" | "manual";

export interface AgentSessionRecord {
  id: string;
  projectId: string;
  agentId: string;
  currentTaskId: string | null;
  currentRunId: string | null;
  appSessionId: string | null;
  status: AgentSessionStatus;
  controlMode: AgentControlMode;
  lastOutputAt: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface RunLogEntry {
  seq: number;
  projectId: string;
  runId: string;
  taskId: string;
  agentId: string;
  stream: string;
  content: string;
  createdAt: number;
}

export interface AgentInstruction {
  id: string;
  projectId: string;
  agentId: string;
  taskId: string | null;
  body: string;
  status: string;
  createdAt: number;
  deliveredAt: number | null;
}

export interface RuntimeEventRecord {
  seq: number;
  projectId: string;
  runId: string;
  taskId: string;
  agentId: string;
  eventType: string;
  payload: Record<string, unknown>;
  createdAt: number;
}

export interface MemoryEntry {
  id: string;
  projectId: string;
  namespace: string;
  key: string;
  value: string;
  sourceTaskId: string | null;
  sourceRunId: string | null;
  confidence: number;
  verified: boolean;
  createdAt: number;
  updatedAt: number;
}

export type KnowledgeSourceType =
  | "project"
  | "task"
  | "conversation"
  | "run"
  | "artifact"
  | "review"
  | "memory";

export interface KnowledgeSearchResult {
  id: string;
  projectId: string;
  sourceType: KnowledgeSourceType;
  sourceId: string;
  taskId: string | null;
  runId: string | null;
  title: string;
  snippet: string;
  content: string;
  sourceUri: string;
  relevance: number;
  createdAt: number;
  updatedAt: number;
}

export interface KnowledgeIndexReceipt {
  projectId: string;
  documentCount: number;
  sourceCounts: Partial<Record<KnowledgeSourceType, number>>;
  indexedAt: number;
}

export type ReviewVerdict = "approved" | "changes_requested";

export interface ReviewDecision {
  id: string;
  projectId: string;
  reviewTaskId: string;
  reviewRunId: string;
  reviewerAgentId: string;
  targetTaskId: string;
  verdict: ReviewVerdict;
  summary: string;
  createdAt: number;
  appliedAt: number | null;
}

export interface AgentControlReceipt {
  projectId: string;
  agentId: string;
  controlMode: AgentControlMode;
  queuedInstruction: boolean;
  message: string;
}

export interface MultiAgentSnapshot {
  project: MultiAgentProject;
  agents: AgentProfile[];
  tasks: MultiAgentTask[];
  runs: AgentRun[];
  wakeups: WakeupRequest[];
  events: MultiAgentActivityEvent[];
  messages: CollaborationMessage[];
  artifacts: ArtifactRecord[];
  approvals: ApprovalRecord[];
  dependencies: TaskDependencyRecord[];
  agentSessions: AgentSessionRecord[];
  runLogs: RunLogEntry[];
  instructions: AgentInstruction[];
  runtimeEvents: RuntimeEventRecord[];
  memories: MemoryEntry[];
  reviewDecisions: ReviewDecision[];
}

export type CoordinatorIntent =
  | "plan_project"
  | "add_tasks"
  | "start_project"
  | "pause_project"
  | "resume_project"
  | "cancel_project"
  | "inspect_project"
  | "retry_task"
  | "reassign_task"
  | "approve_task"
  | "unknown";

export interface CoordinatorHistoryMessage {
  role: "user" | "assistant";
  content: string;
  surface?: "quick" | "multi_agent";
}

export interface PlannedAgent {
  key: string;
  name: string;
  runtimeKind: AgentRuntimeKind;
  role: AgentRole;
  description: string;
  model: string | null;
  permissionProfile: PermissionProfile;
  existingAgentId?: string | null;
}

export interface PlannedTask {
  key: string;
  stage: number;
  title: string;
  description: string;
  acceptanceCriteria: string;
  verificationCommand: string | null;
  assigneeKey: string;
  reviewerKey: string | null;
  dependsOn: string[];
  workspaceMode: WorkspaceMode;
  priority: number;
  requiredCapabilities: string[];
  maxAttempts: number;
  retryBackoffSeconds: number;
}

export interface CoordinatorPlan {
  intent: CoordinatorIntent;
  reply: string;
  projectName: string | null;
  goal: string | null;
  rootPath: string | null;
  targetTaskId: string | null;
  targetAgentId: string | null;
  agents: PlannedAgent[];
  tasks: PlannedTask[];
  requestedRuntimes: AgentRuntimeKind[];
  requiresConfirmation: boolean;
}

export interface ApplyCoordinatorPlanReceipt {
  projectId: string;
  createdAgents: number;
  createdTasks: number;
}

export interface ProjectExecutionReceipt {
  projectId: string;
  accepted: boolean;
  message: string;
}

export interface CollaborationMessage {
  id: string;
  projectId: string;
  taskId: string | null;
  senderType: string;
  senderId: string | null;
  targetAgentId: string | null;
  body: string;
  createdAt: number;
}

/** A project owns one public team conversation. Private session history is never imported. */
export type ConversationMode = "discussion" | "execution_plan";
export type ConversationIntent = "chat" | "exchange";
/** The three legacy phases remain readable in existing conversation history. */
export type ConversationPhase = "chat" | "exchange" | "plan" | "opinions" | "responses" | "summary";
export type ConversationStatus = "queued" | "running" | "waiting_approval" | "stopping" | "completed" | "failed" | "cancelled" | "paused" | "unconfirmed" | "needs_decision";
export interface CollaborationConversation {
  id: string;
  projectId: string;
  createdAt: number;
}
export interface ConversationHistoryEntry {
  id: string;
  createdAt: number;
  lastMessageAt: number | null;
  preview: string | null;
  current: boolean;
  legacy: boolean;
}
export interface ConversationAttachment {
  id: string;
  name: string;
  bytes: number;
  mediaType: string;
}

export interface ConversationMessage {
  id: string;
  /** Present on human messages; server id is separate and stable across retries. */
  clientMessageId?: string | null;
  projectId: string;
  conversationId: string;
  roundId: string | null;
  replyToMessageId: string | null;
  taskId: string | null;
  runId: string | null;
  agentId: string | null;
  senderType: "user" | "agent" | "system";
  kind: "message" | "reply" | "status" | "plan";
  phase: ConversationPhase | null;
  body: string;
  attachments?: ConversationAttachment[];
  status: ConversationStatus;
  createdAt: number;
  updatedAt: number;
  /** Durable interjection. queued waits for stop; needs_decision requires explicit retry. */
  queuedInput?: ConversationSendInput | null;
}

export interface ConversationOriginalSession {
  projectId: string;
  runId: string;
  agentName: string;
  request: string;
  reply: string;
  error: string | null;
}

export interface ConversationDesignDelegation {
  id: string;
  sourceAgentId: string;
  targetAgentId: string;
  sourceRunId: string;
  deliveryRunId: string | null;
  resumeRunId: string | null;
  title: string;
  brief: string;
  checkpointAt: number | null;
  submittedAt: number | null;
  acceptedAt: number | null;
  resumedAt: number | null;
  filename: string | null;
  status: "checkpointing" | "waiting_design" | "accepted" | "resumed" | "failed";
  failure: string | null;
}

export interface ConversationDesignDelivery {
  filename: string;
  markdown: string;
  acceptanceEvidence: Array<{ criterion: string; evidence: string; met: boolean }>;
}
export type ConversationChatDispatch =
  | { kind: "parallel" }
  | { kind: "assigned"; agentId: string };
export interface ConversationRound {
  id: string;
  projectId: string;
  conversationId: string;
  userMessageId: string;
  participantAgentIds: string[];
  chatDispatch?: ConversationChatDispatch;
  mode: ConversationMode;
  intent?: ConversationIntent;
  status: ConversationStatus;
  phase: ConversationPhase;
  maxRounds: number;
  currentRound: number;
  deadlineAt: number;
  stopReason: string | null;
  managedTarget?: { root: string; branch: string };
  createdAt: number;
  updatedAt: number;
}
export interface ConversationPlannedTask {
  key: string;
  title: string;
  description: string;
  acceptanceCriteria: string;
  verificationCommand: string | null;
  assigneeAgentId: string;
  reviewerAgentId: string | null;
  workspaceMode: "read_only" | "worktree";
  writePaths: string[];
  dependsOn: string[];
}
export interface ProjectPreviewConfig {
  program: string;
  args: string[];
  workingDirectory: string;
  url: string | null;
}

export type DeliveryDemo = { state: "preparing" | "ready" } | { state: "unavailable"; error: string };

export interface ConversationExecutionPlan {
  id: string;
  projectId: string;
  roundId: string;
  messageId: string;
  summary: string;
  tasks: ConversationPlannedTask[];
  status: "pending" | "applied" | "superseded";
  taskIds: string[];
  managed?: {
    target: { root: string; branch: string };
    status: "planning" | "running" | "completed" | "failed";
    summary: string | null;
    commit: string | null;
    verificationCommand: string | null;
    preview?: ProjectPreviewConfig;
    demo?: DeliveryDemo;
  };
  createdAt: number;
}
export interface ConversationHardBoundary {
  tools: "research" | "none";
  network: "public_web" | "none";
  deniedReadPaths: string[];
}
export interface ConversationTaskRules {
  tools: "research" | "none";
  instructions?: string;
  archivedRuleId?: string;
}
export interface ConversationPolicyEntry {
  projectId: string;
  agentId: string | null;
  boundary: ConversationHardBoundary;
  legacyReviewRequired: boolean;
  legacyText: string;
}
export interface ConversationRuntimeCapability {
  runtimeKind: AgentRuntimeKind;
  research: boolean;
  replyOnly: boolean;
  networkControl: boolean;
  pathControl: boolean;
  status: "native" | "adapted" | "unsupported";
  detail: string;
}
export interface ConversationArchivedRule {
  id: string;
  agentId: string | null;
  label: string;
  instructions: string;
  tools: "research" | "none";
  createdAt: number;
}
export interface ConversationMigrationPreview {
  id: string;
  projectId: string;
  agentId: string | null;
  previousText: string;
  replacementText: string;
  archivedRules: string;
  createdAt: number;
}
export interface ConversationSnapshot {
  delegations?: ConversationDesignDelegation[];
  /** Safe per-run metadata; never raw reasoning or tool results. */
  activityEvents?: RuntimeEventRecord[];
  conversation: CollaborationConversation;
  messages: ConversationMessage[];
  rounds: ConversationRound[];
  executionPlans: ConversationExecutionPlan[];
  supportedDiscussionRuntimes: AgentRuntimeKind[];
  projectPolicy?: ConversationPolicyEntry;
  memberPolicies?: ConversationPolicyEntry[];
  runtimeCapabilities?: ConversationRuntimeCapability[];
  archivedTaskRules?: ConversationArchivedRule[];
}
export interface ConversationSendInput {
  nativeCapabilities?: boolean;
  managed?: boolean;
  projectId: string;
  expectedConversationId?: string;
  clientMessageId: string;
  body: string;
  attachmentIds?: string[];
  participantAgentIds: string[];
  chatDispatch?: ConversationChatDispatch;
  mode: ConversationMode;
  intent?: ConversationIntent;
  taskRules?: ConversationTaskRules;
  /** Legacy compatible bound for directed exchange; ordinary chat never repeats members. */
  maxRounds?: number;
  /** Whole discussion deadline, including queue and approvals; defaults to 600. */
  maxDurationSeconds?: number;
}
export interface ConversationInterruptInput {
  nativeCapabilities?: boolean;
  projectId: string;
  clientMessageId: string;
  attachmentIds?: string[];
  body: string;
  /** Omit only for legacy pause-only interjections. New clients preserve the full request. */
  participantAgentIds?: string[];
  chatDispatch?: ConversationChatDispatch;
  mode?: ConversationMode;
  intent?: ConversationIntent;
  taskRules?: ConversationTaskRules;
  maxRounds?: number;
  maxDurationSeconds?: number;
}
export interface ConversationExecutePlanInput {
  projectId: string;
  planId: string;
  clientRequestId: string;
}

export interface ArtifactRecord {
  id: string;
  projectId: string;
  taskId: string;
  runId: string;
  artifactType: string;
  uri: string;
  summary: string;
  contentHash: string | null;
  createdAt: number;
}

export interface ApprovalRecord {
  id: string;
  projectId: string;
  taskId: string | null;
  runId: string | null;
  approvalType: string;
  summary: string;
  status: string;
  createdAt: number;
  decidedAt: number | null;
}

export type InterventionKind =
  | "approval_required"
  | "agent_input_required"
  | "retry_exhausted"
  | "dependency_blocked"
  | "semantic_review";
export type InterventionSeverity = "critical" | "high" | "medium" | "low";
export type InterventionStatus = "open" | "snoozed" | "resolved";
export type InterventionRecommendedAction =
  | "approve"
  | "respond"
  | "retry"
  | "open_project"
  | "review";
export type InterventionAction =
  | "approve"
  | "respond"
  | "retry"
  | "reassign"
  | "snooze"
  | "dismiss";

export interface InterventionRecord {
  id: string;
  projectId: string;
  projectName: string;
  taskId: string | null;
  runId: string | null;
  kind: InterventionKind;
  severity: InterventionSeverity;
  title: string;
  summary: string;
  status: InterventionStatus;
  recommendedAction: InterventionRecommendedAction;
  fingerprint: string;
  snoozedUntil: number | null;
  metadata: Record<string, unknown>;
  createdAt: number;
  updatedAt: number;
  resolvedAt: number | null;
}

export interface InterventionResolutionReceipt {
  interventionId: string;
  projectId: string;
  resumeProject: boolean;
  message: string;
}
