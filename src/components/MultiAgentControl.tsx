import { UnverifiedRunNotice } from "./UnverifiedRunNotice";
import { GroupChatPanel } from "./GroupChatPanel";
import { createTeamMessagePreview } from "../teamMessagesPreview";
import type { ControlSurface } from "../controlSurface";
import { useRuntimeAvailability } from "../runtimeAvailability";
import { FormEvent, useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { ArrowSquareOutIcon } from "@phosphor-icons/react/dist/csr/ArrowSquareOut";
import { BroadcastIcon } from "@phosphor-icons/react/dist/csr/Broadcast";
import { CaretDownIcon } from "@phosphor-icons/react/dist/csr/CaretDown";
import { CheckIcon } from "@phosphor-icons/react/dist/csr/Check";
import { CopyIcon } from "@phosphor-icons/react/dist/csr/Copy";
import { PaperPlaneRightIcon } from "@phosphor-icons/react/dist/csr/PaperPlaneRight";
import { PlusIcon } from "@phosphor-icons/react/dist/csr/Plus";
import { PushPinIcon } from "@phosphor-icons/react/dist/csr/PushPin";
import { RobotIcon } from "@phosphor-icons/react/dist/csr/Robot";
import { SparkleIcon } from "@phosphor-icons/react/dist/csr/Sparkle";
import { TrashIcon } from "@phosphor-icons/react/dist/csr/Trash";
import { UsersThreeIcon } from "@phosphor-icons/react/dist/csr/UsersThree";
import { AgentAvatar } from "./AgentAvatar";
import { AgentMentionMenu, type AgentMentionOption as MentionOption } from "./AgentMentionMenu";
import { autoApprovalTaskIds, canAutoContinueProject, mergeFailedCollaborationMessages } from "../collaborationAutomation";
import {
  optimizationIsUseful,
  validatePromptDraft,
  PROMPT_OPTIMIZATION_PREVIEW,
  type PromptOptimization,
} from "../promptOptimization";
import {
  decodeMultiAgentDraft,
  encodeMultiAgentDraft,
  getRuntimeHealth,
  isUsablePlanText,
  planIsLowRiskCommunication,
  sanitizeGeneratedPlan,
  type MentionBinding,
  type PendingPlanContext,
} from "../multiAgentDraftState";
import {
  hasMentionToken,
  toggleMentionToken,
} from "../multiAgentMention";
import {
  buildTaskProgressSummary,
  buildTaskStatusPresentations,
  shouldExpandTaskProgress,
} from "../multiAgentTaskPresentation";
import type {
  AgentProfile,
  AgentRole,
  AgentRuntimeKind,
  ApplyCoordinatorPlanReceipt,
  CoordinatorHistoryMessage,
  CoordinatorPlan,
  MultiAgentProject,
  MultiAgentSnapshot,
  ProjectExecutionReceipt,
  PermissionMode,
  RuntimeProbe,
  SessionRecord,
  WorkspaceMode,
} from "../types";
import {
  resolveUnifiedControlRoute,
  stripUnifiedControlPrefix,
} from "../unifiedControlRouter";

import type { ControlCommandResult } from "../controlCommandResult";

interface Props {
  sessions?: SessionRecord[];
  active?: boolean;
  defaultCwd: string | null;
  onOpenWorkspace?: (projectId: string) => void;
  focusedProjectId?: string | null;
  focusedProjectRequest?: number;
  onOpenQuickCommands?: () => void;
  onQuickCommand?: (
    command: string,
  ) => Promise<ControlCommandResult>;
  onQuickBroadcast?: (
    command: string,
  ) => Promise<ControlCommandResult>;
  onQuickPermissionModeChange?: (
    mode: PermissionMode,
  ) => Promise<ControlCommandResult>;
  onClearQuickHistory?: () => Promise<ControlCommandResult>;
  quickLiveSessionCount?: number;
  quickKnownModeCount?: number;
  quickPermissionMode?: PermissionMode | "mixed";
  quickModesKnown?: boolean;
  quickModesSwitchable?: boolean;
  quickPermissionModeBusy?: boolean;
  workspaceTrustStatus?: {
    awaitingCount: number;
    startingCount: number;
  };
  workspaceTrustBusy?: boolean;
  onConfirmWorkspaceTrust?: () => void | Promise<void>;
}

const COORDINATOR_DRAFT_STORAGE_KEY = "crewvela-multi-agent-draft-v1";
const AUTO_REPORT_STORAGE_KEY = "crewvela-multi-agent-auto-report-v1";
const MAX_COORDINATOR_HISTORY = 200;
const COORDINATOR_REPLY_STREAM_EVENT = "multi-agent-coordinator-reply-stream";
const QUICK_PROMPT_COMPLETION = "打开 1 个 GLM 和 2 个 Kimi 会话";
const QUICK_PERMISSION_MODES: Array<{ value: PermissionMode; label: string }> = [
  { value: "default", label: "Default" },
  { value: "acceptEdits", label: "Accept Edits" },
  { value: "plan", label: "Plan" },
  { value: "auto", label: "Auto" },
];
const UI_PREVIEW =
  new URLSearchParams(window.location.search).get("ui-preview") === "1";
const MULTI_AGENT_UI_PREVIEW_SNAPSHOT: MultiAgentSnapshot = {
  project: {
    id: "ui-preview-project",
    name: "账户登录模块",
    goal: "由 Claude 实现功能，Codex 完成测试与审核。",
    rootPath: "C:\\workspace\\crewvela",
    defaultBranch: "main",
    status: "completed",
    createdAt: 0,
    updatedAt: 0,
  },
  agents: [
    {
      id: "ui-preview-claude",
      projectId: "ui-preview-project",
      name: "Claude",
      runtimeKind: "claude_code",
      role: "worker",
      description: "负责功能实现",
      persona: "efficient",
      personaCustom: "",
      model: null,
      permissionProfile: "default",
      maxConcurrency: 1,
      capabilities: ["implementation"],
      enabled: true,
      createdAt: 0,
      updatedAt: 0,
    },
    {
      id: "ui-preview-codex",
      projectId: "ui-preview-project",
      name: "Codex",
      runtimeKind: "codex",
      role: "reviewer",
      description: "负责测试与审核",
      persona: "rigorous",
      personaCustom: "",
      model: null,
      permissionProfile: "default",
      maxConcurrency: 1,
      capabilities: ["testing", "review"],
      enabled: true,
      createdAt: 0,
      updatedAt: 0,
    },
  ],
  tasks: [
    {
      id: "ui-preview-task-implementation",
      projectId: "ui-preview-project",
      parentId: null,
      stage: 0,
      title: "实现 PI 写阅读与 账户登录模块",
      description: "完成前端与交互逻辑",
      acceptanceCriteria: "功能可用并通过本地验证",
      verificationCommand: null,
      status: "done",
      assigneeAgentId: "ui-preview-claude",
      reviewerAgentId: "ui-preview-codex",
      workspaceMode: "worktree",
      priority: 10,
      requiredCapabilities: ["implementation"],
      maxAttempts: 2,
      retryBackoffSeconds: 5,
      nextRetryAt: null,
      createdAt: 0,
      updatedAt: 0,
    },
    {
      id: "ui-preview-task-review",
      projectId: "ui-preview-project",
      parentId: null,
      stage: 1,
      title: "完成测试与代码审核",
      description: "运行测试并确认实现满足验收标准",
      acceptanceCriteria: "测试通过且审核无阻塞问题",
      verificationCommand: "npm test",
      status: "done",
      assigneeAgentId: "ui-preview-codex",
      reviewerAgentId: null,
      workspaceMode: "worktree",
      priority: 9,
      requiredCapabilities: ["testing", "review"],
      maxAttempts: 2,
      retryBackoffSeconds: 5,
      nextRetryAt: null,
      createdAt: 0,
      updatedAt: 0,
    },
  ],
  runs: [],
  wakeups: [],
  events: [],
  messages: [
    {
      id: "ui-preview-message-user",
      projectId: "ui-preview-project",
      taskId: null,
      senderType: "user",
      senderId: null,
      targetAgentId: null,
      body: "请 Claude 和 Codex 合作实现阅读功能与账户登录模块。",
      createdAt: 0,
    },
    {
      id: "ui-preview-message-assistant",
      projectId: "ui-preview-project",
      taskId: null,
      senderType: "coordinator",
      senderId: null,
      targetAgentId: null,
      body: "好的，我会让 Claude 负责功能实现，Codex 完成测试与审核。\n\n我会持续跟进进度，并在这里同步结果。",
      createdAt: 1,
    },
  ],
  artifacts: [],
  approvals: [],
  dependencies: [],
  agentSessions: [],
  runLogs: [],
  instructions: [],
  runtimeEvents: [],
  memories: [],
  reviewDecisions: [],
};

const TEAM_MESSAGE_PREVIEW = createTeamMessagePreview(MULTI_AGENT_UI_PREVIEW_SNAPSHOT);

interface CoordinatorReplyStreamEvent {
  requestId: string;
  text: string | null;
  reset: boolean;
  done: boolean;
}

type CoordinatorSendResult = ControlCommandResult;

interface AutoReportState {
  projectIds: string[];
  approvalRevisions: Record<string, number>;
  failureRevisions: Record<string, number>;
}

const RUNTIME_LABELS: Record<AgentRuntimeKind, string> = {
  claude_code: "Claude Code",
  codex: "Codex",
  kimi_code: "Kimi Code",
  minimax_code: "已停用的 CLI",
  pi: "Pi",
  oh_my_pi: "Oh My Pi（已移除）",
  unsupported: "已停用的运行时",
  opencode: "OpenCode",
  deepseek_harness: "DeepSeek Harness",
  claude_chat: "Claude Code Chat",
  codex_chat: "Codex Chat",
  kimi_chat: "Kimi Code Chat",
  opencode_chat: "OpenCode Chat",
  pi_chat: "Pi Chat",
  zcode: "ZCode Desktop",
  zcode_cli: "ZCode CLI",
};

interface DirectCollaborationReceipt {
  targetSessionIds: string[];
  targetTitles: string[];
  deliveryTaskIds?: string[];
  reply: string;
}

type MentionHighlightKind = "group" | "agent" | "invalid";

interface MentionHighlightToken {
  token: string;
  kind: MentionHighlightKind;
}

interface DraftHighlightSegment {
  text: string;
  kind: MentionHighlightKind | null;
}

const GROUP_MENTION_OPTIONS: Array<{
  runtimeKind: AgentRuntimeKind;
  token: string;
  description: string;
}> = [
  {
    runtimeKind: "claude_code",
    token: "@ClaudeCode",
    description: "规划、实现与整合",
  },
  {
    runtimeKind: "codex",
    token: "@Codex",
    description: "编码、测试与 Review",
  },
  {
    runtimeKind: "kimi_code",
    token: "@KimiCode",
    description: "Kimi Code 编码、分析与长上下文任务",
  },
  {
    runtimeKind: "pi",
    token: "@Pi",
    description: "测试、验证与问题复现",
  },
  {
    runtimeKind: "opencode",
    token: "@OpenCode",
    description: "并行实现与独立模块开发",
  },
  { runtimeKind: "zcode", token: "@ZCode", description: "ZCode Desktop 原生 Agent 编码与验证" },
  { runtimeKind: "zcode_cli", token: "@ZCodeCLI", description: "ZCode CLI 终端 Agent 编码与验证" },
  { runtimeKind: "deepseek_harness", token: "@DSH", description: "DeepSeek Harness 原生 ACP Agent 编码与验证" },
];

const ROLE_LABELS: Record<AgentRole, string> = {
  orchestrator: "总协调",
  worker: "执行",
  reviewer: "审查",
  integrator: "集成",
};

const ROLE_PRIORITY: Record<AgentRole, number> = {
  orchestrator: 0,
  worker: 1,
  integrator: 2,
  reviewer: 3,
};

function preferredAgentForRuntime(
  snapshot: MultiAgentSnapshot,
  runtimeKind: AgentRuntimeKind,
): AgentProfile | undefined {
  return snapshot.agents
    .filter((agent) => agent.enabled && agent.runtimeKind === runtimeKind)
    .sort((left, right) =>
      ROLE_PRIORITY[left.role] - ROLE_PRIORITY[right.role]
      || left.createdAt - right.createdAt
      || left.id.localeCompare(right.id)
    )[0];
}

const AGENT_SESSION_LABELS: Record<
  MultiAgentSnapshot["agentSessions"][number]["status"],
  string
> = {
  idle: "空闲",
  starting: "启动中",
  running: "执行中",
  waiting: "等待中",
  completed: "已完成",
  failed: "失败",
  stopped: "已停止",
};

interface ActiveMention {
  start: number;
  end: number;
  query: string;
}

function activeMentionAt(value: string, cursor: number): ActiveMention | null {
  const prefix = value.slice(0, cursor);
  const start = prefix.lastIndexOf("@");
  if (start < 0) return null;
  const previous = prefix[start - 1];
  if (previous && /[\p{L}\p{N}_.-]/u.test(previous)) return null;
  const query = prefix.slice(start + 1);
  if (/\s|@/.test(query)) return null;
  return { start, end: cursor, query: query.toLocaleLowerCase() };
}

function compactAgentHandle(name: string, fallback: string): string {
  const handle = [...name.normalize("NFKC")]
    .filter((character) => /[\p{L}\p{N}_-]/u.test(character))
    .join("")
    .slice(0, 24);
  return handle || fallback;
}

function splitDraftByMentionHighlights(
  value: string,
  tokens: MentionHighlightToken[],
): DraftHighlightSegment[] {
  const ranges = tokens.flatMap((item) => {
    const expression = new RegExp(
      `(^|[^\\p{L}\\p{N}_.-])(${item.token.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})(?=$|[^\\p{L}\\p{N}_-])`,
      "giu",
    );
    const matches: Array<{ start: number; end: number; kind: MentionHighlightKind }> = [];
    for (const match of value.matchAll(expression)) {
      const start = (match.index ?? 0) + match[1].length;
      matches.push({ start, end: start + match[2].length, kind: item.kind });
    }
    return matches;
  });
  ranges.sort((left, right) => left.start - right.start || right.end - left.end);

  const segments: DraftHighlightSegment[] = [];
  let cursor = 0;
  for (const range of ranges) {
    if (range.start < cursor) continue;
    if (range.start > cursor) {
      segments.push({ text: value.slice(cursor, range.start), kind: null });
    }
    segments.push({ text: value.slice(range.start, range.end), kind: range.kind });
    cursor = range.end;
  }
  if (cursor < value.length) {
    segments.push({ text: value.slice(cursor), kind: null });
  }
  return segments;
}

function buildAgentMentionOptions(snapshot: MultiAgentSnapshot | null): MentionOption[] {
  if (!snapshot) return [];
  const reserved = new Set(
    [
      "All",
      "Claude",
      "Claude-Code",
      "Claude_Code",
      ...GROUP_MENTION_OPTIONS.map((option) => option.token.slice(1)),
    ]
      .map((handle) => handle.toLocaleLowerCase()),
  );
  const used = new Set<string>();
  return [...snapshot.agents]
    .filter((agent) => agent.enabled)
    .sort((left, right) => left.createdAt - right.createdAt || left.id.localeCompare(right.id))
    .map((agent) => {
      const fallback = `${RUNTIME_LABELS[agent.runtimeKind].replace(/\s/g, "")}-${agent.id.slice(0, 4)}`;
      const base = compactAgentHandle(agent.name, fallback);
      let handle = base;
      let suffix = 2;
      while (reserved.has(handle.toLocaleLowerCase()) || used.has(handle.toLocaleLowerCase())) {
        handle = `${base}-${suffix}`;
        suffix += 1;
      }
      used.add(handle.toLocaleLowerCase());
      const session = snapshot.agentSessions.find((item) => item.agentId === agent.id);
      const task = session?.currentTaskId
        ? snapshot.tasks.find((item) => item.id === session.currentTaskId)
        : snapshot.tasks.find(
            (item) =>
              item.assigneeAgentId === agent.id &&
              ["claimed", "running", "waiting_approval", "in_review"].includes(item.status),
          );
      const status = session ? AGENT_SESSION_LABELS[session.status] : "未启动";
      return {
        key: `agent:${agent.id}`,
        kind: "agent" as const,
        token: `@${handle}`,
        description: [RUNTIME_LABELS[agent.runtimeKind], ROLE_LABELS[agent.role], status, task?.title]
          .filter(Boolean)
          .join(" · "),
        section: "具体 Agent" as const,
        runtimeKind: agent.runtimeKind,
        avatarId: agent.avatarId,
        agentId: agent.id,
        targetCount: 1,
      };
    });
}

function isMentionBindingAvailable(
  snapshot: MultiAgentSnapshot | null,
  binding: MentionBinding,
): boolean {
  return (
    snapshot === null ||
    snapshot.agents.some(
      (agent) => agent.id === binding.agentId && agent.enabled,
    )
  );
}

/// 网格终端会话作为 @ 目标：token 与项目 Agent 共用命名空间（reserved 已占用则加后缀）。
function buildSessionMentionOptions(
  sessions: SessionRecord[],
  reserved: Set<string>,
): MentionOption[] {
  const used = new Set(reserved);
  return sessions
    .filter(
      (session) =>
        session.ptyId !== null &&
        session.agentRuntimeKind !== "oh_my_pi" &&
        session.agentRuntimeKind !== "unsupported" &&
        (session.kind === "claude" || Boolean(session.agentRuntimeKind)),
    )
    .sort((left, right) => left.createdAt - right.createdAt || left.id.localeCompare(right.id))
    .map((session) => {
      const fallback = `会话-${session.id.slice(0, 4)}`;
      const base = compactAgentHandle(session.title, fallback);
      let handle = base;
      let suffix = 2;
      while (used.has(handle.toLocaleLowerCase())) {
        handle = `${base}-${suffix}`;
        suffix += 1;
      }
      used.add(handle.toLocaleLowerCase());
      const status = session.ptyId !== null ? "运行中" : "未运行";
      return {
        key: `session:${session.id}`,
        kind: "session" as const,
        token: `@${handle}`,
        description: [session.providerName, status, session.title].join(" · "),
        section: "终端会话" as const,
        sessionId: session.id,
        runtimeKind: session.agentRuntimeKind ?? (session.kind === "claude" ? "claude_code" : undefined),
        avatarId: session.agentAvatarId,
        targetCount: 1,
      };
    });
}

interface SessionMentionTarget {
  option: MentionOption;
  session: SessionRecord;
}

function resolveSessionMentionTargets(
  value: string,
  options: MentionOption[],
  sessions: SessionRecord[],
): SessionMentionTarget[] {
  const byId = new Map(sessions.map((session) => [session.id, session]));
  const seen = new Set<string>();
  const targets: SessionMentionTarget[] = [];
  for (const option of options) {
    if (option.kind !== "session" || !option.sessionId) continue;
    if (!hasMentionToken(value, option.token) || seen.has(option.sessionId)) continue;
    const session = byId.get(option.sessionId);
    if (!session) continue;
    seen.add(option.sessionId);
    targets.push({ option, session });
  }
  return targets;
}

function resolveMentionTargetAgentIds(
  value: string,
  options: MentionOption[],
  snapshot: MultiAgentSnapshot | null,
  bindings: MentionBinding[],
): string[] {
  const selected = new Set<string>();
  const boundTokens = new Set<string>();
  for (const binding of bindings) {
    if (!hasMentionToken(value, binding.token)) continue;
    if (!isMentionBindingAvailable(snapshot, binding)) continue;
    boundTokens.add(binding.token.toLocaleLowerCase());
    selected.add(binding.agentId);
  }
  if (!snapshot) return [...selected];
  for (const option of options) {
    if (!hasMentionToken(value, option.token)) continue;
    if (option.kind === "all") {
      snapshot.agents.filter((agent) => agent.enabled).forEach((agent) => selected.add(agent.id));
    } else if (option.kind === "runtime" && option.runtimeKind) {
      const hasSelectedRuntime = snapshot.agents.some(
        (agent) => selected.has(agent.id) && agent.runtimeKind === option.runtimeKind,
      );
      if (!hasSelectedRuntime) {
        const agent = preferredAgentForRuntime(snapshot, option.runtimeKind);
        if (agent) selected.add(agent.id);
      }
    } else if (
      option.agentId &&
      !boundTokens.has(option.token.toLocaleLowerCase())
    ) {
      selected.add(option.agentId);
    }
  }
  return [...selected];
}

function resolveMentionTargetRuntimeKinds(value: string, options: MentionOption[]): AgentRuntimeKind[] {
  return [...new Set(options
    .filter((option) => option.kind === "runtime" && option.runtimeKind && hasMentionToken(value, option.token))
    .map((option) => option.runtimeKind!))];
}

function countMentionTargets(agentIds: string[], runtimeKinds: AgentRuntimeKind[], sessions: SessionRecord[], snapshot: MultiAgentSnapshot | null): number {
  const agents = snapshot?.agents.filter((agent) => agentIds.includes(agent.id)) ?? [];
  const coveredRuntimes = new Set([...agents.map((agent) => agent.runtimeKind), ...sessions.map((session) => session.agentRuntimeKind)]);
  const separateSessions = sessions.filter((session) => !session.multiAgentAgentId || !agentIds.includes(session.multiAgentAgentId));
  return agentIds.length + separateSessions.length + runtimeKinds.filter((runtime) => !coveredRuntimes.has(runtime)).length;
}

function resolveExplicitMentionTargetAgentIds(
  value: string,
  options: MentionOption[],
  snapshot: MultiAgentSnapshot | null,
  bindings: MentionBinding[],
): string[] {
  const selected = new Set<string>();
  const boundTokens = new Set<string>();
  for (const binding of bindings) {
    if (!hasMentionToken(value, binding.token)) continue;
    if (!isMentionBindingAvailable(snapshot, binding)) continue;
    boundTokens.add(binding.token.toLocaleLowerCase());
    selected.add(binding.agentId);
  }
  if (!snapshot) return [...selected];
  for (const option of options) {
    if (!hasMentionToken(value, option.token)) continue;
    if (option.kind === "all") {
      snapshot.agents.filter((agent) => agent.enabled).forEach((agent) => selected.add(agent.id));
    } else if (
      option.kind === "agent" &&
      option.agentId &&
      !boundTokens.has(option.token.toLocaleLowerCase())
    ) {
      selected.add(option.agentId);
    }
  }
  return [...selected];
}

function suggestedProjectPath(
  basePath: string,
  projectName: string | null,
): string {
  const base = basePath.trim().replace(/[\\/]+$/, "");
  if (!base) return "";
  const directoryName = (projectName ?? "crewvela-project")
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, "-")
    .trim()
    .replace(/[. ]+$/g, "")
    .slice(0, 48) || "crewvela-project";
  const currentDirectory = base.split(/[\\/]/).pop()?.toLocaleLowerCase();
  if (currentDirectory === directoryName.toLocaleLowerCase()) return base;
  const separator = base.includes("\\") ? "\\" : "/";
  return `${base}${separator}${directoryName}`;
}

function collaborationResultMessage(snapshot: MultiAgentSnapshot): string {
  const completed = snapshot.tasks.filter((task) => task.status === "done");
  const failed = snapshot.tasks.filter((task) => task.status === "failed");
  const latestSummaries = snapshot.runs
    .filter((run) => run.outputSummary?.trim())
    .sort((left, right) => (right.finishedAt ?? right.createdAt) - (left.finishedAt ?? left.createdAt))
    .slice(0, 3)
    .map((run) => `- ${run.outputSummary!.trim()}`);
  const artifactLines = snapshot.artifacts
    .slice(0, 3)
    .map((artifact) => `- ${artifact.summary}（${artifact.uri}）`);
  return [
    `协作开发已完成：${completed.length}/${snapshot.tasks.length} 个任务完成${failed.length ? `，${failed.length} 个失败` : ""}。`,
    `项目目录：${snapshot.project.rootPath}`,
    latestSummaries.length ? `执行结果：\n${latestSummaries.join("\n")}` : "",
    artifactLines.length ? `交付物：\n${artifactLines.join("\n")}` : "",
  ]
    .filter(Boolean)
    .join("\n");
}

let runtimeProbeCache: RuntimeProbe[] | null = null;
let runtimeProbeRequest: Promise<RuntimeProbe[]> | null = null;

function loadRuntimeProbes(): Promise<RuntimeProbe[]> {
  if (runtimeProbeCache) return Promise.resolve(runtimeProbeCache);
  if (runtimeProbeRequest) return runtimeProbeRequest;

  runtimeProbeRequest = invoke<RuntimeProbe[]>("ma_probe_runtimes")
    .then((result) => {
      runtimeProbeCache = result;
      return result;
    })
    .finally(() => {
      runtimeProbeRequest = null;
    });
  return runtimeProbeRequest;
}

const TEAM_BLUEPRINT: Array<{
  runtimeKind: AgentRuntimeKind;
  name: string;
  role: AgentRole;
  description: string;
}> = [
  {
    runtimeKind: "claude_code",
    name: "Claude 架构师",
    role: "orchestrator",
    description: "负责理解目标、拆分阶段、协调依赖和汇总结果。",
  },
  {
    runtimeKind: "codex",
    name: "Codex 工程师",
    role: "worker",
    description: "负责主要编码、重构和运行验证。",
  },
  {
    runtimeKind: "opencode",
    name: "OpenCode 工程师",
    role: "worker",
    description: "负责可并行实现、界面或独立模块。",
  },
  {
    runtimeKind: "pi",
    name: "Pi 测试工程师",
    role: "reviewer",
    description: "负责测试设计、回归验证和问题复现。",
  },
];

function loadSelectedProjectId(): string | null {
  try {
    return window.localStorage.getItem("crewvela-multi-agent-project");
  } catch {
    return null;
  }
}

function loadCoordinatorDraft() {
  try {
    return decodeMultiAgentDraft(
      window.localStorage.getItem(COORDINATOR_DRAFT_STORAGE_KEY),
    );
  } catch {
    return decodeMultiAgentDraft(null);
  }
}

function loadAutoReportState(): AutoReportState {
  const empty: AutoReportState = {
    projectIds: [],
    approvalRevisions: {},
    failureRevisions: {},
  };
  try {
    const value: unknown = JSON.parse(
      window.localStorage.getItem(AUTO_REPORT_STORAGE_KEY) ?? "null",
    );
    if (!value || typeof value !== "object" || Array.isArray(value)) return empty;
    const record = value as Record<string, unknown>;
    const numberRecord = (candidate: unknown): Record<string, number> =>
      candidate && typeof candidate === "object" && !Array.isArray(candidate)
        ? Object.fromEntries(
            Object.entries(candidate).filter(
              ([key, revision]) => key.length > 0 && typeof revision === "number",
            ),
          )
        : {};
    return {
      projectIds: Array.isArray(record.projectIds)
        ? record.projectIds.filter((id): id is string => typeof id === "string")
        : [],
      approvalRevisions: numberRecord(record.approvalRevisions),
      failureRevisions: numberRecord(record.failureRevisions),
    };
  } catch {
    return empty;
  }
}

function coordinatorMessagesFromSnapshot(
  snapshot: MultiAgentSnapshot,
): CoordinatorHistoryMessage[] {
  const clearedAt = snapshot.events
    .filter((event) => event.eventType === "conversation.cleared")
    .reduce((latest, event) => Math.max(latest, event.createdAt), 0);
  return snapshot.messages
    .filter(
      (message) =>
        message.createdAt > clearedAt &&
        (message.senderType === "user" ||
          message.senderType === "coordinator"),
    )
    .slice(-MAX_COORDINATOR_HISTORY)
    .map((message): CoordinatorHistoryMessage => ({
      role: message.senderType === "user" ? "user" : "assistant",
      content: message.body,
      surface: "multi_agent",
    }));
}

const LOW_VALUE_SYSTEM_ACTIVITY_PATTERNS = [
  /^计划已确认[：:]/,
  /^计划已落地(?:并启动)?[：:]/,
  /^项目调度(?:器)?已(?:启动|在运行)/,
];

function isLowValueSystemActivity(
  message: CoordinatorHistoryMessage,
): boolean {
  if (message.role !== "assistant") return false;
  const content = message.content.trim();
  return LOW_VALUE_SYSTEM_ACTIVITY_PATTERNS.some((pattern) =>
    pattern.test(content),
  );
}

interface SystemActivityGroup {
  content: string;
  count: number;
}

function groupSystemActivities(
  messages: CoordinatorHistoryMessage[],
): SystemActivityGroup[] {
  const groups: SystemActivityGroup[] = [];
  let previousWasSystemActivity = false;
  messages.forEach((message) => {
    if (!isLowValueSystemActivity(message)) {
      previousWasSystemActivity = false;
      return;
    }
    const content = message.content.trim();
    const previous = groups[groups.length - 1];
    if (previousWasSystemActivity && previous?.content === content) {
      previous.count += 1;
    } else {
      groups.push({ content, count: 1 });
    }
    previousWasSystemActivity = true;
  });
  return groups;
}

function dedupeConsecutiveSystemMessages(
  messages: CoordinatorHistoryMessage[],
): CoordinatorHistoryMessage[] {
  return messages.reduce<CoordinatorHistoryMessage[]>((items, message) => {
    const previous = items[items.length - 1];
    if (
      previous &&
      isLowValueSystemActivity(previous) &&
      isLowValueSystemActivity(message) &&
      previous.content.trim() === message.content.trim()
    ) {
      return items;
    }
    items.push(message);
    return items;
  }, []);
}

export function MultiAgentControl({
  sessions = [],
  active = true,
  defaultCwd,
  onOpenWorkspace,
  focusedProjectId,
  focusedProjectRequest,
  onOpenQuickCommands,
  onQuickCommand,
  onQuickBroadcast,
  onQuickPermissionModeChange,
  onClearQuickHistory,
  quickLiveSessionCount = 0,
  quickKnownModeCount = 0,
  quickPermissionMode = "mixed",
  quickModesKnown = false,
  quickModesSwitchable = false,
  quickPermissionModeBusy = false,
  workspaceTrustStatus,
  workspaceTrustBusy = false,
  onConfirmWorkspaceTrust,
}: Props) {
  const { ready: availabilityReady, error: availabilityError, prefs: runtimePreferences, isRuntimeEnabled } = useRuntimeAvailability();
  const [collaborationView, setCollaborationView] = useState<"tasks" | "messages">("tasks");
  const [initialAutoReportState] = useState(loadAutoReportState);
  const autoReportProjects = useRef(new Set(initialAutoReportState.projectIds));
  const directDeliveryTasks = useRef(new Map<string, Set<string>>());
  const reportedApprovalTasks = useRef(
    new Map(Object.entries(initialAutoReportState.approvalRevisions)),
  );
  const reportedFailedTasks = useRef(
    new Map(Object.entries(initialAutoReportState.failureRevisions)),
  );
  const detailDrawerRef = useRef<HTMLDetailsElement | null>(null);
  const coordinatorTextareaRef = useRef<HTMLTextAreaElement | null>(null);
  const coordinatorHighlightRef = useRef<HTMLDivElement | null>(null);
  const targetMenuRef = useRef<HTMLDivElement | null>(null);
  const quickTargetMenuRef = useRef<HTMLDivElement | null>(null);
  const quickModeMenuRef = useRef<HTMLDivElement | null>(null);
  const textStreamRequestRef = useRef<string | null>(null);
  const [streamedReply, setStreamedReply] = useState<{
    requestId: string;
    text: string;
  } | null>(null);
  const mentionMenuId = useId();
  const persistAutoReportState = useCallback(() => {
    try {
      window.localStorage.setItem(
        AUTO_REPORT_STORAGE_KEY,
        JSON.stringify({
          projectIds: [...autoReportProjects.current],
          approvalRevisions: Object.fromEntries(reportedApprovalTasks.current),
          failureRevisions: Object.fromEntries(reportedFailedTasks.current),
        } satisfies AutoReportState),
      );
    } catch {
      // 本地存储不可用时，不阻断协作执行。
    }
  }, []);
  const requestOpenWorkspace = useCallback((projectId: string) => {
    if (onOpenWorkspace) {
      onOpenWorkspace(projectId);
    } else {
      void emit("multi-agent-workspace-open", { projectId });
    }
  }, [onOpenWorkspace]);
  const [initialCoordinatorDraft] = useState(loadCoordinatorDraft);
  const [initialSelectedProjectId] = useState(loadSelectedProjectId);
  const [projects, setProjects] = useState<MultiAgentProject[]>(
    UI_PREVIEW ? [MULTI_AGENT_UI_PREVIEW_SNAPSHOT.project] : [],
  );
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(
    UI_PREVIEW
      ? MULTI_AGENT_UI_PREVIEW_SNAPSHOT.project.id
      : initialSelectedProjectId,
  );
  const [snapshotState, setSnapshot] = useState<MultiAgentSnapshot | null>(
    UI_PREVIEW ? MULTI_AGENT_UI_PREVIEW_SNAPSHOT : null,
  );
  const snapshot = snapshotState?.project.id === selectedProjectId ? snapshotState : null;
  const [runtimes, setRuntimes] = useState<RuntimeProbe[]>(
    () => runtimeProbeCache ?? [],
  );
  const [busy, setBusyState] = useState(false);
  const busyRef = useRef(false);
  const setBusy = (value: boolean) => { busyRef.current = value; setBusyState(value); };
  const applyingPlan = useRef(false);
  const optimizationRequest = useRef(0);
  const coordinatorSending = useRef(false);
  const activeProjectId = useRef(selectedProjectId);
  activeProjectId.current = selectedProjectId;
  const snapshotRequest = useRef(0);
  const failedCollaborationMessages = useRef(new Map<string, CoordinatorHistoryMessage[]>());
  const [collaborationAuto, setCollaborationAuto] = useState(false);
  const [autoProjectId, setAutoProjectId] = useState<string | null>(null);
  const autoApprovalAttempts = useRef(new Set<string>());
  const autoApproving = useRef(false);

  useEffect(() => {
    let enabled = false;
    try { enabled = window.localStorage.getItem(`crewvela-collaboration-auto:${selectedProjectId ?? "draft"}`) === "true"; } catch { /* Storage may be disabled. */ }
    setCollaborationAuto(enabled);
    setAutoProjectId(selectedProjectId);
  }, [selectedProjectId]);

  const autoEnabled = collaborationAuto && autoProjectId === selectedProjectId;
  const automationContext = useRef({ projectId: selectedProjectId, enabled: autoEnabled && active });
  automationContext.current = { projectId: selectedProjectId, enabled: autoEnabled && active };
  // 同一窗口不会收到自己的 storage 事件；多个常驻面板必须同步项目的 Auto 开关。
  useEffect(() => {
    automationContext.current.enabled = autoEnabled && active;
    const apply = (enabled: boolean) => {
      automationContext.current.enabled = enabled && active;
      setAutoProjectId(selectedProjectId);
      setCollaborationAuto(enabled);
      autoApprovalAttempts.current.clear();
    };
    const onMode = (event: Event) => {
      const detail = (event as CustomEvent<{ projectId: string | null; enabled: boolean }>).detail;
      if (detail.projectId === selectedProjectId) apply(detail.enabled);
    };
    const onStorage = (event: StorageEvent) => {
      if (event.key === `crewvela-collaboration-auto:${selectedProjectId ?? "draft"}`) apply(event.newValue === "true");
    };
    window.addEventListener("crewvela:collaboration-auto-changed", onMode);
    window.addEventListener("storage", onStorage);
    return () => {
      window.removeEventListener("crewvela:collaboration-auto-changed", onMode);
      window.removeEventListener("storage", onStorage);
      automationContext.current.enabled = false;
    };
  }, [selectedProjectId, active]);
  const changeCollaborationAuto = (enabled: boolean) => {
    automationContext.current.enabled = enabled && active;
    setAutoProjectId(selectedProjectId);
    setCollaborationAuto(enabled);
    autoApprovalAttempts.current.clear();
    window.dispatchEvent(new CustomEvent("crewvela:collaboration-auto-changed", { detail: { projectId: selectedProjectId, enabled } }));
    try { window.localStorage.setItem(`crewvela-collaboration-auto:${selectedProjectId ?? "draft"}`, String(enabled)); } catch { /* Keep the current choice for this panel. */ }
  };
  const [error, setError] = useState<string | null>(null);
  const [projectName, setProjectName] = useState("");
  const [projectGoal, setProjectGoal] = useState("");
  const [projectPath, setProjectPath] = useState(
    initialCoordinatorDraft.projectPath || defaultCwd || "",
  );
  const [taskTitle, setTaskTitle] = useState("");
  const [taskDescription, setTaskDescription] = useState("");
  const [taskStage, setTaskStage] = useState(0);
  const [taskWorkspaceMode, setTaskWorkspaceMode] =
    useState<WorkspaceMode>("worktree");
  const [taskAssigneeId, setTaskAssigneeId] = useState("");
  const [coordinatorDraft, setCoordinatorDraft] = useState(
    initialCoordinatorDraft.coordinatorDraft,
  );
  const optimizationInputs = useRef({ draft: coordinatorDraft, projectId: selectedProjectId });
  optimizationInputs.current = { draft: coordinatorDraft, projectId: selectedProjectId };
  const [optimization, setOptimization] = useState<PromptOptimization | null>(null);
  const [optimizing, setOptimizing] = useState(false);
  const [optimizationError, setOptimizationError] = useState<string | null>(null);
  const [mentionBindings, setMentionBindings] = useState<MentionBinding[]>(
    initialCoordinatorDraft.mentionBindings,
  );
  const [mentionCursor, setMentionCursor] = useState(0);
  const [mentionMenuOpen, setMentionMenuOpen] = useState(false);
  const [mentionOptionIndex, setMentionOptionIndex] = useState(0);
  const [targetMenuOpen, setTargetMenuOpen] = useState(false);
  const [quickSendTarget, setQuickSendTarget] = useState<"control" | "all">("control");
  const [quickTargetMenuOpen, setQuickTargetMenuOpen] = useState(false);
  const [quickModeMenuOpen, setQuickModeMenuOpen] = useState(false);
  const [coordinatorFocused, setCoordinatorFocused] = useState(false);
  const [showSystemMessages, setShowSystemMessages] = useState(false);
  const [clearConversationConfirmOpen, setClearConversationConfirmOpen] =
    useState(false);
  const clearConfirmRef = useRef<HTMLDivElement | null>(null);
  const clearConfirmWasOpen = useRef(false);
  useEffect(() => {
    if (clearConversationConfirmOpen) {
      clearConfirmWasOpen.current = true;
      clearConfirmRef.current?.scrollIntoView({ block: "nearest" });
      clearConfirmRef.current?.focus();
    } else if (clearConfirmWasOpen.current) {
      clearConfirmWasOpen.current = false;
      coordinatorTextareaRef.current?.focus();
    }
  }, [clearConversationConfirmOpen]);
  const [planAdvancedOpen, setPlanAdvancedOpen] = useState(
    Boolean(
      initialCoordinatorDraft.pendingPlan &&
        !initialCoordinatorDraft.pendingPlanContext?.autoStart,
    ),
  );
  const [coordinatorProjectId, setCoordinatorProjectId] = useState<string | null>(
    initialCoordinatorDraft.coordinatorProjectId,
  );
  const [coordinatorMessages, setCoordinatorMessages] = useState<CoordinatorHistoryMessage[]>(
    UI_PREVIEW
      ? coordinatorMessagesFromSnapshot(MULTI_AGENT_UI_PREVIEW_SNAPSHOT)
      : initialCoordinatorDraft.coordinatorMessages,
  );
  const [copiedMessageKey, setCopiedMessageKey] = useState<string | null>(null);
  // 网格终端会话列表：@ 定向消息的目标源，5s 轮询保持标题/状态新鲜。
  const [gridSessions, setGridSessions] = useState<SessionRecord[]>([]);
  useEffect(() => {
    if (UI_PREVIEW) return;
    const unlisten = listen<CoordinatorReplyStreamEvent>(
      COORDINATOR_REPLY_STREAM_EVENT,
      (event) => {
        const update = event.payload;
        if (update.requestId === textStreamRequestRef.current) {
          if (update.reset) {
            setStreamedReply(null);
          } else if (update.text !== null) {
            setStreamedReply({ requestId: update.requestId, text: update.text });
          }
          if (update.done) {
            textStreamRequestRef.current = null;
          }
          return;
        }
      },
    );
    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, []);
  useEffect(() => {
    let cancelled = false;
    const refresh = () => {
      invoke<SessionRecord[]>("list_sessions")
        .then((records) => {
          if (!cancelled) setGridSessions(records);
        })
        .catch(() => {});
    };
    refresh();
    const timer = window.setInterval(refresh, 5000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, []);
  const [pendingPlan, setPendingPlan] = useState<CoordinatorPlan | null>(
    initialCoordinatorDraft.pendingPlan,
  );
  const [pendingPlanContext, setPendingPlanContext] =
    useState<PendingPlanContext | null>(
      initialCoordinatorDraft.pendingPlanContext,
    );
  const runtimeHealthByKind = useMemo(
    () =>
      new Map(
        runtimes.map((runtime) => [runtime.kind, getRuntimeHealth(runtime)]),
      ),
    [runtimes],
  );

  const availableSnapshot = useMemo(() => snapshot ? {
    ...snapshot, agents: snapshot.agents.filter((agent) => isRuntimeEnabled(agent.runtimeKind)),
  } : null, [snapshot, availabilityReady, runtimePreferences]);
  const availableAgents = availableSnapshot?.agents ?? [];
  const availableGridSessions = useMemo(() => gridSessions.filter((session) => {
    const kind = session.agentRuntimeKind ?? (session.kind === "claude" ? "claude_code" : null);
    return kind === null || isRuntimeEnabled(kind);
  }), [gridSessions, availabilityReady, runtimePreferences]);
  const activeMention = useMemo(
    () =>
      mentionMenuOpen
        ? activeMentionAt(coordinatorDraft, mentionCursor)
        : null,
    [coordinatorDraft, mentionCursor, mentionMenuOpen],
  );
  const mentionOptions = useMemo(() => {
    const runtimeOptions: MentionOption[] = GROUP_MENTION_OPTIONS
      .filter((option) => isRuntimeEnabled(option.runtimeKind))
      .filter(
        (option) =>
          runtimeHealthByKind.get(option.runtimeKind) !== "missing" ||
          availableSnapshot?.agents.some(
            (agent) => agent.enabled && agent.runtimeKind === option.runtimeKind,
          ),
      )
      .map((option) => {
        const targetCount = availableSnapshot?.agents.filter(
          (agent) => agent.enabled && agent.runtimeKind === option.runtimeKind,
        ).length ?? 0;
        return {
          key: `runtime:${option.runtimeKind}`,
          kind: "runtime" as const,
          token: option.token,
          description: targetCount > 0
            ? `${option.description} · ${targetCount} 个实例`
            : option.description,
          section: "群组" as const,
          runtimeKind: option.runtimeKind,
          targetCount,
        };
      });
    const allOption: MentionOption[] = availableSnapshot?.agents.some((agent) => agent.enabled)
      ? [{
          key: "group:all",
          kind: "all" as const,
          token: "@All",
          description: `当前项目全部 Agent · ${availableSnapshot.agents.filter((agent) => agent.enabled).length} 个实例`,
          section: "群组" as const,
          targetCount: availableSnapshot.agents.filter((agent) => agent.enabled).length,
        }]
      : [];
    const agentOptions = buildAgentMentionOptions(availableSnapshot);
    const reservedHandles = new Set(
      [
        ...GROUP_MENTION_OPTIONS.map((option) => option.token),
        "@All",
        ...agentOptions.map((option) => option.token),
      ].map((token) => token.slice(1).toLocaleLowerCase()),
    );
    return [
      ...allOption,
      ...runtimeOptions,
      ...agentOptions,
      ...buildSessionMentionOptions(availableGridSessions, reservedHandles),
    ];
  }, [runtimeHealthByKind, availableSnapshot, availableGridSessions, availabilityReady, runtimePreferences]);
  const filteredMentionOptions = useMemo(
    () =>
      activeMention
        ? mentionOptions.filter((option) => {
            const query = activeMention.query;
            return (
              option.token.slice(1).toLocaleLowerCase().includes(query) ||
              option.description.toLocaleLowerCase().includes(query)
            );
          })
        : [],
    [activeMention, mentionOptions],
  );
  const selectedMentionAgentIds = useMemo(
    () => resolveMentionTargetAgentIds(
      coordinatorDraft,
      mentionOptions,
      availableSnapshot,
      mentionBindings,
    ),
    [coordinatorDraft, mentionBindings, mentionOptions, availableSnapshot],
  );
  const quickComposerActive =
    selectedMentionAgentIds.length === 0 &&
    resolveUnifiedControlRoute(coordinatorDraft) === "quick";
  const selectedMentionTargetCount = countMentionTargets(
    selectedMentionAgentIds,
    resolveMentionTargetRuntimeKinds(coordinatorDraft, mentionOptions),
    resolveSessionMentionTargets(coordinatorDraft, mentionOptions, availableGridSessions).map(({ session }) => session),
    availableSnapshot,
  );
  const quickCompletionSuffix =
    quickComposerActive && QUICK_PROMPT_COMPLETION.startsWith(coordinatorDraft)
      ? QUICK_PROMPT_COMPLETION.slice(coordinatorDraft.length)
      : "";
  const canCompleteQuickPrompt =
    coordinatorFocused &&
    coordinatorDraft.length > 0 &&
    !busy &&
    quickCompletionSuffix.length > 0;
  const quickPermissionModeLabel =
    quickPermissionMode === "mixed"
      ? quickModesKnown
        ? "Mixed"
        : "等待"
      : QUICK_PERMISSION_MODES.find((mode) => mode.value === quickPermissionMode)?.label ??
        quickPermissionMode;
  const mentionHighlightTokens = useMemo(() => {
    const highlighted = new Map<string, MentionHighlightToken>();
    for (const option of mentionOptions) {
      if (!hasMentionToken(coordinatorDraft, option.token)) continue;
      highlighted.set(option.token.toLocaleLowerCase(), {
        token: option.token,
        kind: option.kind === "agent" || option.kind === "session" ? "agent" : "group",
      });
    }
    for (const binding of mentionBindings) {
      if (!hasMentionToken(coordinatorDraft, binding.token)) continue;
      const valid = snapshot === null || snapshot.agents.some(
        (agent) => agent.id === binding.agentId && agent.enabled,
      );
      highlighted.set(binding.token.toLocaleLowerCase(), {
        token: binding.token,
        kind: valid ? "agent" : "invalid",
      });
    }
    return [...highlighted.values()];
  }, [coordinatorDraft, mentionBindings, mentionOptions, snapshot]);
  const coordinatorDraftSegments = useMemo(
    () => splitDraftByMentionHighlights(coordinatorDraft, mentionHighlightTokens),
    [coordinatorDraft, mentionHighlightTokens],
  );
  const groupedSystemActivities = useMemo(
    () => groupSystemActivities(coordinatorMessages),
    [coordinatorMessages],
  );
  const systemActivityCount = useMemo(
    () =>
      groupedSystemActivities.reduce(
        (total, activity) => total + activity.count,
        0,
      ),
    [groupedSystemActivities],
  );
  const visibleCoordinatorMessages = useMemo(
    () => {
      const messages = showSystemMessages
        ? dedupeConsecutiveSystemMessages(coordinatorMessages)
        : coordinatorMessages.filter(
            (message) => !isLowValueSystemActivity(message),
          );
      return messages.slice(-8);
    },
    [coordinatorMessages, showSystemMessages],
  );

  const refreshProjects = useCallback(async () => {
    if (UI_PREVIEW) {
      setProjects([MULTI_AGENT_UI_PREVIEW_SNAPSHOT.project]);
      setSelectedProjectId(MULTI_AGENT_UI_PREVIEW_SNAPSHOT.project.id);
      return;
    }
    const next = await invoke<MultiAgentProject[]>("ma_list_projects");
    setProjects(next);
    setSelectedProjectId((current) => {
      if (current && next.some((project) => project.id === current)) return current;
      return next[0]?.id ?? null;
    });
  }, []);

  const refreshSnapshot = useCallback(async () => {
    if (UI_PREVIEW) {
      setSnapshot(MULTI_AGENT_UI_PREVIEW_SNAPSHOT);
      return;
    }
    if (!selectedProjectId) {
      setSnapshot(null);
      return;
    }
    const request = ++snapshotRequest.current;
    try {
      const next = await invoke<MultiAgentSnapshot>("ma_snapshot", { projectId: selectedProjectId });
      if (request === snapshotRequest.current && activeProjectId.current === selectedProjectId) setSnapshot(next);
    } catch (reason) {
      if (request === snapshotRequest.current && activeProjectId.current === selectedProjectId) throw reason;
    }
  }, [selectedProjectId]);


  useEffect(() => {
    let cancelled = false;
    refreshProjects().catch((reason) => {
      if (!cancelled) setError(String(reason));
    });
    if (UI_PREVIEW) {
      return () => {
        cancelled = true;
      };
    }
    loadRuntimeProbes()
      .then((result) => {
        if (!cancelled) setRuntimes(result);
      })
      .catch((reason) => {
        if (!cancelled) setError(String(reason));
      });
    return () => {
      cancelled = true;
    };
  }, [refreshProjects]);

  useEffect(() => {
    try {
      if (selectedProjectId) {
        window.localStorage.setItem(
          "crewvela-multi-agent-project",
          selectedProjectId,
        );
      }
    } catch {
      // 当前窗口内仍可继续使用。
    }
    refreshSnapshot().catch((reason) => setError(String(reason)));
    if (!selectedProjectId) return;
    const timer = window.setInterval(() => {
      refreshSnapshot().catch((reason) => setError(String(reason)));
    }, 1000);
    return () => { window.clearInterval(timer); snapshotRequest.current += 1; };
  }, [refreshSnapshot, selectedProjectId]);

  useEffect(() => {
    if (focusedProjectId) { activeProjectId.current = focusedProjectId; snapshotRequest.current += 1; setSelectedProjectId(focusedProjectId); }
  }, [focusedProjectId, focusedProjectRequest]);

  useEffect(() => {
    const handleProjectSelection = (event: Event) => {
      const projectId = (event as CustomEvent<{ projectId?: string }>).detail
        ?.projectId;
      if (projectId) { activeProjectId.current = projectId; snapshotRequest.current += 1; setSelectedProjectId(projectId); }
    };
    window.addEventListener(
      "crewvela-multi-agent-project-selected",
      handleProjectSelection,
    );
    return () => {
      window.removeEventListener(
        "crewvela-multi-agent-project-selected",
        handleProjectSelection,
      );
    };
  }, []);

  useEffect(() => {
    if (!projectPath && defaultCwd) setProjectPath(defaultCwd);
  }, [defaultCwd, projectPath]);

  useEffect(() => {
    try {
      window.localStorage.setItem(
        COORDINATOR_DRAFT_STORAGE_KEY,
        encodeMultiAgentDraft({
          projectPath,
          coordinatorProjectId,
          coordinatorDraft,
          mentionBindings,
          coordinatorMessages,
          pendingPlan,
          pendingPlanContext,
        }),
      );
    } catch {
      // 本地存储不可用时仍保留当前窗口内状态。
    }
  }, [
    coordinatorDraft,
    coordinatorMessages,
    coordinatorProjectId,
    mentionBindings,
    pendingPlan,
    pendingPlanContext,
    projectPath,
  ]);

  useEffect(() => {
    if (!snapshot) return;
    if (
      pendingPlan &&
      pendingPlanContext &&
      pendingPlanContext.projectId !== snapshot.project.id
    ) {
      return;
    }
    const restored = coordinatorMessagesFromSnapshot(snapshot);
    if (snapshot.project.id !== selectedProjectId) return;
    setCoordinatorMessages((current) => {
      if (coordinatorSending.current && coordinatorProjectId === snapshot.project.id) return current;
      if (
        pendingPlan &&
        pendingPlanContext?.projectId === snapshot.project.id
      ) {
        return current;
      }
      const quickMessages = current.filter((item) => item.surface === "quick");
      return [...quickMessages, ...mergeFailedCollaborationMessages(restored,
        failedCollaborationMessages.current.get(snapshot.project.id) ?? [])].slice(-MAX_COORDINATOR_HISTORY);
    });
    setCoordinatorProjectId(snapshot.project.id);
  }, [pendingPlan, pendingPlanContext?.projectId, snapshot, selectedProjectId]);

  const createProject = async (event: FormEvent) => {
    event.preventDefault();
    if (!projectName.trim() || !projectGoal.trim() || !projectPath.trim() || busyRef.current) return;
    setBusy(true);
    setError(null);
    try {
      const project = await invoke<MultiAgentProject>("ma_create_project", {
        input: {
          name: projectName,
          goal: projectGoal,
          rootPath: projectPath,
          defaultBranch: "main",
        },
      });
      await refreshProjects();
      setSelectedProjectId(project.id);
      setProjectName("");
      setProjectGoal("");
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const createDefaultTeam = async () => {
    if (!availabilityReady || !selectedProjectId || busyRef.current || snapshot?.agents.length) return;
    setBusy(true);
    setError(null);
    try {
      const results = await Promise.allSettled(
        TEAM_BLUEPRINT.filter((member) => isRuntimeEnabled(member.runtimeKind)).map((member) =>
          invoke<AgentProfile>("ma_create_agent", {
            input: {
              projectId: selectedProjectId,
              name: member.name,
              runtimeKind: member.runtimeKind,
              role: member.role,
              description: member.description,
              model: null,
              permissionProfile: "default",
              maxConcurrency: 1,
            },
          }),
        ),
      );
      const failures = results.filter(
        (result): result is PromiseRejectedResult => result.status === "rejected",
      );
      if (failures.length) {
        setError(failures.map((failure) => String(failure.reason)).join("；"));
      }
      await refreshSnapshot();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const createTask = async (event: FormEvent) => {
    event.preventDefault();
    if (!selectedProjectId || !taskTitle.trim() || busyRef.current) return;
    if (!availabilityReady || (taskAssigneeId && !availableAgents.some((agent) => agent.id === taskAssigneeId))) { setError(availabilityError ?? "请选择已启用的 Agent"); return; }
    setBusy(true);
    setError(null);
    try {
      await invoke("ma_create_task", {
        input: {
          projectId: selectedProjectId,
          parentId: null,
          stage: taskStage,
          title: taskTitle,
          description: taskDescription,
          acceptanceCriteria: "",
          verificationCommand: null,
          assigneeAgentId: taskAssigneeId || null,
          reviewerAgentId: null,
          workspaceMode: taskWorkspaceMode,
          priority: 0,
        },
      });
      setTaskTitle("");
      setTaskDescription("");
      await refreshSnapshot();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const appendCoordinatorMessage = useCallback(
    (
      role: "user" | "assistant",
      content: string,
      projectId: string | null = selectedProjectId,
      surface: ControlSurface = "multi_agent",
    ) => {
      setCoordinatorProjectId(projectId);
      setCoordinatorMessages((items) => {
        const normalizedContent = content.trim();
        const previous = items[items.length - 1];
        const nextMessage = { role, content, surface } satisfies CoordinatorHistoryMessage;
        if (
          role === "assistant" &&
          previous?.role === role &&
          previous.surface === surface &&
          previous.content.trim() === normalizedContent
        ) {
          return items;
        }
        if (
          isLowValueSystemActivity(nextMessage) &&
          previous &&
          isLowValueSystemActivity(previous) &&
          previous?.role === role &&
          previous.content.trim() === normalizedContent
        ) {
          return items;
        }
        return [...items, nextMessage].slice(-MAX_COORDINATOR_HISTORY);
      });
    },
    [selectedProjectId],
  );

  const copyCoordinatorMessage = useCallback(async (key: string, content: string) => {
    try {
      await navigator.clipboard.writeText(content);
      setCopiedMessageKey(key);
      window.setTimeout(
        () => setCopiedMessageKey((current) => (current === key ? null : current)),
        1_500,
      );
    } catch (reason) {
      setError("复制消息失败：" + String(reason));
    }
  }, []);
  const insertMention = useCallback(
    (option: MentionOption) => {
      const mention = activeMentionAt(coordinatorDraft, mentionCursor);
      const start = mention?.start ?? coordinatorDraft.length;
      const end = mention?.end ?? coordinatorDraft.length;
      const needsLeadingSpace = start > 0 && !/\s/.test(coordinatorDraft[start - 1]);
      const insertion = `${needsLeadingSpace ? " " : ""}${option.token} `;
      const next = `${coordinatorDraft.slice(0, start)}${insertion}${coordinatorDraft.slice(end)}`;
      const nextCursor = start + insertion.length;
      setCoordinatorDraft(next);
      if (option.agentId) {
        const agentId = option.agentId;
        setMentionBindings((items) => [
          ...items.filter(
            (item) => item.token.toLocaleLowerCase() !== option.token.toLocaleLowerCase(),
          ),
          { token: option.token, agentId },
        ].slice(-100));
      }
      setMentionCursor(nextCursor);
      setMentionMenuOpen(false);
      setMentionOptionIndex(0);
      window.requestAnimationFrame(() => {
        coordinatorTextareaRef.current?.focus();
        coordinatorTextareaRef.current?.setSelectionRange(nextCursor, nextCursor);
      });
    },
    [coordinatorDraft, mentionCursor],
  );

  const toggleMentionTarget = useCallback(
    (option: MentionOption) => {
      const result = toggleMentionToken(coordinatorDraft, option.token);
      setCoordinatorDraft(result.value);
      if (option.agentId) {
        const normalizedToken = option.token.toLocaleLowerCase();
        setMentionBindings((items) =>
          result.selected
            ? [
                ...items.filter(
                  (item) => item.token.toLocaleLowerCase() !== normalizedToken,
                ),
                { token: option.token, agentId: option.agentId! },
              ].slice(-100)
            : items.filter(
                (item) => item.token.toLocaleLowerCase() !== normalizedToken,
              ),
        );
      }
      setMentionCursor(result.value.length);
      setMentionMenuOpen(false);
      setTargetMenuOpen(false);
      window.requestAnimationFrame(() => {
        coordinatorTextareaRef.current?.focus();
        coordinatorTextareaRef.current?.setSelectionRange(
          result.value.length,
          result.value.length,
        );
      });
    },
    [coordinatorDraft],
  );

  useEffect(() => {
    if (busy) {
      setTargetMenuOpen(false);
      return;
    }
    if (!targetMenuOpen) return;
    const closeTargetMenu = (event: PointerEvent) => {
      if (targetMenuRef.current?.contains(event.target as Node)) return;
      setTargetMenuOpen(false);
    };
    const closeTargetMenuOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setTargetMenuOpen(false);
    };
    window.addEventListener("pointerdown", closeTargetMenu);
    window.addEventListener("keydown", closeTargetMenuOnEscape);
    return () => {
      window.removeEventListener("pointerdown", closeTargetMenu);
      window.removeEventListener("keydown", closeTargetMenuOnEscape);
    };
  }, [busy, targetMenuOpen]);

  useEffect(() => {
    if (busy || !quickComposerActive) {
      setQuickTargetMenuOpen(false);
      return;
    }
    if (!quickTargetMenuOpen) return;
    const closeQuickTargetMenu = (event: PointerEvent) => {
      if (quickTargetMenuRef.current?.contains(event.target as Node)) return;
      setQuickTargetMenuOpen(false);
    };
    const closeQuickTargetMenuOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setQuickTargetMenuOpen(false);
    };
    window.addEventListener("pointerdown", closeQuickTargetMenu);
    window.addEventListener("keydown", closeQuickTargetMenuOnEscape);
    return () => {
      window.removeEventListener("pointerdown", closeQuickTargetMenu);
      window.removeEventListener("keydown", closeQuickTargetMenuOnEscape);
    };
  }, [busy, quickComposerActive, quickTargetMenuOpen]);

  useEffect(() => {
    if (busy) {
      setQuickModeMenuOpen(false);
      return;
    }
    if (!quickModeMenuOpen) return;
    const closeQuickModeMenu = (event: PointerEvent) => {
      if (quickModeMenuRef.current?.contains(event.target as Node)) return;
      setQuickModeMenuOpen(false);
    };
    const closeQuickModeMenuOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setQuickModeMenuOpen(false);
    };
    window.addEventListener("pointerdown", closeQuickModeMenu);
    window.addEventListener("keydown", closeQuickModeMenuOnEscape);
    return () => {
      window.removeEventListener("pointerdown", closeQuickModeMenu);
      window.removeEventListener("keydown", closeQuickModeMenuOnEscape);
    };
  }, [busy, quickModeMenuOpen]);

  useEffect(() => {
    if (!snapshot) return;
    const autoReport = autoReportProjects.current.has(snapshot.project.id);
    const directTasks = directDeliveryTasks.current.get(snapshot.project.id);
    if (!autoReport && !directTasks?.size) return;
    const waitingApproval = snapshot.tasks.filter(
      (task) =>
        autoReport &&
        task.status === "waiting_approval" &&
        reportedApprovalTasks.current.get(task.id) !== task.updatedAt,
    );
    if (waitingApproval.length) {
      waitingApproval.forEach((task) => {
        reportedApprovalTasks.current.set(task.id, task.updatedAt);
      });
      appendCoordinatorMessage(
        "assistant",
        `Agent 已提交阶段结果，等待你验收：${waitingApproval
          .map((task) => task.title)
          .join("、")}。可在下方任务列表查看结果并批准或要求修改。`,
        snapshot.project.id,
      );
      persistAutoReportState();
    }
    const failedTasks = snapshot.tasks.filter(
      (task) =>
        (autoReport || directTasks?.has(task.id)) &&
        task.status === "failed" &&
        reportedFailedTasks.current.get(task.id) !== task.updatedAt,
    );
    if (failedTasks.length) {
      failedTasks.forEach((task) => {
        reportedFailedTasks.current.set(task.id, task.updatedAt);
      });
      const details = failedTasks.map((task) => {
        const latestRun = snapshot.runs
          .filter((run) => run.taskId === task.id)
          .sort((left, right) => right.createdAt - left.createdAt)[0];
        const reason = latestRun?.error ?? latestRun?.outputSummary;
        return `- ${task.title}${reason ? `：${reason}` : ""}`;
      });
      const failureMessage = `协作任务执行失败：\n${details.join("\n")}\n可在任务列表查看日志并点击重试。`;
      appendCoordinatorMessage("assistant", failureMessage, snapshot.project.id);
      if (failedTasks.some((task) => directTasks?.has(task.id))) {
        failedCollaborationMessages.current.set(snapshot.project.id, [
          ...(failedCollaborationMessages.current.get(snapshot.project.id) ?? []),
          { role: "assistant", content: failureMessage, surface: "multi_agent" },
        ].slice(-MAX_COORDINATOR_HISTORY) as CoordinatorHistoryMessage[]);
      }
      persistAutoReportState();
    }
    for (const task of snapshot.tasks) {
      if (["done", "failed", "cancelled"].includes(task.status)) directTasks?.delete(task.id);
    }
    if (directTasks?.size === 0) directDeliveryTasks.current.delete(snapshot.project.id);
    if (autoReport && snapshot.project.status === "completed") {
      appendCoordinatorMessage(
        "assistant",
        collaborationResultMessage(snapshot),
        snapshot.project.id,
      );
      autoReportProjects.current.delete(snapshot.project.id);
      snapshot.tasks.forEach((task) => {
        reportedApprovalTasks.current.delete(task.id);
        reportedFailedTasks.current.delete(task.id);
      });
      persistAutoReportState();
    }
  }, [appendCoordinatorMessage, persistAutoReportState, snapshot]);

  const executeProjectControl = async (plan: CoordinatorPlan): Promise<string> => {
    if (!selectedProjectId) {
      return "请先生成并确认项目计划。";
    }
    const command =
      plan.intent === "pause_project"
        ? "ma_pause_project"
        : plan.intent === "cancel_project"
          ? "ma_cancel_project"
          : "ma_start_project";
    const receipt = await invoke<ProjectExecutionReceipt>(command, {
      projectId: selectedProjectId,
    });
    if (!receipt.accepted) throw new Error(receipt.message);
    const reply = `${plan.reply}\n${receipt.message}`;
    await refreshSnapshot();
    return reply;
  };

  const sendCoordinatorMessage = async (
    messageInput: string,
    bindings: MentionBinding[] = [],
    clearDraft = false,
  ): Promise<CoordinatorSendResult | null> => {
    if (pendingPlanTargetMismatch) {
      setError("请先切回计划所属项目，或取消该计划后再发送新指令。");
      return null;
    }
    if (!availabilityReady) { setError(availabilityError ?? "Runtime 启用配置尚未加载"); return null; }
    const message = messageInput.trim();
    if (!message || busyRef.current || coordinatorSending.current) return null;
    coordinatorSending.current = true;
    const sendingProjectId = selectedProjectId;
    const appendSendReply = (content: string) => {
      if (activeProjectId.current === sendingProjectId) appendCoordinatorMessage("assistant", content, sendingProjectId);
    };
    appendCoordinatorMessage("user", message);
    if (clearDraft) {
      setCoordinatorDraft("");
      setMentionBindings([]);
    }
    const sessionTargets = resolveSessionMentionTargets(message, mentionOptions, availableGridSessions);
    const hasExplicitMention = mentionOptions.some((option) =>
      hasMentionToken(message, option.token),
    );
    if (hasExplicitMention) {
      setBusy(true);
      setError(null);
      try {
        const receipt = await invoke<DirectCollaborationReceipt | null>(
          "ma_send_direct_message",
          {
            input: {
              message,
              projectId: selectedProjectId,
              targetAgentIds: resolveExplicitMentionTargetAgentIds(
                message,
                mentionOptions,
                availableSnapshot,
                bindings,
              ),
              targetSessionIds: sessionTargets.map(({ session }) => session.id),
              targetRuntimeKinds: resolveMentionTargetRuntimeKinds(message, mentionOptions),
            },
          },
        );
        if (receipt) {
          appendSendReply(receipt.reply);
          if (sendingProjectId && receipt.deliveryTaskIds?.length) {
            const tracked = directDeliveryTasks.current.get(sendingProjectId) ?? new Set<string>();
            receipt.deliveryTaskIds.forEach((id) => tracked.add(id));
            directDeliveryTasks.current.set(sendingProjectId, tracked);
          }
          await refreshSnapshot();
          coordinatorSending.current = false;
          return { ok: true, reply: receipt.reply };
        }
      } catch (reason) {
        const replyText = `定向协作失败：${String(reason)}`;
        appendSendReply(replyText);
        failedCollaborationMessages.current.set(sendingProjectId ?? "draft", [
          ...(failedCollaborationMessages.current.get(sendingProjectId ?? "draft") ?? []),
          { role: "user", content: message, surface: "multi_agent" },
          { role: "assistant", content: replyText, surface: "multi_agent" },
        ].slice(-MAX_COORDINATOR_HISTORY) as CoordinatorHistoryMessage[]);
        if (activeProjectId.current === sendingProjectId) setError(replyText);
        coordinatorSending.current = false;
        return { ok: false, reply: replyText };
      } finally {
        setBusy(false);
      }
    }
    setBusy(true);
    setError(null);
    let assistantReply = "";
    let succeeded = true;
    const reply = (content: string, projectId?: string | null) => {
      assistantReply = content;
      if (activeProjectId.current === sendingProjectId) appendCoordinatorMessage("assistant", content, projectId);
    };
    const streamRequestId = crypto.randomUUID();
    textStreamRequestRef.current = streamRequestId;
    setStreamedReply(null);
    try {
      const plan = await invoke<CoordinatorPlan>("ma_interpret_message", {
        message,
        history: coordinatorMessages
          .filter((item) => item.surface !== "quick")
          .slice(-10)
          .map(({ role, content }) => ({ role, content })),
        projectId: selectedProjectId,
        targetAgentIds: resolveExplicitMentionTargetAgentIds(
          message,
          mentionOptions,
          availableSnapshot,
          bindings,
        ),
        requestId: streamRequestId,
      });
      if (activeProjectId.current !== sendingProjectId) return { ok: false, reply: "指令已解析；项目已切换，未执行计划。" };
      if (plan.intent === "plan_project" || plan.intent === "add_tasks") {
        const nextPlan = sanitizeGeneratedPlan(plan);
        const requestedRuntimes = nextPlan.requestedRuntimes;
        const unavailableRuntimes = requestedRuntimes.filter(
          (runtime) => runtimeHealthByKind.get(runtime) === "missing",
        );
        if (unavailableRuntimes.length) {
          throw new Error(
            `以下 Agent Runtime 尚未安装：${unavailableRuntimes
              .map((runtime) => RUNTIME_LABELS[runtime])
              .join("、")}`,
          );
        }
        const generatedProjectPath =
          nextPlan.intent === "plan_project" && requestedRuntimes.length > 0
            ? nextPlan.rootPath ??
              suggestedProjectPath(projectPath || defaultCwd || "", nextPlan.projectName)
            : nextPlan.rootPath ?? projectPath;
        const nextContext = {
          projectId:
            nextPlan.intent === "add_tasks" ? selectedProjectId : null,
          projectPath:
            nextPlan.intent === "plan_project"
              ? generatedProjectPath
              : snapshot?.project.rootPath ?? projectPath,
          autoStart: autoEnabled || requestedRuntimes.length > 0,
          requestedRuntimes,
        };
        setPendingPlan(nextPlan);
        setPendingPlanContext(nextContext);
        setPlanAdvancedOpen(
          requestedRuntimes.length === 0 ||
            (nextPlan.intent === "plan_project" &&
              (!isUsablePlanText(nextPlan.projectName) ||
                !isUsablePlanText(nextPlan.goal) ||
                !generatedProjectPath.trim())),
        );
        const planReply = plan.reply;
        reply(
          planReply,
          plan.intent === "plan_project" ? null : selectedProjectId,
        );
        if (autoEnabled || planIsLowRiskCommunication(nextPlan)) {
          return await applyPendingPlan(nextPlan, nextContext);
        }
      } else if (
        plan.intent === "start_project" ||
        plan.intent === "resume_project" ||
        plan.intent === "pause_project" ||
        plan.intent === "cancel_project"
      ) {
        reply(await executeProjectControl(plan));
      } else if (plan.intent === "retry_task" || plan.intent === "reassign_task" || plan.intent === "approve_task") {
        if (!plan.targetTaskId) throw new Error("协调器没有返回目标任务 ID");
        if (plan.intent === "retry_task") {
          await invoke("ma_retry_task", { taskId: plan.targetTaskId });
          if (selectedProjectId) {
            const execution = await invoke<ProjectExecutionReceipt>("ma_start_project", { projectId: selectedProjectId });
            if (!execution.accepted) throw new Error(execution.message);
          }
        } else if (plan.intent === "approve_task") {
          await invoke("ma_approve_task", { taskId: plan.targetTaskId });
          if (selectedProjectId) {
            const execution = await invoke<ProjectExecutionReceipt>("ma_start_project", { projectId: selectedProjectId });
            if (!execution.accepted) throw new Error(execution.message);
          }
        } else {
          if (!plan.targetAgentId) throw new Error("协调器没有返回目标 Agent ID");
          await invoke("ma_reassign_task", {
            taskId: plan.targetTaskId,
            agentId: plan.targetAgentId,
          });
        }
        reply(plan.reply);
        await refreshSnapshot();
      } else if (plan.intent === "inspect_project" && snapshot) {
        const active = snapshot.tasks.filter((task) => ["claimed", "running", "in_review"].includes(task.status)).length;
        const done = snapshot.tasks.filter((task) => task.status === "done").length;
        reply(`${plan.reply}\n当前共 ${snapshot.tasks.length} 个任务，${active} 个进行中，${done} 个已完成。`);
      } else {
        reply(plan.reply);
      }
    } catch (reason) {
        succeeded = false;
        if (activeProjectId.current === sendingProjectId) setError(String(reason));
        reply(`协调失败：${String(reason)}`);
        failedCollaborationMessages.current.set(sendingProjectId ?? "draft", [
          ...(failedCollaborationMessages.current.get(sendingProjectId ?? "draft") ?? []),
          { role: "user", content: message, surface: "multi_agent" },
          { role: "assistant", content: `协调失败：${String(reason)}`, surface: "multi_agent" },
        ].slice(-MAX_COORDINATOR_HISTORY) as CoordinatorHistoryMessage[]);
    } finally {
      coordinatorSending.current = false;
      textStreamRequestRef.current = null;
      setStreamedReply(null);
      setBusy(false);
    }
    return assistantReply
      ? { ok: succeeded, reply: assistantReply }
      : null;
  };

  const sendQuickBroadcast = async () => {
    const message = stripUnifiedControlPrefix(coordinatorDraft.trim());
    if (!message || busyRef.current) return;
    setBusy(true);
    appendCoordinatorMessage("user", message, null, "quick");
    setError(null);
    try {
      const result = await onQuickBroadcast?.(message) ?? {
        ok: false,
        reply: "发送至全部会话暂时不可用。",
      };
      appendCoordinatorMessage("assistant", result.reply, null, "quick");
      if (!result.ok) setError(result.reply);
    } catch (reason) {
      const reply = `发送至全部会话失败：${String(reason)}`;
      appendCoordinatorMessage("assistant", reply, null, "quick");
      setError(reply);
    } finally {
      setBusy(false);
    }
  };

  const requestQuickPermissionMode = async (mode: PermissionMode) => {
    if (busyRef.current || quickPermissionModeBusy) return;
    setQuickModeMenuOpen(false);
    setBusy(true);
    setError(null);
    try {
      const result = await onQuickPermissionModeChange?.(mode) ?? {
        ok: false,
        reply: "统一权限模式切换暂时不可用。",
      };
      appendCoordinatorMessage("assistant", result.reply, null, "quick");
      if (!result.ok) setError(result.reply);
    } catch (reason) {
      const reply = `统一权限模式切换失败：${String(reason)}`;
      appendCoordinatorMessage("assistant", reply, null, "quick");
      setError(reply);
    } finally {
      setBusy(false);
    }
  };

  // 唯一的对话清空确认执行：快捷指挥与项目协作两个入口共用。
  const clearCoordinatorConversation = async () => {
    if (busyRef.current) return;
    setBusy(true);
    setError(null);
    try {
      const result = await onClearQuickHistory?.() ?? { ok: true };
      if (!result.ok) {
        setError(result.reply);
        return;
      }
      const projectId =
        coordinatorProjectId ?? snapshot?.project.id ?? selectedProjectId;
      if (projectId && !UI_PREVIEW) {
        await invoke<number>("ma_clear_conversation", { projectId });
      }
      setCoordinatorMessages([]);
      setCoordinatorDraft("");
      setMentionBindings([]);
      setPendingPlan(null);
      setPendingPlanContext(null);
      setPlanAdvancedOpen(false);
      setShowSystemMessages(false);
      setClearConversationConfirmOpen(false);
    } catch (reason) {
      setError(`清除对话记录失败：${String(reason)}`);
    } finally {
      setBusy(false);
    }
  };

  const applyQuickPromptCompletion = () => {
    if (!canCompleteQuickPrompt) return;
    setCoordinatorDraft(QUICK_PROMPT_COMPLETION);
    setMentionCursor(QUICK_PROMPT_COMPLETION.length);
    window.requestAnimationFrame(() => {
      coordinatorTextareaRef.current?.focus();
      coordinatorTextareaRef.current?.setSelectionRange(
        QUICK_PROMPT_COMPLETION.length,
        QUICK_PROMPT_COMPLETION.length,
      );
    });
  };

  const sendUnifiedControlMessage = async () => {
    const rawMessage = coordinatorDraft.trim();
    if (!rawMessage || busyRef.current) return;
    const route = resolveUnifiedControlRoute(
      rawMessage,
      selectedMentionAgentIds.length,
    );
    const message = stripUnifiedControlPrefix(rawMessage);
    if (route === "multi_agent") {
      await sendCoordinatorMessage(message, mentionBindings, true);
      return;
    }

    setBusy(true);
    appendCoordinatorMessage("user", message, null, "quick");
    setCoordinatorDraft("");
    setMentionBindings([]);
    setError(null);
    try {
      const result = await onQuickCommand?.(message);
      const reply = result?.reply || "快速指挥暂时不可用。";
      appendCoordinatorMessage("assistant", reply, null, "quick");
      if (!result?.ok) setError(reply);
    } catch (reason) {
      const reply = `快速指挥失败：${String(reason)}`;
      appendCoordinatorMessage("assistant", reply, null, "quick");
      setError(reply);
    } finally {
      setBusy(false);
    }
  };

  const runPromptOptimization = async () => {
    if (optimizing || busyRef.current) return;
    if (!isRuntimeEnabled("claude_code")) {
      setOptimizationError(availabilityError ?? "请先启用 Claude Code 再优化提示词");
      return;
    }
    const invalid = validatePromptDraft(coordinatorDraft);
    if (invalid) {
      setOptimizationError(invalid);
      return;
    }
    if (UI_PREVIEW) {
      setOptimization(PROMPT_OPTIMIZATION_PREVIEW);
      setOptimizationError(null);
      return;
    }
    setOptimizing(true);
    setOptimizationError(null);
    const request = ++optimizationRequest.current;
    const inputs = optimizationInputs.current;
    const isCurrent = () => request === optimizationRequest.current
      && optimizationInputs.current.draft === inputs.draft
      && optimizationInputs.current.projectId === inputs.projectId;
    try {
      const context = snapshot
        ? `项目：${snapshot.project.name}（目标：${snapshot.project.goal}）`
        : null;
      const result = await invoke<PromptOptimization>("optimize_prompt", {
        draft: coordinatorDraft,
        context,
      });
      if (isCurrent()) setOptimization(result);
    } catch (reason) {
      if (isCurrent()) setOptimizationError(String(reason));
    } finally {
      setOptimizing(false);
    }
  };

  const submitCoordinator = (event: FormEvent) => {
    event.preventDefault();
    if (quickComposerActive && quickSendTarget === "all") {
      void sendQuickBroadcast();
      return;
    }
    void sendUnifiedControlMessage();
  };

  const applyPendingPlan = async (
    planOverride?: CoordinatorPlan,
    contextOverride?: PendingPlanContext,
  ): Promise<ControlCommandResult> => {
    const planToApply = planOverride ?? pendingPlan;
    const contextForApply = contextOverride ?? pendingPlanContext;
    if (!planToApply || applyingPlan.current || (busyRef.current && planOverride === undefined)) {
      return { ok: false, reply: "当前没有可确认的协作计划。" };
    }
    if (!availabilityReady || planToApply.agents.some((agent) => !isRuntimeEnabled(agent.runtimeKind))) {
      const reply = availabilityError ?? "计划包含未启用的 Runtime，请调整后再确认。"; setError(reply); return { ok: false, reply };
    }
    const createsProject = planToApply.intent === "plan_project";
    const targetProjectId = createsProject
      ? null
      : contextForApply?.projectId ?? null;
    if (
      createsProject &&
      (!isUsablePlanText(planToApply.projectName) ||
        !isUsablePlanText(planToApply.goal))
    ) {
      const reply = "新项目计划需要填写有效的项目名称和目标。";
      setError(reply);
      return { ok: false, reply };
    }
    if (!createsProject && targetProjectId !== selectedProjectId) {
      const reply = "该计划属于另一个协作项目，请切回原项目后再确认。";
      setError(reply);
      return { ok: false, reply };
    }
    const rootPath =
      contextForApply?.projectPath || projectPath || defaultCwd || "";
    if (!rootPath.trim()) {
      const reply = "请先填写项目目录。";
      setError(reply);
      return { ok: false, reply };
    }
    applyingPlan.current = true;
    const requestedProjectId = selectedProjectId;
    setBusy(true);
    setError(null);
    try {
      const autoStart = contextForApply?.autoStart === true;
      const collaborationAgents = (contextForApply?.requestedRuntimes ?? [])
        .map((runtime) => RUNTIME_LABELS[runtime])
        .join("、");
      const receipt = await invoke<ApplyCoordinatorPlanReceipt>("ma_apply_plan", {
        input: {
          projectId: targetProjectId,
          rootPath,
          plan: planToApply,
          conversation: coordinatorMessages,
        },
      });
      if (activeProjectId.current !== requestedProjectId) {
        setPendingPlan(current => current === planToApply ? null : current);
        setPendingPlanContext(current => current === contextForApply ? null : current);
        await refreshProjects();
        return { ok: false, reply: "计划已创建；当前项目已切换，未自动启动。请在原项目中查看已创建的任务。" };
      }
      setPendingPlan(null);
      setPendingPlanContext(null);
      setSelectedProjectId(receipt.projectId);
      activeProjectId.current = receipt.projectId;
      if (autoEnabled) {
        try { window.localStorage.setItem(`crewvela-collaboration-auto:${receipt.projectId}`, "true"); } catch { /* Current panel still tracks this choice. */ }
        setAutoProjectId(receipt.projectId);
      }
      if (autoStart) {
        autoReportProjects.current.add(receipt.projectId);
        persistAutoReportState();
      }
      let refreshWarning = "";
      try {
        await refreshProjects();
      } catch (refreshReason) {
        refreshWarning = `项目已创建，但刷新项目列表失败：${String(refreshReason)}`;
        setError(refreshWarning);
      }
      if (activeProjectId.current !== receipt.projectId) return { ok: false, reply: "计划已创建；当前项目已切换，未自动启动。" };
      let executionMessage = "";
      if (autoStart) {
        try {
          const execution = await invoke<ProjectExecutionReceipt>("ma_start_project", {
            projectId: receipt.projectId,
          });
          if (!execution.accepted) throw new Error(execution.message);
          executionMessage = `\n${execution.message}`;
        } catch (startReason) {
          try {
            const next = await invoke<MultiAgentSnapshot>("ma_snapshot", { projectId: receipt.projectId });
            if (activeProjectId.current === receipt.projectId) setSnapshot(next);
          } catch {
            // 轮询会继续刷新，保留已创建的项目即可。
          }
          const message = `计划已创建，但自动启动失败：${String(startReason)}。项目已保留，可点击“启动”重试。${refreshWarning ? `\n${refreshWarning}` : ""}`;
          if (activeProjectId.current === receipt.projectId) { setError(message); appendCoordinatorMessage("assistant", message, receipt.projectId); }
          return { ok: false, reply: message };
        }
      }
      try {
        const next = await invoke<MultiAgentSnapshot>("ma_snapshot", { projectId: receipt.projectId });
        if (activeProjectId.current === receipt.projectId) setSnapshot(next);
      } catch (snapshotReason) {
        if (activeProjectId.current === receipt.projectId) setError(`项目已创建${autoStart ? "并启动" : ""}，但刷新状态失败：${String(snapshotReason)}`);
      }
      const reply = autoStart
        ? `计划已落地并启动：新增 ${receipt.createdAgents} 个 Agent、${receipt.createdTasks} 个任务。${collaborationAgents || "参与 Agent"} 的执行过程已在协作工作台中打开。${executionMessage}${refreshWarning ? `\n${refreshWarning}` : ""}`
        : `计划已落地：新增 ${receipt.createdAgents} 个 Agent、${receipt.createdTasks} 个任务。你可以说“启动项目”，或点击启动。${refreshWarning ? `\n${refreshWarning}` : ""}`;
      if (activeProjectId.current === receipt.projectId) appendCoordinatorMessage("assistant", reply, receipt.projectId);
      return { ok: true, reply };
    } catch (reason) {
      const reply = `创建协作计划失败：${String(reason)}`;
      if (activeProjectId.current === requestedProjectId) { setError(String(reason)); appendCoordinatorMessage("assistant", reply); }
      return { ok: false, reply };
    } finally {
      applyingPlan.current = false;
      setBusy(false);
    }
  };


  const approveTask = async (taskId: string) => {
    if (busyRef.current) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("ma_approve_task", { taskId });
      if (selectedProjectId) {
        const execution = await invoke<ProjectExecutionReceipt>("ma_start_project", { projectId: selectedProjectId });
        if (!execution.accepted) throw new Error(execution.message);
      }
      await refreshSnapshot();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const controlProject = async (intent: "start_project" | "pause_project") => {
    if (busyRef.current || !snapshot) return;
    setBusy(true); setError(null);
    const projectId = snapshot.project.id;
    try {
      const receipt = await invoke<ProjectExecutionReceipt>(intent === "start_project" ? "ma_start_project" : "ma_pause_project", { projectId });
      if (!receipt.accepted) throw new Error(receipt.message);
      if (activeProjectId.current === projectId) { appendCoordinatorMessage("assistant", receipt.message, projectId); await refreshSnapshot(); }
    } catch (reason) { if (activeProjectId.current === projectId) setError(String(reason)); }
    finally { setBusy(false); }
  };

  useEffect(() => {
    if (!active || !autoEnabled || !snapshot || snapshot.project.id !== selectedProjectId || busy || autoApproving.current || UI_PREVIEW) return;
    if (snapshot.project.status === "paused" || snapshot.project.status === "cancelled") return;
    const attemptKey = (id: string) => `${id}:${snapshot.runs.filter((run) => run.taskId === id).sort((a, b) => b.attempt - a.attempt)[0]?.id}`;
    const ids = autoApprovalTaskIds(snapshot).filter((id) => !autoApprovalAttempts.current.has(attemptKey(id)));
    if (!ids.length) return;
    const projectId = snapshot.project.id;
    autoApproving.current = true;
    void (async () => {
      try {
        for (const taskId of ids) {
          if (!automationContext.current.enabled || automationContext.current.projectId !== projectId) return;
          autoApprovalAttempts.current.add(attemptKey(taskId));
          await invoke("ma_approve_task", { taskId });
        }
        if (!automationContext.current.enabled || automationContext.current.projectId !== projectId) return;
        // Integrator merge can take time. Respect a pause made while approval was running.
        const latest = await invoke<MultiAgentSnapshot>("ma_snapshot", { projectId });
        if (!canAutoContinueProject(latest, projectId) || !automationContext.current.enabled
          || automationContext.current.projectId !== projectId) return;
        const execution = await invoke<ProjectExecutionReceipt>("ma_start_project", { projectId });
        if (!execution.accepted) throw new Error(execution.message);
        await refreshSnapshot();
      } catch (reason) {
        if (automationContext.current.projectId === projectId) setError(`Auto 批准继续失败：${String(reason)}。请查看任务后手动处理，或重新切换 Auto 重试。`);
      } finally { autoApproving.current = false; }
    })();
  }, [active, autoEnabled, snapshot, selectedProjectId, busy]);

  const cancelPendingPlan = () => {
    if (busyRef.current) return;
    setPendingPlan(null);
    setPendingPlanContext(null);
    setPlanAdvancedOpen(false);
    setError(null);
    if (snapshot) {
      setCoordinatorMessages((items) => [
        ...items.filter((item) => item.surface === "quick"),
        ...coordinatorMessagesFromSnapshot(snapshot),
      ].slice(-MAX_COORDINATOR_HISTORY));
      setCoordinatorProjectId(snapshot.project.id);
    }
  };

  const pendingPlanNeedsProjectMetadata =
    pendingPlan?.intent === "plan_project";
  const pendingPlanMetadataValid =
    !pendingPlanNeedsProjectMetadata ||
    (isUsablePlanText(pendingPlan?.projectName) &&
      isUsablePlanText(pendingPlan?.goal));
  const pendingPlanPathValid =
    !pendingPlanNeedsProjectMetadata ||
    Boolean(pendingPlanContext?.projectPath.trim());
  const pendingPlanTargetMismatch =
    pendingPlan?.intent === "add_tasks" &&
    (!pendingPlanContext?.projectId ||
      pendingPlanContext.projectId !== selectedProjectId);
  const pendingPlanValid =
    pendingPlanMetadataValid &&
    pendingPlanPathValid &&
    !pendingPlanTargetMismatch;
  const pendingPlanPermissions = pendingPlan
    ? [...new Set(pendingPlan.agents.map((agent) => agent.permissionProfile))]
        .map((profile) =>
          profile === "yolo"
            ? "YOLO"
            : profile === "auto"
              ? "Auto"
              : "Default",
        )
        .join(" / ")
    : "";
  const taskStatusRows = buildTaskStatusPresentations(snapshot);
  const taskProgressSummary = buildTaskProgressSummary(snapshot);
  const compactTaskProgressSummary = taskProgressSummary.replace(
    " · 已完成",
    " 完成",
  );
  const taskProgressNeedsAttention = shouldExpandTaskProgress(snapshot);
  const approvalTask =
    snapshot?.tasks.find((task) => task.status === "in_review") ?? null;
  const approvalAgent = approvalTask
    ? snapshot?.agents.find(
        (agent) => agent.id === approvalTask.assigneeAgentId,
      ) ?? null
    : null;
  const collaborationRoleSummary = useMemo(() => {
    const agents = availableSnapshot?.agents ?? pendingPlan?.agents.filter((agent) => isRuntimeEnabled(agent.runtimeKind)) ?? [];
    const labels = agents
      .filter((agent) => !("enabled" in agent) || agent.enabled)
      .slice(0, 2)
      .map((agent) => {
        const roleLabel =
          agent.role === "reviewer"
            ? "审核"
            : agent.role === "integrator"
              ? "集成"
              : agent.role === "orchestrator"
                ? "协调"
                : "开发";
        return `${agent.name} ${roleLabel}`;
      });
    return labels.length > 0 ? labels.join(" · ") : "等待分配 Agent";
  }, [pendingPlan?.agents, availableSnapshot?.agents, runtimePreferences]);
  const collaborationProjectName =
    snapshot?.project.name ?? pendingPlan?.projectName ?? "未选择项目";

  return (
    <div className="multi-agent-control" data-team-view={collaborationView}>
      <nav className="team-message-tabs" aria-label="协作视图">
        <button type="button" aria-pressed={collaborationView === "tasks"} onClick={() => setCollaborationView("tasks")}>任务指令</button>
        <button type="button" aria-pressed={collaborationView === "messages"} onClick={() => setCollaborationView("messages")}>团队消息</button>
      </nav>
      <section
        className={`multi-agent-coordinator${pendingPlan ? " pending-plan" : ""}`}
      >
          {clearConversationConfirmOpen && (
            <div
              ref={clearConfirmRef}
              className="multi-agent-clear-confirm"
              role="alertdialog"
              aria-label="确认清空对话记录：将清空快捷指挥与协作对话，Agent 与任务不受影响"
              title="将清空快捷指挥与项目协作对话；不会停止 Agent，也不会删除项目、任务或执行记录"
              tabIndex={-1}
              onKeyDown={(event) => {
                if (event.key === "Escape") setClearConversationConfirmOpen(false);
              }}
            >
              <i className="clear-confirm-icon" aria-hidden="true">
                <TrashIcon size={14} />
              </i>
              <p className="clear-confirm-text">清空对话？</p>
              <div className="clear-confirm-actions">
                <button
                  type="button"
                  onClick={() => setClearConversationConfirmOpen(false)}
                >
                  取消
                </button>
                <button
                  className="danger"
                  type="button"
                  disabled={busy}
                  onClick={clearCoordinatorConversation}
                >
                  确认
                </button>
              </div>
            </div>
          )}
          <div className="multi-agent-coordinator-messages">
          {visibleCoordinatorMessages.map((message, index) => {
            const messageKey = message.role + "-" + index;
            const copied = copiedMessageKey === messageKey;
            return (
              <article
                key={messageKey}
                className={"multi-agent-message " + message.role + " route-" + (message.surface ?? "multi_agent")}
              >
                <p className="multi-agent-message-content">
                  <span>
                    {message.role === "user"
                      ? message.surface === "quick"
                        ? "你 · 快速"
                        : "你 · 协作"
                      : message.surface === "quick"
                        ? "总控 · 快速"
                        : "总控 · 协作"}
                  </span>
                  {message.content}
                </p>
                <div className="multi-agent-message-actions">
                  <button
                    className={"multi-agent-message-copy" + (copied ? " copied" : "")}
                    type="button"
                    aria-label={copied ? "消息已复制" : "复制这条消息"}
                    onClick={() => void copyCoordinatorMessage(messageKey, message.content)}
                  >
                    {copied ? <CheckIcon size={13} weight="bold" /> : <CopyIcon size={13} />}
                  </button>
                </div>
              </article>
            );
          })}
          {streamedReply && (
            <article className="multi-agent-message assistant">
              <p className="multi-agent-message-content">
                <span>总控 · 协作</span>
                {streamedReply.text}
              </p>
            </article>
          )}
          {busy && !streamedReply && (
            <article className="multi-agent-message assistant thinking">
              <p className="multi-agent-message-content">
                <span>总控 · 协作</span>
                正在思考，可能需要一分钟…（消息已显示，正在处理协作指令）
              </p>
            </article>
          )}
          {approvalTask && (
            <section className="multi-agent-inline-approval">
              <span className="eyebrow">WAITING FOR YOU</span>
              <strong>等待你的确认</strong>
              <p>
                {approvalAgent?.name ?? "Agent"} 已完成“{approvalTask.title}”，批准后继续后续任务。
              </p>
              <div>
                {snapshot && (
                  <button
                    type="button"
                    onClick={() => requestOpenWorkspace(snapshot.project.id)}
                  >
                    <ArrowSquareOutIcon size={13} />
                    查看变更
                  </button>
                )}
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => approveTask(approvalTask.id)}
                >
                  批准继续
                </button>
              </div>
            </section>
          )}
        </div>
        {(workspaceTrustStatus?.awaitingCount ?? 0) > 0 && (
          <button
            className="multi-agent-trust-notice"
            type="button"
            disabled={workspaceTrustBusy || busy}
            onClick={() => void onConfirmWorkspaceTrust?.()}
          >
            <span>
              {workspaceTrustStatus?.awaitingCount} 个新会话等待确认工作区信任
              {(workspaceTrustStatus?.startingCount ?? 0) > 0 &&
                ` · ${workspaceTrustStatus?.startingCount} 个正在启动`}
            </span>
            <kbd>Enter</kbd>
          </button>
        )}
        <form className="multi-agent-composer" onSubmit={submitCoordinator}>
          {optimization && (
            <div className="prompt-optimization-card" role="dialog" aria-label="提示词优化结果">
              <header>
                <strong>优化建议</strong>
                {optimization.notes && <small>{optimization.notes}</small>}
                <button
                  type="button"
                  className="prompt-optimization-dismiss"
                  aria-label="关闭优化建议"
                  onClick={() => setOptimization(null)}
                >
                  <TrashIcon size={13} />
                </button>
              </header>
              <pre>{optimization.optimized}</pre>
              <footer>
                <button type="button" onClick={() => setOptimization(null)}>
                  保留原稿
                </button>
                <button
                  type="button"
                  className="primary"
                  onClick={() => {
                    setCoordinatorDraft(optimization.optimized);
                    setOptimization(null);
                  }}
                >
                  <CheckIcon size={13} weight="bold" /> 采纳
                </button>
              </footer>
            </div>
          )}
          <div className="multi-agent-composer-field">
            <div className="multi-agent-composer-editor">
              <div
                ref={coordinatorHighlightRef}
                className="multi-agent-composer-highlight"
                aria-hidden="true"
              >
                {coordinatorDraftSegments.map((segment, index) =>
                  segment.kind ? (
                    <mark
                      className={segment.kind}
                      key={`${segment.kind}-${index}`}
                    >
                      {segment.text}
                    </mark>
                  ) : (
                    <span key={`text-${index}`}>{segment.text}</span>
                  ),
                )}
                {coordinatorDraft.endsWith("\n") && "\u00a0"}
              </div>
              <textarea
                ref={coordinatorTextareaRef}
              role="combobox"
              aria-autocomplete="list"
              aria-haspopup="listbox"
              aria-expanded={Boolean(activeMention && filteredMentionOptions.length)}
              aria-controls={
                activeMention && filteredMentionOptions.length
                  ? mentionMenuId
                  : undefined
              }
              aria-activedescendant={
                activeMention && filteredMentionOptions.length
                  ? `${mentionMenuId}-option-${Math.min(
                      mentionOptionIndex,
                      filteredMentionOptions.length - 1,
                    )}`
                  : undefined
              }
              value={coordinatorDraft}
              placeholder="输入指令；使用 @ 或选择目标进入项目协作……"
              disabled={busy}
              onChange={(event) => {
                const cursor = event.currentTarget.selectionStart;
                setCoordinatorDraft(event.target.value);
                setMentionCursor(cursor);
                setMentionMenuOpen(
                  activeMentionAt(event.target.value, cursor) !== null,
                );
                setMentionOptionIndex(0);
              }}
              onFocus={() => setCoordinatorFocused(true)}
              onSelect={(event) => {
                const cursor = event.currentTarget.selectionStart;
                if (cursor === mentionCursor) return;
                setMentionCursor(cursor);
                setMentionMenuOpen(
                  activeMentionAt(event.currentTarget.value, cursor) !== null,
                );
              }}
              onBlur={() => {
                setCoordinatorFocused(false);
                setMentionMenuOpen(false);
              }}
              onScroll={(event) => {
                if (!coordinatorHighlightRef.current) return;
                coordinatorHighlightRef.current.scrollTop = event.currentTarget.scrollTop;
                coordinatorHighlightRef.current.scrollLeft = event.currentTarget.scrollLeft;
              }}
              onKeyDown={(event) => {
                if (activeMention && filteredMentionOptions.length) {
                  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                    event.preventDefault();
                    const direction = event.key === "ArrowDown" ? 1 : -1;
                    setMentionOptionIndex(
                      (current) =>
                        (current + direction + filteredMentionOptions.length) %
                        filteredMentionOptions.length,
                    );
                    return;
                  }
                  if (event.key === "Tab" || (event.key === "Enter" && !event.shiftKey)) {
                    event.preventDefault();
                    insertMention(
                      filteredMentionOptions[
                        Math.min(mentionOptionIndex, filteredMentionOptions.length - 1)
                      ],
                    );
                    return;
                  }
                  if (event.key === "Escape") {
                    event.preventDefault();
                    event.stopPropagation();
                    setMentionMenuOpen(false);
                    return;
                  }
                }
                if (
                  event.key === "Tab" &&
                  !event.shiftKey &&
                  !event.ctrlKey &&
                  !event.altKey &&
                  !event.metaKey &&
                  canCompleteQuickPrompt
                ) {
                  event.preventDefault();
                  applyQuickPromptCompletion();
                  return;
                }
                if (
                  event.key === "Enter" &&
                  (event.ctrlKey || event.metaKey) &&
                  event.shiftKey &&
                  quickComposerActive
                ) {
                  event.preventDefault();
                  void sendQuickBroadcast();
                  return;
                }
                if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
                  event.preventDefault();
                  if (
                    !coordinatorDraft.trim() &&
                    (workspaceTrustStatus?.awaitingCount ?? 0) > 0 &&
                    !workspaceTrustBusy
                  ) {
                    void onConfirmWorkspaceTrust?.();
                    return;
                  }
                  event.currentTarget.form?.requestSubmit();
                }
              }}
              />
              {canCompleteQuickPrompt && (
                <button
                  className="multi-agent-quick-completion"
                  type="button"
                  aria-label={`补全为：${QUICK_PROMPT_COMPLETION}`}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={applyQuickPromptCompletion}
                >
                  Tab 补全
                  <span>{quickCompletionSuffix}</span>
                </button>
              )}
            </div>
            {activeMention && filteredMentionOptions.length > 0 && (
              <AgentMentionMenu
                id={mentionMenuId}
                options={filteredMentionOptions}
                activeIndex={Math.min(mentionOptionIndex, filteredMentionOptions.length - 1)}
                onHighlight={setMentionOptionIndex}
                onSelect={insertMention}
              />
            )}
            <div className="multi-agent-composer-footer">
              <button type="button" className="multi-agent-target-trigger"
                aria-label="协作自动确认模式" aria-pressed={autoEnabled}
                disabled={busy || autoApproving.current}
                title="面板打开时自动确认计划和成功交付；独立审查、权限审批和失败处理仍保留"
                onClick={() => changeCollaborationAuto(!autoEnabled)}>
                {autoEnabled ? "Auto" : "手动"}
              </button>
              <div className="multi-agent-target-picker" ref={targetMenuRef}>
              <button
                className="multi-agent-target-trigger"
                type="button"
                disabled={busy}
                aria-expanded={targetMenuOpen}
                aria-haspopup="menu"
                aria-label="快捷指令与协作目标"
                title="快捷指令与协作目标"
                onClick={() => {
                  setMentionMenuOpen(false);
                  setQuickTargetMenuOpen(false);
                  setQuickModeMenuOpen(false);
                  setTargetMenuOpen((open) => !open);
                }}
              >
                <PlusIcon size={13} weight="bold" />
              </button>
              {quickComposerActive ? (
                <div
                  className="multi-agent-quick-target-control"
                  ref={quickTargetMenuRef}
                >
                  <button
                    className="multi-agent-quick-target-trigger"
                    type="button"
                    aria-label="快捷指挥发送目标"
                    aria-haspopup="menu"
                    aria-expanded={quickTargetMenuOpen}
                    disabled={busy}
                    onClick={() => {
                      setTargetMenuOpen(false);
                      setQuickModeMenuOpen(false);
                      setQuickTargetMenuOpen((open) => !open);
                    }}
                  >
                    {quickSendTarget === "control" ? (
                      <RobotIcon size={14} weight="regular" aria-hidden="true" />
                    ) : (
                      <BroadcastIcon size={14} weight="regular" aria-hidden="true" />
                    )}
                    <span>{quickSendTarget === "control" ? "总控" : "选择会话"}</span>
                    <CaretDownIcon size={11} aria-hidden="true" />
                  </button>
                  {quickTargetMenuOpen && (
                    <div
                      className="multi-agent-quick-target-menu"
                      role="menu"
                      aria-label="选择快捷指挥发送目标"
                    >
                      <button
                        type="button"
                        role="menuitemradio"
                        aria-checked={quickSendTarget === "control"}
                        className={quickSendTarget === "control" ? "selected" : ""}
                        onClick={() => {
                          setQuickSendTarget("control");
                          setQuickTargetMenuOpen(false);
                          coordinatorTextareaRef.current?.focus();
                        }}
                      >
                        <i><RobotIcon size={17} weight="regular" /></i>
                        <span>
                          <strong>总控</strong>
                          <small>由总控理解并执行</small>
                        </span>
                        {quickSendTarget === "control" && <CheckIcon size={14} weight="bold" />}
                      </button>
                      <button
                        type="button"
                        role="menuitemradio"
                        aria-checked={quickSendTarget === "all"}
                        className={quickSendTarget === "all" ? "selected" : ""}
                        onClick={() => {
                          setQuickSendTarget("all");
                          setQuickTargetMenuOpen(false);
                          coordinatorTextareaRef.current?.focus();
                        }}
                      >
                        <i><BroadcastIcon size={17} weight="regular" /></i>
                        <span>
                          <strong>选择会话</strong>
                          <small>打开快捷指令，核对范围与目标</small>
                        </span>
                        {quickSendTarget === "all" && <CheckIcon size={14} weight="bold" />}
                      </button>
                    </div>
                  )}
                </div>
              ) : (
                <span className="multi-agent-target-summary">
                  {selectedMentionTargetCount > 0
                    ? `项目协作 · ${selectedMentionTargetCount} 个目标`
                    : "项目协作"}
                </span>
              )}
              {targetMenuOpen && (
                <div
                  className="multi-agent-target-menu"
                  role="menu"
                  aria-label="快捷指令与协作目标"
                >
                  <header>
                    <strong>快捷指令与协作目标</strong>
                    <small>执行常用操作，或选择群组和具体 Agent</small>
                  </header>
                  <div className="multi-agent-quick-command-section">
                    <span>快捷指令</span>
                    <button type="button" role="menuitem" disabled={!onOpenQuickCommands} onClick={() => { setTargetMenuOpen(false); onOpenQuickCommands?.(); }}>
                      搜索与配置快捷指令
                    </button>
                    <button
                      className="danger"
                      type="button"
                      role="menuitem"
                      onClick={() => {
                        setTargetMenuOpen(false);
                        setClearConversationConfirmOpen(true);
                      }}
                    >
                      <TrashIcon size={13} />
                      清除对话记录
                    </button>
                  </div>
                  {(["群组", "具体 Agent"] as const).map((section) => {
                    const sectionOptions = mentionOptions.filter(
                      (option) => option.section === section,
                    );
                    if (!sectionOptions.length) return null;
                    return (
                      <div className="multi-agent-target-section" key={section}>
                        <span>{section}</span>
                        {sectionOptions.map((option) => {
                          const selected = hasMentionToken(
                            coordinatorDraft,
                            option.token,
                          );
                          return (
                            <button
                              type="button"
                              role="option"
                              disabled={busy}
                              aria-selected={selected}
                              className={selected ? "selected" : ""}
                              key={option.key}
                              onClick={() => toggleMentionTarget(option)}
                            >
                              <i className={option.runtimeKind ? "runtime-logo-frame" : undefined}>
                                {option.runtimeKind ? (
                                  <AgentAvatar avatarId={option.avatarId} runtimeKind={option.runtimeKind} decorative />
                                ) : (
                                  <UsersThreeIcon size={14} />
                                )}
                              </i>
                              <span>
                                <strong>{option.token}</strong>
                                <small>{option.description}</small>
                              </span>
                              {selected ? (
                                <CheckIcon size={14} weight="bold" />
                              ) : typeof option.targetCount === "number" ? (
                                <em>{option.targetCount}</em>
                              ) : null}
                            </button>
                          );
                        })}
                      </div>
                    );
                  })}
                </div>
              )}
              </div>
              {quickComposerActive && (
                <div className="multi-agent-quick-mode-control" ref={quickModeMenuRef}>
                  <button
                    className="multi-agent-quick-mode-trigger"
                    type="button"
                    aria-label="统一权限模式"
                    aria-haspopup="menu"
                    aria-expanded={quickModeMenuOpen}
                    disabled={
                      busy ||
                      quickPermissionModeBusy ||
                      quickLiveSessionCount === 0 ||
                      !quickModesKnown ||
                      !quickModesSwitchable
                    }
                    title={
                      quickLiveSessionCount === 0
                        ? "当前没有运行中的 Claude Code 会话"
                        : !quickModesKnown
                          ? `等待会话上报权限模式（${quickKnownModeCount}/${quickLiveSessionCount}）`
                          : !quickModesSwitchable
                            ? "存在无法统一切换权限模式的会话"
                            : `统一切换 ${quickLiveSessionCount} 个运行会话`
                    }
                    onClick={() => {
                      setTargetMenuOpen(false);
                      setQuickTargetMenuOpen(false);
                      setQuickModeMenuOpen((open) => !open);
                    }}
                  >
                    <span>{quickPermissionModeLabel}</span>
                    <CaretDownIcon size={12} aria-hidden="true" />
                  </button>
                  {quickModeMenuOpen && (
                    <div className="multi-agent-quick-mode-menu" role="menu">
                      <button
                        type="button"
                        role="menuitem"
                        className={quickPermissionMode === "mixed" ? "selected" : ""}
                        disabled
                      >
                        <span>Mixed</span>
                        {quickPermissionMode === "mixed" && <CheckIcon size={13} />}
                      </button>
                      {QUICK_PERMISSION_MODES.map((mode) => (
                        <button
                          type="button"
                          role="menuitem"
                          className={quickPermissionMode === mode.value ? "selected" : ""}
                          key={mode.value}
                          onClick={() => void requestQuickPermissionMode(mode.value)}
                        >
                          <span>{mode.label}</span>
                          {quickPermissionMode === mode.value && <CheckIcon size={13} />}
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              )}
              <div className="multi-agent-composer-actions">
                <button
              className={`multi-agent-optimize-trigger${optimizing ? " busy" : ""}`}
              type="button"
              disabled={!isRuntimeEnabled("claude_code") || busy || optimizing || !coordinatorDraft.trim()}
              aria-label="优化提示词"
              title={optimizing ? "正在优化提示词…" : "优化提示词：让 AI 把这条指令改写得更清晰完整"}
              onClick={() => void runPromptOptimization()}
            >
              <SparkleIcon size={15} weight={optimizing ? "fill" : "regular"} />
                </button>
                <button
              className="multi-agent-send"
              disabled={
                busy ||
                !coordinatorDraft.trim() ||
                pendingPlanTargetMismatch ||
                (quickComposerActive &&
                  quickSendTarget === "all" &&
                  !onQuickBroadcast)
              }
              aria-label={
                quickComposerActive && quickSendTarget === "all"
                  ? "核对目标后发送消息"
                  : selectedMentionTargetCount > 0
                  ? `发送给 ${selectedMentionTargetCount} 个 Agent`
                  : "发送协作指令"
              }
              title={
                quickComposerActive && quickSendTarget === "all"
                  ? "核对目标后发送消息"
                  : selectedMentionTargetCount > 0
                  ? `发送给 ${selectedMentionTargetCount} 个 Agent`
                  : "发送协作指令"
              }
            >
                  <PaperPlaneRightIcon size={15} weight="fill" />
                </button>
              </div>
            </div>
          </div>
        </form>
      </section>

      {pendingPlan && (
        <section className="multi-agent-plan-card">
          <header>
            <div>
              <span className="eyebrow">PLAN PREVIEW</span>
              <strong>
                {pendingPlan.projectName ?? snapshot?.project.name ?? "追加任务"}
              </strong>
            </div>
            <span className={pendingPlanContext?.autoStart ? "auto" : ""}>
              {pendingPlanContext?.autoStart ? "确认后启动" : "确认后创建"}
            </span>
          </header>
          <div className="multi-agent-plan-overview">
            <article className="wide">
              <span>Agent 分工</span>
              <div className="multi-agent-plan-participants">
                {pendingPlan.agents.length > 0 ? pendingPlan.agents.map((agent) => (
                  <strong key={agent.key}>
                    {agent.name} · {RUNTIME_LABELS[agent.runtimeKind]} · {ROLE_LABELS[agent.role]}
                  </strong>
                )) : (
                  <strong>按现有团队执行</strong>
                )}
              </div>
            </article>
            <article>
              <span>权限模式</span>
              <strong>{pendingPlanPermissions || "Default"}</strong>
            </article>
            <article>
              <span>任务规模</span>
              <strong>{pendingPlan.agents.length} Agent · {pendingPlan.tasks.length} 任务</strong>
            </article>
            <article className="wide">
              <span>项目目录</span>
              <code>{pendingPlanContext?.projectPath || snapshot?.project.rootPath || "待填写"}</code>
            </article>
          </div>
          <p className="multi-agent-plan-goal">
            {pendingPlan.goal ?? pendingPlan.reply}
          </p>
          <div className="multi-agent-plan-task-preview">
            {pendingPlan.tasks.slice(0, 4).map((task) => {
              const assignee = pendingPlan.agents.find(
                (agent) => agent.key === task.assigneeKey,
              );
              return (
                <span key={task.key}>
                  {task.title} → {assignee?.name ?? "未指派"}
                </span>
              );
            })}
            {pendingPlan.tasks.length > 4 && (
              <span>另有 {pendingPlan.tasks.length - 4} 个任务</span>
            )}
          </div>
          <details
            className="multi-agent-plan-advanced"
            open={planAdvancedOpen}
            onToggle={(event) =>
              setPlanAdvancedOpen(event.currentTarget.open)
            }
          >
            <summary>
              高级调整：项目、角色、权限、任务与依赖
            </summary>
          <fieldset disabled={busy} style={{ border: 0, padding: 0, margin: 0, minWidth: 0 }}>
          {pendingPlanNeedsProjectMetadata && (
            <div className="multi-agent-plan-metadata-editor">
              <input
                className="multi-agent-plan-project-name"
                aria-label="协作项目名称"
                value={pendingPlan.projectName ?? ""}
                placeholder="填写项目名称"
                onChange={(event) =>
                  setPendingPlan((plan) =>
                    plan ? { ...plan, projectName: event.target.value } : plan,
                  )
                }
              />
              <textarea
                className="multi-agent-plan-project-goal"
                aria-label="协作项目目标"
                value={pendingPlan.goal ?? ""}
                placeholder="填写项目目标和验收要求"
                onChange={(event) =>
                  setPendingPlan((plan) =>
                    plan ? { ...plan, goal: event.target.value } : plan,
                  )
                }
              />
              <input
                className="multi-agent-plan-project-path"
                aria-label="协作项目目录"
                value={pendingPlanContext?.projectPath ?? ""}
                placeholder="填写新项目的绝对路径"
                onChange={(event) =>
                  setPendingPlanContext((context) => ({
                    ...context,
                    projectId: context?.projectId ?? null,
                    projectPath: event.target.value,
                  }))
                }
              />
            </div>
          )}
          <div className="multi-agent-plan-agents">
            {pendingPlan.agents.map((agent, agentIndex) => (
              <div key={agent.key}>
                <span>{RUNTIME_LABELS[agent.runtimeKind]}</span>
                <select
                  aria-label={`${agent.name} 角色`}
                  value={agent.role}
                  onChange={(event) => setPendingPlan((plan) => plan ? {
                    ...plan,
                    agents: plan.agents.map((item, index) => index === agentIndex ? {
                      ...item, role: event.target.value as AgentRole,
                    } : item),
                  } : plan)}
                >
                  {Object.entries(ROLE_LABELS).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
                </select>
                <select
                  aria-label={`${agent.name} 权限`}
                  value={agent.permissionProfile}
                  onChange={(event) => setPendingPlan((plan) => plan ? {
                    ...plan,
                    agents: plan.agents.map((item, index) => index === agentIndex ? {
                      ...item, permissionProfile: event.target.value as "default" | "auto" | "yolo",
                    } : item),
                  } : plan)}
                >
                  <option value="default">Default</option>
                  <option value="auto">Auto</option>
                  <option value="yolo">YOLO</option>
                </select>
              </div>
            ))}
          </div>
          <ol>
            {pendingPlan.tasks.map((task, taskIndex) => (
              <li key={task.key}>
                <div className="multi-agent-plan-task-row">
                  <input
                    type="number"
                    min={0}
                    max={99}
                    aria-label={`任务 ${taskIndex + 1} Stage`}
                    value={task.stage}
                    onChange={(event) => setPendingPlan((plan) => plan ? {
                      ...plan,
                      tasks: plan.tasks.map((item, index) => index === taskIndex ? { ...item, stage: Number(event.target.value) } : item),
                    } : plan)}
                  />
                  <input
                    aria-label={`任务 ${taskIndex + 1} 标题`}
                    value={task.title}
                    onChange={(event) => setPendingPlan((plan) => plan ? {
                      ...plan,
                      tasks: plan.tasks.map((item, index) => index === taskIndex ? { ...item, title: event.target.value } : item),
                    } : plan)}
                  />
                  <select
                    aria-label={`任务 ${taskIndex + 1} 执行 Agent`}
                    value={task.assigneeKey}
                    onChange={(event) => setPendingPlan((plan) => plan ? {
                      ...plan,
                      tasks: plan.tasks.map((item, index) => index === taskIndex ? { ...item, assigneeKey: event.target.value } : item),
                    } : plan)}
                  >
                    {pendingPlan.agents.map((agent) => <option key={agent.key} value={agent.key}>{agent.name}</option>)}
                  </select>
                </div>
                <textarea
                  aria-label={`任务 ${taskIndex + 1} 说明`}
                  value={task.description}
                  onChange={(event) => setPendingPlan((plan) => plan ? {
                    ...plan,
                    tasks: plan.tasks.map((item, index) => index === taskIndex ? { ...item, description: event.target.value } : item),
                  } : plan)}
                />
                <input
                  aria-label={`任务 ${taskIndex + 1} 验收命令`}
                  value={task.verificationCommand ?? ""}
                  placeholder="验收命令；留空则需要人工批准"
                  onChange={(event) => setPendingPlan((plan) => plan ? {
                    ...plan,
                    tasks: plan.tasks.map((item, index) => index === taskIndex ? {
                      ...item,
                      verificationCommand: event.target.value.trim() ? event.target.value : null,
                    } : item),
                  } : plan)}
                />
                <div className="multi-agent-plan-task-row secondary">
                  <select
                    aria-label={`任务 ${taskIndex + 1} 工作区模式`}
                    value={task.workspaceMode}
                    onChange={(event) => setPendingPlan((plan) => plan ? {
                      ...plan,
                      tasks: plan.tasks.map((item, index) => index === taskIndex ? {
                        ...item, workspaceMode: event.target.value as WorkspaceMode,
                      } : item),
                    } : plan)}
                  >
                    <option value="worktree">独立 Worktree</option>
                    <option value="read_only">只读共享</option>
                    <option value="shared_write">共享写入</option>
                  </select>
                  <input
                    aria-label={`任务 ${taskIndex + 1} 依赖`}
                    value={task.dependsOn.join(", ")}
                    placeholder="依赖 key，逗号分隔"
                    onChange={(event) => setPendingPlan((plan) => plan ? {
                      ...plan,
                      tasks: plan.tasks.map((item, index) => index === taskIndex ? {
                        ...item,
                        dependsOn: event.target.value.split(",").map((value) => value.trim()).filter(Boolean),
                      } : item),
                    } : plan)}
                  />
                </div>
              </li>
            ))}
          </ol>
          </fieldset>
          </details>
          {!pendingPlanMetadataValid && (
            <p className="multi-agent-plan-validation">
              生成结果缺少有效的项目名称或目标，请补充后再确认。
            </p>
          )}
          {!pendingPlanPathValid && (
            <p className="multi-agent-plan-validation">
              请填写新项目的工作区目录。
            </p>
          )}
          {pendingPlanTargetMismatch && (
            <p className="multi-agent-plan-validation">
              该计划已绑定到生成时选择的协作项目，请切回原项目后再确认。
            </p>
          )}
          <footer>
            <button
              type="button"
              disabled={busy}
              onClick={cancelPendingPlan}
            >
              取消
            </button>
            <button
              type="button"
              disabled={busy || !pendingPlanValid}
              onClick={() => void applyPendingPlan()}
            >
              {pendingPlanContext?.autoStart ? "确认并启动" : "确认创建"}
            </button>
          </footer>
        </section>
      )}

      {error && <div className="multi-agent-error">{error}</div>}

      <details ref={detailDrawerRef} className="multi-agent-detail-drawer">
        <summary aria-label="打开协作上下文">
          <span className="multi-agent-context-summary">
            <PushPinIcon size={14} weight="fill" />
            <b>上下文</b>
          </span>
          <span className="multi-agent-context-action">
            <CaretDownIcon className="multi-agent-detail-caret" size={13} />
          </span>
        </summary>
        <div className="multi-agent-detail-content compact">
          <div className="multi-agent-context-overview">
            <div>
              <small>当前项目</small>
              <strong title={collaborationProjectName}>{collaborationProjectName}</strong>
            </div>
            <div>
              <small>协作角色</small>
              <strong title={collaborationRoleSummary}>{collaborationRoleSummary}</strong>
            </div>
            <div>
              <small>进度</small>
              <strong className={taskProgressNeedsAttention ? "needs-attention" : ""}>
                {taskStatusRows.length > 0 ? compactTaskProgressSummary : "尚无任务"}
              </strong>
            </div>
          </div>
          <div className="multi-agent-detail-actions">
            {snapshot && (
              <button
                type="button"
                onClick={() => requestOpenWorkspace(snapshot.project.id)}
              >
                <ArrowSquareOutIcon size={13} />
                打开协作详情
              </button>
            )}
            <details className="multi-agent-context-operations">
              <summary>
                项目操作
                <CaretDownIcon size={12} />
              </summary>
              <div>
                {snapshot && (
                  <>
                    <button type="button" disabled={busy} onClick={() => void controlProject("start_project")}>启动</button>
                    <button type="button" disabled={busy} onClick={() => void controlProject("pause_project")}>暂停</button>
                  </>
                )}
                <button
                  className="multi-agent-clear-conversation"
                  type="button"
                  aria-expanded={clearConversationConfirmOpen}
                  disabled={
                    busy ||
                    (!coordinatorMessages.length && !pendingPlan && !coordinatorDraft)
                  }
                  onClick={() =>
                    setClearConversationConfirmOpen((visible) => !visible)
                  }
                >
                  <TrashIcon size={13} />
                  清除会话
                </button>
              </div>
            </details>
          </div>
          <div className="multi-agent-runtime-strip" aria-label="Agent Runtime 状态">
            {runtimes.map((runtime) => {
              const health = getRuntimeHealth(runtime);
              const label = RUNTIME_LABELS[runtime.kind] ?? runtime.kind;
              const detail = runtime.error ?? runtime.version ?? runtime.protocol;
              return (
                <span
                  key={runtime.kind}
                  className={health}
                  title={detail}
                  aria-label={`${label}：${detail}`}
                >
                  <i />
                  {label}
                  {health === "warning" && <em>异常</em>}
                  {health === "missing" && <em>缺失</em>}
                </span>
              );
            })}
          </div>
          {groupedSystemActivities.length > 0 && (
            <section className="multi-agent-section multi-agent-system-activity">
              <header>
                <div>
                  <strong>自动操作记录</strong>
                  <span>{systemActivityCount} 项</span>
                </div>
                <button
                  type="button"
                  onClick={() => setShowSystemMessages((visible) => !visible)}
                >
                  {showSystemMessages ? "在对话中隐藏" : "在对话中显示"}
                </button>
              </header>
              <div>
                {groupedSystemActivities.map((activity, index) => (
                  <article key={`${activity.content}-${index}`}>
                    <span className="task-status done" />
                    <p>{activity.content}</p>
                    {activity.count > 1 && <em>×{activity.count}</em>}
                  </article>
                ))}
              </div>
            </section>
          )}

      {projects.length > 0 && (
        <label className="multi-agent-project-picker">
          <span>协作项目</span>
          <select
            value={selectedProjectId ?? ""}
            onChange={(event) => { activeProjectId.current = event.target.value || null; snapshotRequest.current += 1; setSelectedProjectId(event.target.value || null); }}
          >
            {projects.map((project) => (
              <option key={project.id} value={project.id}>
                {project.name}
              </option>
            ))}
          </select>
        </label>
      )}

      {!snapshot ? (
        <>
          <section className="multi-agent-conversation-empty">
            <span className="eyebrow">CONVERSATION FIRST</span>
            <strong>直接描述目标即可开始</strong>
            <p>项目名称、目录、Agent 分工、任务依赖和验证方式会自动生成，并在执行前集中确认。</p>
          </section>
          <details className="multi-agent-manual-panel">
            <summary>
              <span>高级编排</span>
              <small>手动创建项目</small>
            </summary>
            <form className="multi-agent-onboarding" onSubmit={createProject}>
              <span className="eyebrow">MANUAL PROJECT</span>
              <h3>手动创建协作项目</h3>
              <p>仅在需要完全自定义项目元数据时使用。</p>
              <input
                value={projectName}
                placeholder="项目名称"
                disabled={busy}
                onChange={(event) => setProjectName(event.target.value)}
              />
              <textarea
                value={projectGoal}
                placeholder="项目目标和验收要求"
                disabled={busy}
                onChange={(event) => setProjectGoal(event.target.value)}
              />
              <input
                value={projectPath}
                placeholder="项目绝对路径"
                disabled={busy}
                onChange={(event) => setProjectPath(event.target.value)}
              />
              <button disabled={busy || !projectName.trim() || !projectGoal.trim() || !projectPath.trim()}>
                创建项目
              </button>
            </form>
          </details>
        </>
      ) : (
        <>
          <section className="multi-agent-project-summary">
            <div>
              <span className="eyebrow">ACTIVE PROJECT</span>
              <h3>{snapshot.project.name}</h3>
              <p>{snapshot.project.goal}</p>
            </div>
            <small title={snapshot.project.rootPath}>{snapshot.project.rootPath}</small>
          </section>

          <section className="multi-agent-section">
            <header>
              <div>
                <strong>团队</strong>
                <span>{availableAgents.length} 个 Agent</span>
              </div>
            </header>
            <div className="multi-agent-team-list">
              {availableAgents.map((agent) => {
                const runtimeHealth =
                  runtimeHealthByKind.get(agent.runtimeKind) ?? "missing";
                return (
                  <article key={agent.id}>
                    <span
                      className={`runtime-mark ${
                        runtimeHealth === "installed" ? "ready" : runtimeHealth
                      }`}
                      title={
                        runtimeHealth === "warning"
                          ? "Runtime 已安装，但状态探测异常"
                          : runtimeHealth === "missing"
                            ? "Runtime 未安装"
                            : "Runtime 可用"
                      }
                    />
                    <div>
                      <strong>{agent.name}</strong>
                      <small>
                        {RUNTIME_LABELS[agent.runtimeKind]} · {ROLE_LABELS[agent.role]}
                      </small>
                    </div>
                  </article>
                );
              })}
              {availableAgents.length === 0 && <p>先创建团队，再分配任务。</p>}
            </div>
          </section>

          <section className="multi-agent-section multi-agent-tasks">
            <header>
              <div>
                <strong>任务图</strong>
                <span>{snapshot.tasks.length} 个任务</span>
              </div>
            </header>
            {snapshot.runs.filter(run => run.sessionLocator?.stopUnverified === true).map(run => <UnverifiedRunNotice key={run.id} run={run} taskTitle={snapshot.tasks.find(task => task.id === run.taskId)?.title} onConfirmed={refreshSnapshot} />)}
            <div className="multi-agent-task-list">
              {snapshot.tasks.map((task) => {
                const assignee = snapshot.agents.find(
                  (agent) => agent.id === task.assigneeAgentId,
                );
                const latestRun = snapshot.runs.find((run) => run.taskId === task.id);
                const stopUnverified = snapshot.runs.some(run => run.taskId === task.id && run.sessionLocator?.stopUnverified === true);
                return (
                  <article key={task.id}>
                    <span className={`task-status ${task.status}`} />
                    <div>
                      <strong>{task.title}</strong>
                      <small>
                        Stage {task.stage} · {assignee?.name ?? "未指派"} · {task.status}
                      </small>
                      {(latestRun?.error || latestRun?.outputSummary) && (
                        <em className={latestRun.error ? "error" : "result"} title={latestRun.error ?? latestRun.outputSummary ?? ""}>
                          {latestRun.error ?? latestRun.outputSummary}
                        </em>
                      )}
                    </div>
                    {task.status === "in_review" && (
                      <button type="button" disabled={busy} onClick={() => approveTask(task.id)}>
                        {assignee?.role === "integrator" ? "批准并合并" : "批准"}
                      </button>
                    )}
                    {["failed", "blocked", "cancelled", "changes_requested"].includes(task.status) && (
                      <button type="button" disabled={stopUnverified || busy} onClick={async () => {
                        if (busyRef.current) return;
                        setBusy(true);
                        try {
                          await invoke("ma_retry_task", { taskId: task.id });
                          if (selectedProjectId) { const execution = await invoke<ProjectExecutionReceipt>("ma_start_project", { projectId: selectedProjectId }); if (!execution.accepted) throw new Error(execution.message); }
                          await refreshSnapshot();
                        } catch (reason) { setError(`重试失败：${String(reason)}`); }
                        finally { setBusy(false); }
                      }}>重试</button>
                    )}
                  </article>
                );
              })}
              {snapshot.tasks.length === 0 && <p>尚未创建任务。</p>}
            </div>
          </section>

          <details className="multi-agent-manual-panel">
            <summary>
              <span>高级编排</span>
              <small>手动创建 Agent 与任务</small>
            </summary>
            <div className="multi-agent-manual-actions">
              <div>
                <strong>团队配置</strong>
                <span>自动计划不需要手动初始化；这里保留兼容入口。</span>
              </div>
              {availableAgents.length === 0 ? (
                <button type="button" disabled={busy} onClick={createDefaultTeam}>
                  初始化六 Agent 团队
                </button>
              ) : (
                <span>{availableAgents.length} 个 Agent 已就绪</span>
              )}
            </div>
            <form className="multi-agent-task-form" onSubmit={createTask}>
              <span className="eyebrow">MANUAL TASK</span>
              <input
                value={taskTitle}
                placeholder="任务标题"
                disabled={busy}
                onChange={(event) => setTaskTitle(event.target.value)}
              />
              <textarea
                value={taskDescription}
                placeholder="任务说明"
                disabled={busy}
                onChange={(event) => setTaskDescription(event.target.value)}
              />
              <div>
                <label>
                  Stage
                  <input
                    type="number"
                    min={0}
                    max={99}
                    value={taskStage}
                    onChange={(event) => setTaskStage(Number(event.target.value))}
                  />
                </label>
                <select
                  value={taskAssigneeId}
                  onChange={(event) => setTaskAssigneeId(event.target.value)}
                >
                  <option value="">未指派</option>
                  {availableAgents.map((agent) => (
                    <option key={agent.id} value={agent.id}>
                      {agent.name}
                    </option>
                  ))}
                </select>
                <select
                  value={taskWorkspaceMode}
                  onChange={(event) =>
                    setTaskWorkspaceMode(event.target.value as WorkspaceMode)
                  }
                >
                  <option value="worktree">独立 Worktree</option>
                  <option value="read_only">只读共享</option>
                  <option value="shared_write">共享写入</option>
                </select>
              </div>
              <button disabled={busy || !taskTitle.trim()}>添加任务</button>
            </form>
          </details>

          <details className="multi-agent-runtime-details">
            <summary>
              <span>运行详情</span>
              <small>{snapshot.events.length} 条活动记录</small>
            </summary>
            <section className="multi-agent-section multi-agent-activity">
              <div>
                {snapshot.events.slice(-8).reverse().map((event) => (
                  <p key={event.seq}>
                    <span>{event.eventType}</span>
                    <time>{new Date(event.createdAt).toLocaleTimeString()}</time>
                  </p>
                ))}
              </div>
            </section>
          </details>
        </>
      )}
        </div>
      </details>
      {collaborationView === "messages" && <GroupChatPanel
        key={selectedProjectId ?? "no-project"}
        projectId={selectedProjectId}
        active={active}
        sessions={sessions}
        previewSnapshot={UI_PREVIEW ? TEAM_MESSAGE_PREVIEW.snapshot : undefined}
        previewEvidence={UI_PREVIEW ? TEAM_MESSAGE_PREVIEW.evidence : undefined}
      />}
    </div>
  );
}
