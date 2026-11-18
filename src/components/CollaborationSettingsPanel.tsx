import { CollaborationRecordsPanel } from "./CollaborationRecordsPanel";
import { createPortal } from "react-dom";
import { CollaborationTaskDetails } from "./CollaborationTaskDetails";
import { UnverifiedRunNotice } from "./UnverifiedRunNotice";
import { visibleProjectTasks, type CollaborationTasksRequest } from "../collaborationTasks";
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { CollaborationAgentEditor, type AgentDraft } from "./CollaborationAgentEditor";
import { CollaborationAgentRemovalDialog } from "./CollaborationAgentRemovalDialog";
import { CollaborationProjectDialog } from "./CollaborationProjectDialog";
import { SettingsSection } from "./ui/SettingsSection";
import { Modal } from "./ui/Modal";
import { activityLabel, diagnosticChecks, summarizeDiagnosticChecks, HANDOFF_LABELS, taskMatchesFilter, taskPriority } from "../collaborationDiagnostics";
import type { Health } from "./CollaborationSupportPanel";
import { CollaborationSupportPanel } from "./CollaborationSupportPanel";
import { CommunicationGuideButton } from "./CommunicationGuide";
import { KnowledgeBaseDialog } from "./KnowledgeBaseDialog";
import { ArrowSquareOutIcon } from "@phosphor-icons/react/dist/csr/ArrowSquareOut";
import { ArrowClockwiseIcon } from "@phosphor-icons/react/dist/csr/ArrowClockwise";
import { CaretRightIcon } from "@phosphor-icons/react/dist/csr/CaretRight";
import { DotsThreeIcon } from "@phosphor-icons/react/dist/csr/DotsThree";
import { AgentAvatar } from "./AgentAvatar";
import { AgentCard } from "./AgentCard";
import { RuntimeManagementDialog } from "./RuntimeManagementDialog";
import { useRuntimeAvailability } from "../runtimeAvailability";
import { RUNTIME_LOGOS } from "../runtimeLogos";
import { groupTasksByStage } from "../multiAgentTaskPresentation";
import { useRuntimeOrder } from "../runtimePreferences";
import { COLLABORATION_RUNTIMES } from "../runtimeTaxonomy";
import "../runtimeUpdates.css";
import "../styles/collaboration-overview.css";
import "../styles/collaboration-runtime-order.css";
import "../styles/collaboration-task-first.css";
import type {
  AgentPersona,
  AgentProfile,
  AgentRole,
  AgentRuntimeKind,
  MultiAgentProject,
  MultiAgentSnapshot,
  MultiAgentTaskStatus,
  KnowledgeSearchResult,
  RuntimeProbe,
  RuntimeUpgradeReceipt,
} from "../types";

interface Props {
  onClose: () => void;
  initialTab?: "overview" | "diagnostics" | "records";
  initialDiagnosticGlobal?: boolean;
  initialProjectId?: string;
  taskRequest?: CollaborationTasksRequest;
}

interface SuggestedSkillSet {
  id: string;
  name: string;
  description: string;
  projectPath: string | null;
  agentRole: AgentRole | null;
  items: Array<{ runtime: AgentRuntimeKind }>;
}

const UI_PREVIEW =
  new URLSearchParams(window.location.search).get("ui-preview") === "1";
const PROJECT_SELECTION_EVENT = "crewvela-multi-agent-project-selected";
const PROJECT_STORAGE_KEY = "crewvela-multi-agent-project";

/** 跨面板共享的项目选中动作（画布启动项目后跳回协作页时复用）。 */
export function selectCollaborationProject(projectId: string | null): void {
  if (!projectId) return;
  try {
    window.localStorage.setItem(PROJECT_STORAGE_KEY, projectId);
  } catch {
    // 当前窗口内仍可继续切换。
  }
  window.dispatchEvent(
    new CustomEvent(PROJECT_SELECTION_EVENT, { detail: { projectId } }),
  );
}

const RUNTIME_ORDER: AgentRuntimeKind[] = [
  "claude_code",
  "codex",
  "kimi_code",
  "pi",
  "opencode",
  "deepseek_harness",
  "claude_chat",
  "codex_chat",
  "kimi_chat",
  "opencode_chat",
  "pi_chat",
  "zcode",
  "zcode_cli",
];

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

const ROLE_LABELS: Record<AgentRole, string> = {
  orchestrator: "总控",
  worker: "执行",
  reviewer: "审查",
  integrator: "集成",
};

const TASK_STATUS_LABELS: Record<MultiAgentTaskStatus, string> = {
  draft: "草稿",
  ready: "待开始",
  blocked: "已阻塞",
  claimed: "已领取",
  running: "执行中",
  waiting_approval: "待批准",
  in_review: "待审核",
  changes_requested: "需修改",
  done: "已完成",
  failed: "失败",
  cancelled: "已取消",
};

const previewTime = (time: string) => Date.parse("2026-09-30T" + time + ":00+08:00");
const PREVIEW_PROJECT: MultiAgentProject = { id: "preview-collaboration-project", name: "从 0 到 1 · Vibecoding 项目", goal: "完善跨运行时协作通信", rootPath: "C:/example/crewvela", defaultBranch: "kimicode", status: "active", createdAt: previewTime("13:41"), updatedAt: previewTime("14:08") };
const previewNames = ["Claude 架构师", "Codex 前端工程师", "Kimi 后端工程师", "Pi 审查工程师", "OpenCode 设计师", "ZCode CLI 工程师", "DeepSeek Harness 工程师", "ZCode Desktop 工程师"];
const previewPersonas: AgentPersona[] = ["rigorous", "efficient", "efficient", "rigorous", "innovative", "concise", "efficient", "efficient"];
const previewDescriptions = [
  "负责产品调研与架构设计，输出 docs/ 下的决策文档",
  "负责前端页面与交互实现",
  "负责服务端、数据模型与接口实现",
  "只读审查前后端代码，产出问题清单与整改建议",
  "负责界面视觉与交互设计，产出可实施的设计说明",
  "负责跨运行时脚手架与工具链脚本",
  "负责 Harness 协议适配与回放验证",
  "负责桌面端集成与面板联调",
];
const previewTaskData: Array<[string, number, number, MultiAgentTaskStatus, string]> = [
 ["需求拆解与技术方案",0,0,"done","13:42"], ["实现会话路由与目标校验",1,1,"running","14:08"],
 ["接入运行时通信桥",1,2,"in_review","14:02"], ["补齐消息投递回归",1,3,"ready","13:56"],
 ["跨运行时兼容性审查",2,4,"ready","13:50"], ["交付检查与集成",2,6,"ready","13:48"],
];
export const COLLABORATION_UI_PREVIEW_SNAPSHOT: MultiAgentSnapshot = {
 project: PREVIEW_PROJECT,
 agents: COLLABORATION_RUNTIMES.map((runtimeKind, i) => ({
 id: "preview-agent-" + i, projectId: PREVIEW_PROJECT.id, name: previewNames[i] ?? `${RUNTIME_LABELS[runtimeKind]} 工程师`, runtimeKind,
 role: i === 0 ? "orchestrator" : i === 3 ? "reviewer" : i === 7 ? "integrator" : "worker",
 description: previewDescriptions[i] ?? "", persona: previewPersonas[i] ?? "efficient", personaCustom: "",
 model: null, permissionProfile: "default", maxConcurrency: 1, capabilities: [], enabled: true,
 createdAt: PREVIEW_PROJECT.createdAt, updatedAt: PREVIEW_PROJECT.updatedAt,
 })),
 tasks: previewTaskData.map(([title, stage, owner, status, time], i) => ({
 id: "preview-task-" + i, projectId: PREVIEW_PROJECT.id, parentId: null, stage, title, description: title,
 acceptanceCriteria: "通过相关验证并提供交付结果", verificationCommand: null, status,
 assigneeAgentId: "preview-agent-" + owner, reviewerAgentId: null, workspaceMode: "worktree",
 priority: 1, requiredCapabilities: [], maxAttempts: 2, retryBackoffSeconds: 5, nextRetryAt: null,
 createdAt: PREVIEW_PROJECT.createdAt, updatedAt: previewTime(time),
 })),
 runs: [], wakeups: [], messages: [], artifacts: [], approvals: [], dependencies: [], agentSessions: [],
 runLogs: [], instructions: [], runtimeEvents: [], memories: [], reviewDecisions: [],
 events: [["14:08","Codex 开始执行会话路由任务","running"],["14:02","Kimi 提交交付，等待审核","in_review"],
 ["13:58","Claude → Pi：消息已提交","done"],["13:56","Pi 消息排队中","ready"]].map(([time,title,tone],i) => ({
 seq:i,projectId:PREVIEW_PROJECT.id,entityType:i < 2 ? "task" : "message",entityId:i < 2 ? "preview-task-" + (i + 1) : "preview-message-" + i,eventType:title,actorType:"agent",actorId:null,payload:{tone},createdAt:previewTime(time)
 })),
};

const PREVIEW_RUNTIMES: RuntimeProbe[] = COLLABORATION_RUNTIMES.map((kind) => ({
  kind,
  installed: true,
  executable: kind,
  version: kind === "codex" ? "codex-cli 0.153.1" : kind === "kimi_code" ? "0.36.1" : "1.18.16",
  protocol: "local",
  capabilities: [],
  error: null,
  latestVersion: kind === "codex" ? "0.153.3" : kind === "kimi_code" ? "0.41.0" : "1.18.16",
  updateAvailable: kind === "codex" || kind === "kimi_code",
  updateStatus: kind === "codex" || kind === "kimi_code" ? "available" : "up_to_date",
  updateCommand: kind === "kimi_code" ? "kimi upgrade" : "codex update",
  canAutoUpdate: kind !== "kimi_code",
}));

function formatDateTime(value: number): string {
  if (!value) return "—";
  return new Intl.DateTimeFormat("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false }).format(new Date(value));
}

function projectStatusLabel(project: MultiAgentProject): string {
  if (project.status === "active") return "运行中";
  if (project.status === "paused") return "已暂停";
  if (project.status === "completed") return "已完成";
  if (project.status === "cancelled") return "已取消";
  return "草稿";
}

function runtimeVersion(value: string | null | undefined): string {
  return value?.match(/\d+(?:\.\d+)+/)?.[0] ?? value ?? "—";
}

export function CollaborationSettingsPanel({ onClose, initialTab = "overview", initialDiagnosticGlobal = false, initialProjectId, taskRequest }: Props) {
  const [projects, setProjects] = useState<MultiAgentProject[]>(
    UI_PREVIEW ? [PREVIEW_PROJECT] : [],
  );
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(() => {
    if (UI_PREVIEW) return PREVIEW_PROJECT.id;
    try {
      return initialProjectId ?? window.localStorage.getItem(PROJECT_STORAGE_KEY);
    } catch {
      return null;
    }
  });
  const [snapshot, setSnapshot] = useState<MultiAgentSnapshot | null>(
    UI_PREVIEW ? COLLABORATION_UI_PREVIEW_SNAPSHOT : null,
  );
  const [runtimes, setRuntimes] = useState<RuntimeProbe[]>(
    UI_PREVIEW ? PREVIEW_RUNTIMES : [],
  );
  const [error, setError] = useState<string | null>(null);
  const [loadingErrors, setLoadingErrors] = useState<Record<string, string>>({});
  const [projectsLoading, setProjectsLoading] = useState(!UI_PREVIEW);
  const [creatingProject, setCreatingProject] = useState(false);
  const [skillSets, setSkillSets] = useState<SuggestedSkillSet[]>([]);
  const [checkingRuntimes, setCheckingRuntimes] = useState(false);
  const pendingUpgrades = useRef(new Set<AgentRuntimeKind>());
  const [upgradingRuntimes, setUpgradingRuntimes] = useState(new Set<AgentRuntimeKind>());
  const [upgradeResults, setUpgradeResults] = useState<Partial<Record<AgentRuntimeKind, { error: boolean; message: string }>>>({});
  const [approvingTaskId, setApprovingTaskId] = useState<string | null>(null);
  const taskInFlight = useRef(false);
  const projectInFlight = useRef(false);
  const runtimeCheckSequence = useRef(0);
  const runtimeCheckInFlight = useRef(false);

  const [editingAgent, setEditingAgent] = useState<AgentProfile | null | undefined>(undefined);
  const [removingAgent, setRemovingAgent] = useState<AgentProfile | null>(null);
  const memberMenuGroup = useId();
  const [editorProjectId, setEditorProjectId] = useState<string | null>(null);
  const [teamRoleFilter, setTeamRoleFilter] = useState<AgentRole | "all">("all");
  const [teamSearch, setTeamSearch] = useState("");

  const [tab, setTab] = useState<string>(initialTab);
  const [health, setHealth] = useState<Health | null>(null);
  const [filter, setFilter] = useState("attention");
  const [collapsed, setCollapsed] = useState(new Set<number>());
  const [handoffRequest, setHandoffRequest] = useState<{ nonce: number; taskId?: string; recordId?: string } | null>(null);
  const [projectBusy, setProjectBusy] = useState(false);
  const availability = useRuntimeAvailability();
  const [managingRuntimes, setManagingRuntimes] = useState(false);
  const [runtimeExpanded, setRuntimeExpanded] = useState(false);
  const runtimeContentId = useId();
  const [detail, setDetail] = useState<{ title: string; content: string } | null>(null);
  const [taskDetail, setTaskDetail] = useState<{ projectId: string; taskId: string } | null>(null);
  const taskPanel = useRef<HTMLElement>(null);
  const appliedTaskRequest = useRef("");
  const focusedTaskRequest = useRef("");
  const currentSnapshot = snapshot?.project.id === selectedProjectId && projects.some((project) => project.id === selectedProjectId) ? snapshot : null;
  const taskSnapshot = currentSnapshot ? { ...currentSnapshot, tasks: visibleProjectTasks(currentSnapshot.tasks) } : null;
  const taskNavigationBlocked = editingAgent !== undefined || creatingProject || managingRuntimes || handoffRequest !== null;
  useEffect(() => { setTaskDetail(null); }, [selectedProjectId]);
  useEffect(() => {
    if (!taskRequest || taskNavigationBlocked || currentSnapshot?.project.id !== taskRequest.projectId) return;
    const key = `${taskRequest.projectId}:${taskRequest.requestId}`;
    if (appliedTaskRequest.current === key) return;
    appliedTaskRequest.current = key;
    setTaskDetail(null); setDetail(null); setTab("overview"); setFilter("all"); setCollapsed(new Set());
  }, [taskRequest, currentSnapshot, taskNavigationBlocked]);
  useEffect(() => {
    if (!taskRequest || taskNavigationBlocked || currentSnapshot?.project.id !== taskRequest.projectId || tab !== "overview" || filter !== "all") return;
    const key = `${taskRequest.projectId}:${taskRequest.requestId}`;
    if (appliedTaskRequest.current !== key || focusedTaskRequest.current === key) return;
    const frame = requestAnimationFrame(() => {
      if (!taskPanel.current) return;
      taskPanel.current.scrollIntoView({ block: "start", behavior: "auto" });
      taskPanel.current.focus({ preventScroll: true });
      focusedTaskRequest.current = key;
    });
    return () => cancelAnimationFrame(frame);
  }, [taskRequest, currentSnapshot?.project.id, taskNavigationBlocked, tab, filter]);
  const requestHandoff = (taskId?: string, recordId?: string) => setHandoffRequest({ nonce: Date.now(), taskId, recordId });
  const pauseProject = async () => {
    if (!currentSnapshot || projectInFlight.current) return;
    projectInFlight.current = true;
    const projectId = currentSnapshot.project.id;
    setProjectBusy(true);
    try {
      const status = currentSnapshot.project.status === "active" ? "paused" : "active";
      if (UI_PREVIEW) setSnapshot({ ...currentSnapshot, project: { ...currentSnapshot.project, status } });
      else {
        const receipt = await invoke<{ accepted: boolean; message: string }>(status === "paused" ? "ma_pause_project" : "ma_start_project", { projectId: currentSnapshot.project.id });
        if (!receipt.accepted) throw new Error(receipt.message);
        await refreshSnapshot();
      }
    } catch (reason) { if (selectionRef.current === projectId) setError(String(reason)); }
    finally { projectInFlight.current = false; setProjectBusy(false); }
  };
  const checks = health ? diagnosticChecks(health) : [];
  const diagnosticIssues = checks.filter((check) => check.state === "异常").map((check) => check.label);
  const checkSummary = summarizeDiagnosticChecks(checks);
  const selectionRef = useRef(selectedProjectId);
  const snapshotRequest = useRef(0);
  const selectProject = useCallback((projectId: string | null) => {
    if (selectionRef.current !== projectId) {
      selectionRef.current = projectId;
      snapshotRequest.current += 1;
      setRemovingAgent(null);
      setSelectedProjectId(projectId);
    }
    selectCollaborationProject(projectId);
  }, []);

  useEffect(() => {
    if (UI_PREVIEW) return;
    let cancelled = false;
    setProjectsLoading(true);
    setLoadingErrors({});
    // Project content must not wait for CLI discovery, network updates or skill scans.
    void invoke<MultiAgentProject[]>("ma_list_projects")
      .then((nextProjects) => {
        if (cancelled) return;
        setProjects(nextProjects);
        const storedProjectId = (() => {
          try {
            return initialProjectId ?? window.localStorage.getItem(PROJECT_STORAGE_KEY);
          } catch {
            return null;
          }
        })();
        const nextProjectId =
          storedProjectId &&
          nextProjects.some((project) => project.id === storedProjectId)
            ? storedProjectId
            : initialProjectId ? null : nextProjects[0]?.id ?? null;
        if (initialProjectId && !nextProjects.some(project => project.id === initialProjectId)) setError("请求的项目已不可用，请明确选择其他项目。");
        selectProject(nextProjectId);
      })
      .catch((reason) => {
        if (!cancelled) setError(String(reason));
      })
      .finally(() => {
        if (!cancelled) setProjectsLoading(false);
      });
    let firstFrame = 0, backgroundFrame = 0;
    const reportBackgroundError = (source: string, reason: unknown) => {
      if (!cancelled) setLoadingErrors(current => ({ ...current, [source]: String(reason) }));
    };
    firstFrame = requestAnimationFrame(() => {
      backgroundFrame = requestAnimationFrame(() => {
        if (cancelled) return;
        setCheckingRuntimes(true);
        runtimeCheckInFlight.current = true;
        const checkSequence = ++runtimeCheckSequence.current;
        void invoke<RuntimeProbe[]>("ma_probe_runtimes")
          .then(next => { if (!cancelled && checkSequence === runtimeCheckSequence.current) setRuntimes(next); })
          .catch(reason => reportBackgroundError("运行环境检测", reason))
          .then(() => {
            if (cancelled || checkSequence !== runtimeCheckSequence.current) return;
            return invoke<RuntimeProbe[]>("ma_check_runtime_updates")
              .then(next => { if (!cancelled && checkSequence === runtimeCheckSequence.current) setRuntimes(next); })
              .catch(reason => reportBackgroundError("更新检查", reason));
          })
          .finally(() => { if (!cancelled && checkSequence === runtimeCheckSequence.current) { runtimeCheckInFlight.current = false; setCheckingRuntimes(false); } });
        void invoke<{ skillSets: SuggestedSkillSet[] }>("skill_center_snapshot")
          .then(next => { if (!cancelled) setSkillSets(next.skillSets); })
          .catch(reason => reportBackgroundError("Skill 配置", reason));
      });
    });
    return () => {
      cancelled = true;
      cancelAnimationFrame(firstFrame); cancelAnimationFrame(backgroundFrame);
      snapshotRequest.current += 1;
      runtimeCheckSequence.current += 1;
    };
  }, [selectProject, initialProjectId]);

  // A repeated deep link must still win after a manual project selection, even
  // when initialProjectId itself has not changed. Do not replay consumed links.
  useEffect(() => {
    if (!taskRequest || taskNavigationBlocked || projectsLoading || appliedTaskRequest.current === `${taskRequest.projectId}:${taskRequest.requestId}`) return;
    if (!projects.some(project => project.id === taskRequest.projectId)) {
      if (selectionRef.current !== null) selectProject(null);
      setError("请求的项目已不可用，请明确选择其他项目。");
      return;
    }
    setError(null);
    if (selectionRef.current !== taskRequest.projectId) selectProject(taskRequest.projectId);
  }, [taskRequest, taskNavigationBlocked, projectsLoading, projects, selectProject]);

  const refreshSnapshot = useCallback(() => {
    if (!selectedProjectId) {
      setSnapshot(null);
      return Promise.resolve();
    }
    if (UI_PREVIEW) return Promise.resolve();
    const request = ++snapshotRequest.current;
    return invoke<MultiAgentSnapshot>("ma_snapshot", { projectId: selectedProjectId })
      .then((nextSnapshot) => {
        if (selectionRef.current !== selectedProjectId || request !== snapshotRequest.current) return;
        setSnapshot(nextSnapshot);
      })
      .catch((reason) => {
        if (selectionRef.current === selectedProjectId && request === snapshotRequest.current) setError(String(reason));
      });
  }, [selectedProjectId]);

  useEffect(() => {
    if (UI_PREVIEW || !selectedProjectId) {
      if (!selectedProjectId) setSnapshot(null);
      return;
    }
    void refreshSnapshot();
    const timer = window.setInterval(() => void refreshSnapshot(), 3000);
    return () => {
      window.clearInterval(timer);
    };
  }, [selectedProjectId, refreshSnapshot]);

  const mutateTask = async (taskId: string, command: "ma_approve_task" | "ma_retry_task") => {
    if (taskInFlight.current) return;
    taskInFlight.current = true;
    const projectId = selectedProjectId;
    setApprovingTaskId(taskId);
    setError(null);
    try {
      if (UI_PREVIEW) { setSnapshot((current) => current ? { ...current, tasks: current.tasks.map((task) => task.id === taskId ? { ...task, status: "done" } : task) } : current); return; }
      await invoke(command, { taskId });
      await refreshSnapshot();
    } catch (reason) {
      if (selectionRef.current === projectId) setError(String(reason));
    } finally {
      taskInFlight.current = false;
      setApprovingTaskId(null);
    }
  };

  const runtimeMap = useMemo(
    () => new Map(runtimes.map((runtime) => [runtime.kind, runtime])),
    [runtimes],
  );
  const runtimePreferences = useRuntimeOrder(RUNTIME_ORDER);
  // chips 只展示协作可调度的 Runtime；五个纯原生聊天（claude/codex/kimi/opencode/pi chat）
  // 不能作为协作 Agent 执行，不出现在协作中心（开关在“管理 Runtime”面板）。
  const visibleRuntimeOrder = useMemo(() => runtimePreferences.order
    .filter((kind) => COLLABORATION_RUNTIMES.includes(kind))
    .filter(availability.isRuntimeEnabled), [runtimePreferences.order, availability.ready, availability.prefs]);
  const moveVisibleRuntime = (kind: AgentRuntimeKind, direction: -1 | 1) => {
    const target = visibleRuntimeOrder[visibleRuntimeOrder.indexOf(kind) + direction];
    if (target) runtimePreferences.reorder(kind, target, direction === -1 ? "before" : "after");
  };
  const alignRuntimePopover = (details: HTMLDetailsElement) => {
    const popover = details.querySelector<HTMLElement>(".collab-runtime-popover");
    const toolbar = details.closest<HTMLElement>(".collab-runtime-toolbar");
    if (!popover || !toolbar) return;
    if (!details.open) {
      popover.style.removeProperty("left");
      popover.style.removeProperty("right");
      return;
    }
    popover.style.left = "0";
    popover.style.right = "auto";
    window.requestAnimationFrame(() => {
      if (!details.open) return;
      const popoverBounds = popover.getBoundingClientRect();
      const toolbarBounds = toolbar.getBoundingClientRect();
      if (popoverBounds.right > toolbarBounds.right - 12) {
        popover.style.left = "auto";
        popover.style.right = "0";
      }
    });
  };
  const [draggingRuntime, setDraggingRuntime] = useState<AgentRuntimeKind | null>(null);
  const [dropTarget, setDropTarget] = useState<{ kind: AgentRuntimeKind; position: "before" | "after" } | null>(null);

  const busyRuntimeKinds = useMemo(() => {
    if (!currentSnapshot) return new Set<AgentRuntimeKind>();
    const agents = new Map(currentSnapshot.agents.map((agent) => [agent.id, agent.runtimeKind]));
    return new Set(currentSnapshot.tasks
      .filter((task) => ["claimed", "running", "waiting_approval", "in_review"].includes(task.status))
      .map((task) => task.assigneeAgentId && agents.get(task.assigneeAgentId))
      .filter((kind): kind is AgentRuntimeKind => Boolean(kind)));
  }, [currentSnapshot]);

  const checkRuntimeUpdates = async () => {
    if (UI_PREVIEW) return;
    if (runtimeCheckInFlight.current || pendingUpgrades.current.size) return;
    runtimeCheckInFlight.current = true;
    const sequence = ++runtimeCheckSequence.current;
    setCheckingRuntimes(true);
    try {
      const checked = await invoke<RuntimeProbe[]>("ma_check_runtime_updates");
      if (sequence === runtimeCheckSequence.current) {
        setRuntimes(checked); setError(null);
        setLoadingErrors(current => { const next = { ...current }; delete next["运行环境检测"]; delete next["更新检查"]; return next; });
      }
    } catch (reason) {
      if (sequence === runtimeCheckSequence.current) setError(`检查 Runtime 更新失败：${String(reason)}`);
    } finally {
      if (sequence === runtimeCheckSequence.current) { runtimeCheckInFlight.current = false; setCheckingRuntimes(false); }
    }
  };

  const upgradeOneRuntime = async (kind: AgentRuntimeKind, skipConfirmation = false) => {
    if (pendingUpgrades.current.has(kind) || runtimeCheckInFlight.current) return false;
    if (!skipConfirmation && busyRuntimeKinds.has(kind) && !window.confirm(`${RUNTIME_LABELS[kind]} 正在执行任务，升级可能中断当前任务。仍要继续吗？`)) return false;
    pendingUpgrades.current.add(kind);
    setUpgradingRuntimes(new Set(pendingUpgrades.current));
    setUpgradeResults((current) => ({ ...current, [kind]: undefined }));
    try {
      const receipt = await invoke<RuntimeUpgradeReceipt>("ma_upgrade_runtime", { kind });
      setRuntimes((current) => current.map((runtime) => {
        if (runtime.kind !== kind) return runtime;
        const updateAvailable = Boolean(runtime.latestVersion && runtimeVersion(receipt.runtime.version) !== runtimeVersion(runtime.latestVersion));
        return { ...receipt.runtime, latestVersion: runtime.latestVersion, updateAvailable, updateStatus: updateAvailable ? "available" : "unchecked" };
      }));
      setUpgradeResults((current) => ({ ...current, [kind]: { error: Boolean(receipt.runtime.error), message: receipt.runtime.error ?? "升级命令完成，已刷新本机版本" } }));
      return true;
    } catch (reason) {
      setUpgradeResults((current) => ({ ...current, [kind]: { error: true, message: `升级失败：${String(reason)}` } }));
      return false;
    } finally {
      pendingUpgrades.current.delete(kind);
      setUpgradingRuntimes(new Set(pendingUpgrades.current));
    }
  };

  const upgradeRuntime = async (kind: AgentRuntimeKind) => {
    await upgradeOneRuntime(kind);
  };

  const upgradeAllRuntimes = async () => {
    const candidates = runtimes.filter((runtime) => availability.isRuntimeEnabled(runtime.kind) && runtime.updateAvailable && runtime.canAutoUpdate && !pendingUpgrades.current.has(runtime.kind));
    if (!candidates.length) return;
    const busy = candidates.filter((runtime) => busyRuntimeKinds.has(runtime.kind));
    if (busy.length && !window.confirm(`${busy.map((runtime) => RUNTIME_LABELS[runtime.kind]).join("、")} 正在执行任务，批量升级可能中断当前任务。仍要继续吗？`)) return;
    await Promise.allSettled(candidates.map((runtime) => upgradeOneRuntime(runtime.kind, true)));
  };

  const suggestedSkillSets = useMemo(() => {
    if (!snapshot) return [];
    const availableAgents = snapshot.agents.filter((agent) => !agent.removedAt && availability.isRuntimeEnabled(agent.runtimeKind));
    const roles = new Set(availableAgents.map((agent) => agent.role));
    const runtimes = new Set(availableAgents.map((agent) => agent.runtimeKind));
    return skillSets.filter((set) =>
      (!set.projectPath || set.projectPath === snapshot.project.rootPath)
      && (!set.agentRole || roles.has(set.agentRole))
      && set.items.some((item) => runtimes.has(item.runtime)),
    );
  }, [skillSets, snapshot, availability.prefs, availability.ready]);

  const applySkillSet = async (set: SuggestedSkillSet) => {
    try {
      await invoke("skill_center_apply_set", { id: set.id, trustConfirmed: false });
      setError(null);
    } catch (reason) {
      setError(`${String(reason)}。请先在 Skills 中审阅并信任相关 Skill。`);
    }
  };

  const openWorkspace = async () => {
    if (!selectedProjectId) return;
    selectProject(selectedProjectId);
    try {
      if (!UI_PREVIEW) await emit("multi-agent-workspace-open", { projectId: selectedProjectId });
      onClose();
    } catch (reason) { setError(`打开协作详情失败：${String(reason)}`); }
  };

  const showTask = (task: NonNullable<typeof currentSnapshot>["tasks"][number]) => {
    if (currentSnapshot) setTaskDetail({ projectId: currentSnapshot.project.id, taskId: task.id });
  };
  const eventTask = (event: NonNullable<typeof currentSnapshot>["events"][number]) => {
    const run = currentSnapshot?.runs.find((run) => run.id === event.entityId);
    const id = event.entityType === "task" ? event.entityId : typeof event.payload.taskId === "string" ? event.payload.taskId : run?.taskId;
    return currentSnapshot?.tasks.find((task) => task.id === id);
  };
  const openArchiveRecord = (record: KnowledgeSearchResult) => {
    const task = currentSnapshot?.tasks.find((task) => task.id === (record.taskId ?? (record.sourceType === "task" ? record.sourceId : null)));
    if (task) { setDetail({ title: record.title, content: record.content + "\n\n关联任务：" + task.title + "\n" + task.description + "\n验收标准：" + task.acceptanceCriteria + "\n验证命令：" + (task.verificationCommand ?? "未设置") + "\n\n引用：" + record.sourceUri }); return; }
    const collection = record.sourceType === "conversation" ? currentSnapshot?.messages : record.sourceType === "artifact" ? currentSnapshot?.artifacts : record.sourceType === "run" ? currentSnapshot?.runs : record.sourceType === "review" ? currentSnapshot?.reviewDecisions : record.sourceType === "memory" ? currentSnapshot?.memories : undefined;
    const original = collection?.find((item) => item.id === record.sourceId);
    setDetail({ title: record.title, content: original ? JSON.stringify(original, null, 2) : record.content + "\n\n引用：" + record.sourceUri });
  };
  const agents = currentSnapshot?.agents ?? [];
  const teamAgents = agents.filter(agent => !agent.removedAt);
  const previewRemovalBlockers = useMemo(() => {
    if (!UI_PREVIEW || !removingAgent) return undefined;
    return (currentSnapshot?.tasks ?? []).filter(task =>
      [task.assigneeAgentId, task.reviewerAgentId].includes(removingAgent.id) && !["done", "cancelled"].includes(task.status))
      .map(task => task.title);
  }, [currentSnapshot, removingAgent]);
  const memberUnavailableReason = currentSnapshot ? null : projectsLoading
    ? "正在加载协作项目，请稍候再新增成员。"
    : error ? "协作项目未能加载，请先处理上方错误，再新增成员。"
    : !projects.length ? "成员必须归属协作项目。点击「创建项目」，完成后即可在这里新增成员。"
    : selectedProjectId ? "正在加载当前项目，请稍候再新增成员。" : "请先在顶部选择一个协作项目，再新增成员。";
  const enabledAgents = teamAgents.filter((agent) => availability.isRuntimeEnabled(agent.runtimeKind));
  const memberCount = currentSnapshot && availability.ready ? enabledAgents.length : "—";
  const hiddenMemberCount = availability.ready ? teamAgents.length - enabledAgents.length : 0;
  const activity = useMemo(() => [...(currentSnapshot?.events ?? [])].sort((a, b) => b.createdAt - a.createdAt), [currentSnapshot?.events]);
  const sessionName = (id: string) => health?.sessions.find((item) => item.id === id)?.title ?? agents.find((item) => item.id === id)?.name ?? currentSnapshot?.tasks.find((item) => item.id === id)?.title ?? id;
  return <div className="collaboration-settings-panel collab-overview-page collab-task-first">
    <header className="collab-page-header"><div><h2>协作中心</h2><p>管理团队、任务与协作通信</p></div><div className="collab-guide-actions"><CommunicationGuideButton scope="project" projectId={selectedProjectId} className="btn secondary communication-workspace-entry" /><button className="btn secondary" disabled={!currentSnapshot} onClick={() => void openWorkspace()}><ArrowSquareOutIcon size={16} />打开工作台</button></div></header>
    {Object.entries(loadingErrors).map(([source, message]) => <p role="alert" key={source}>{source}失败：{message}</p>)}
    {error && <p role="alert" className="collaboration-settings-error">{error}</p>}
    <section className="collab-project-toolbar">
      <div className="collab-project-picker"><select aria-label="当前项目" value={selectedProjectId ?? ""} disabled={!projects.length} onChange={(event) => selectProject(event.target.value || null)}>{!selectedProjectId && <option value="" disabled={projects.length > 0}>{projects.length ? "请选择协作项目" : "暂无协作项目"}</option>}{projects.map((project) => <option key={project.id} value={project.id}>{project.name}</option>)}</select>
      <span className="collab-project-counts"><b>{taskSnapshot?.tasks.length ?? "—"}</b> 个任务<span>· <b>{memberCount}</b> 位成员</span></span></div>
      <span className={"collab-project-state " + (currentSnapshot?.project.status ?? "")}><i />{currentSnapshot ? projectStatusLabel(currentSnapshot.project) : selectedProjectId ? "加载中…" : "未选择项目"}</span>
      <div className="collab-project-actions"><button className="btn secondary" disabled={!currentSnapshot || projectBusy} onClick={() => void pauseProject()}>{projectBusy ? "处理中…" : currentSnapshot?.project.status === "active" ? "暂停项目" : "继续项目"}</button><button type="button" className="btn secondary" disabled={projectsLoading} onClick={() => setCreatingProject(true)}>创建项目</button><button className="btn collab-primary" disabled={Boolean(selectedProjectId && !currentSnapshot)} onClick={() => requestHandoff()}>交接工作</button></div>
    </section>
    <section className="collab-runtime-toolbar" aria-label="协作 Runtime">
      <div className="collab-runtime-heading">
        <button type="button" className="collab-runtime-toggle" aria-expanded={runtimeExpanded} aria-controls={runtimeContentId} onClick={() => setRuntimeExpanded(current => !current)}><CaretRightIcon size={16} aria-hidden="true" /><strong>协作 Runtime · {availability.ready ? `${visibleRuntimeOrder.length} 个已启用` : "配置加载中…"}</strong><span>{upgradingRuntimes.size ? `${upgradingRuntimes.size} 个升级中…` : checkingRuntimes ? "正在检查更新…" : runtimeExpanded ? "收起运行环境" : "需要时展开查看"}</span></button>
        <div className="collab-runtime-actions">
          <button type="button" className="btn secondary" onClick={() => setManagingRuntimes(true)}>管理 Runtime</button>
        </div>
      </div>
      <div className="collab-runtime-content" id={runtimeContentId} hidden={!runtimeExpanded}>
        <div className="collab-runtime-actions collab-runtime-expanded-actions">
          <button className="btn secondary collab-check-updates" disabled={checkingRuntimes || upgradingRuntimes.size > 0} onClick={() => void checkRuntimeUpdates()}><ArrowClockwiseIcon size={16} />{checkingRuntimes ? "检查中…" : "检查更新"}</button>
          <details className="collab-upgrade-menu"><summary aria-label="运行时升级选项">⋯</summary><button className="btn secondary" disabled={checkingRuntimes || !runtimes.some((runtime) => availability.isRuntimeEnabled(runtime.kind) && runtime.updateAvailable && runtime.canAutoUpdate)} onClick={() => { if (!UI_PREVIEW) void upgradeAllRuntimes(); }}>升级全部可更新运行时</button></details>
        </div>
      <div className="collab-runtime-chips">{visibleRuntimeOrder.map((kind, index) => {
      const runtime = runtimeMap.get(kind);
      const dropPosition = dropTarget?.kind === kind ? dropTarget.position : undefined;
      return <div className={`collab-runtime-order-item${draggingRuntime === kind ? " dragging" : ""}`} key={kind} data-drop-position={dropPosition}
        onDragOver={(event) => {
          if (!draggingRuntime || draggingRuntime === kind) return;
          event.preventDefault();
          event.dataTransfer.dropEffect = "move";
          const position = event.clientX < event.currentTarget.getBoundingClientRect().left + event.currentTarget.getBoundingClientRect().width / 2 ? "before" : "after";
          if (dropTarget?.kind !== kind || dropTarget.position !== position) setDropTarget({ kind, position });
        }}
        onDrop={(event) => {
          event.preventDefault();
          const source = event.dataTransfer.getData("text/plain") as AgentRuntimeKind;
          if (source && source !== kind) {
            const rect = event.currentTarget.getBoundingClientRect();
            runtimePreferences.reorder(source, kind, event.clientX < rect.left + rect.width / 2 ? "before" : "after");
          }
          setDraggingRuntime(null);
          setDropTarget(null);
        }}>
        <span className="collab-runtime-order-grip" draggable title={`拖动以调整 ${RUNTIME_LABELS[kind]} 的位置`} aria-hidden="true"
          onDragStart={(event) => { event.dataTransfer.effectAllowed = "move"; event.dataTransfer.setData("text/plain", kind); setDraggingRuntime(kind); }}
          onDragEnd={() => { setDraggingRuntime(null); setDropTarget(null); }}>⠿</span>
        <details className="collab-runtime-chip" onToggle={(event) => alignRuntimePopover(event.currentTarget)}><summary><img src={RUNTIME_LOGOS[kind]} data-runtime={kind} alt="" /><span>{RUNTIME_LABELS[kind]}</span><i className={runtime?.installed ? runtime.error ? "warning" : "installed" : "missing"} /></summary>
        <div className="collab-runtime-popover"><strong>{RUNTIME_LABELS[kind]}</strong><p>{!runtime ? "检测结果尚未返回" : runtime.installed ? "命令可发现" : "未发现命令"} · {runtimeVersion(runtime?.version)}</p><p>{runtime?.error ?? runtime?.updateError ?? (runtime?.latestVersion ? "最新版本 " + runtimeVersion(runtime.latestVersion) : "更新状态尚未确认")}</p>
        <div className="collab-runtime-reorder-controls" aria-label={`${RUNTIME_LABELS[kind]} 位置调整`}><button type="button" aria-label={`将 ${RUNTIME_LABELS[kind]} 左移`} title="左移" disabled={index === 0} onClick={() => moveVisibleRuntime(kind, -1)}>←</button><button type="button" aria-label={`将 ${RUNTIME_LABELS[kind]} 右移`} title="右移" disabled={index === visibleRuntimeOrder.length - 1} onClick={() => moveVisibleRuntime(kind, 1)}>→</button></div>
        {upgradeResults[kind] && <p role="status">{upgradeResults[kind]?.message}</p>}
        {runtime?.updateAvailable && <button className="btn secondary" disabled={checkingRuntimes || upgradingRuntimes.has(kind)} onClick={() => { if (UI_PREVIEW) return; if (runtime.canAutoUpdate) void upgradeRuntime(kind); else void navigator.clipboard.writeText(runtime.updateCommand ?? "").catch((reason) => setError(`复制升级命令失败：${String(reason)}`)); }}>{upgradingRuntimes.has(kind) ? "升级中…" : runtime.canAutoUpdate ? "升级运行时" : "复制命令"}</button>}</div></details></div>;
    })}{!visibleRuntimeOrder.length && <span className="collab-runtime-management-empty">{availability.ready ? "暂无启用的 Runtime" : "Runtime 配置未就绪"}</span>}</div>
      </div>
      {Object.entries(upgradeResults).filter(([, result]) => result?.error).map(([kind, result]) => <p key={kind} role="alert" className="collab-runtime-preference-error">{RUNTIME_LABELS[kind as AgentRuntimeKind]}：{result!.message}</p>)}
      {(availability.error || runtimePreferences.error) && <div className="collab-runtime-status">{availability.error && <span role="alert" className="collab-runtime-preference-error">{availability.error}</span>}{runtimePreferences.error && <span className="collab-runtime-preference-error" role="alert">{runtimePreferences.error}</span>}</div>}
    </section>
    {managingRuntimes && <RuntimeManagementDialog order={runtimePreferences.order} onReorder={runtimePreferences.reorder} orderError={runtimePreferences.error} onClose={() => setManagingRuntimes(false)} />}
    <nav className="collab-tabs" role="tablist" aria-label="协作视图">{[["overview","概览"],["team","团队"],["archive","项目档案"],["records","协作记录"],["diagnostics","协作诊断"]].map(([id,title]) => <button key={id} role="tab" aria-selected={tab === id} className={tab === id ? "active" : ""} onClick={() => setTab(id)}>{title}{id === "team" && <b>{memberCount}</b>}{id === "diagnostics" && !!diagnosticIssues.length && <b className="warning">{diagnosticIssues.length}</b>}</button>)}</nav>
    {tab === "records" && (currentSnapshot
      ? <CollaborationRecordsPanel key={currentSnapshot.project.id} snapshot={currentSnapshot} preview={UI_PREVIEW} />
      : <p className="collab-empty">{selectedProjectId ? "正在加载协作记录…" : "请选择项目后查看协作记录"}</p>)}
    {tab === "archive" && <KnowledgeBaseDialog embedded projectId={selectedProjectId} snapshot={UI_PREVIEW ? currentSnapshot : undefined} onOpenRecord={openArchiveRecord} />}
    {tab === "overview" && <>
      {currentSnapshot && <details className="collab-project-info" key={currentSnapshot.project.id}><summary><CaretRightIcon size={14}/><span>项目信息</span></summary><dl><dt>项目目录</dt><dd><code>{currentSnapshot.project.rootPath || "尚未提供项目目录"}</code></dd></dl></details>}
      <section className="collab-diagnostic-summary" aria-label="协作诊断摘要"><i className={diagnosticIssues.length ? "warning" : "unknown"} /><strong>{health ? checkSummary : "尚未取得检查结果"}</strong><button onClick={() => setTab("diagnostics")} title={health ? `${health.scopeLabel} · ${checks.length} 项检查${health.checkedAt ? " · " + new Date(health.checkedAt).toLocaleTimeString("zh-CN") : ""}` : undefined}>查看诊断 →</button></section>
      <div className="collab-main-grid"><section ref={taskPanel} tabIndex={-1} className="collab-task-panel" aria-label="项目任务"><header><h3>项目任务</h3><div className="collab-task-filters" role="group" aria-label="任务筛选">{[["attention","待处理"],["all","全部"],["running","执行中"],["in_review","待审核"]].map(([id,title]) => <button key={id} aria-pressed={filter === id} className={filter === id ? "active" : ""} onClick={() => setFilter(id)}>{title}</button>)}</div></header>
      {currentSnapshot?.runs.filter(run => run.sessionLocator?.stopUnverified === true).map(run => <UnverifiedRunNotice key={run.id} run={run} taskTitle={currentSnapshot.tasks.find(task => task.id === run.taskId)?.title} onConfirmed={refreshSnapshot}/>)}
      <div className="collab-task-columns"><span>任务</span><span>负责人</span><span>状态</span><span>更新</span><span /></div>
      {groupTasksByStage(taskSnapshot).filter((group) => group.tasks.some((task) => taskMatchesFilter(task.status, filter))).map((group) => <div key={group.stage} className="collab-stage"><button className="collab-stage-title" aria-expanded={!collapsed.has(group.stage)} onClick={() => setCollapsed((current) => { const next = new Set(current); if (next.has(group.stage)) next.delete(group.stage); else next.add(group.stage); return next; })}><span>{collapsed.has(group.stage) ? "›" : "⌄"} 阶段 {group.stage}{UI_PREVIEW ? " · " + (["调研与方案","实现","验证与交付"][group.stage] ?? "") : ""}</span><span>{group.done}/{group.total} 已完成 <i className={group.allDone ? "done" : ""}>{group.allDone ? "✓" : ""}</i></span></button>
      {!collapsed.has(group.stage) && [...group.tasks].sort((a,b) => taskPriority(a.status) - taskPriority(b.status) || b.updatedAt - a.updatedAt).filter((task) => taskMatchesFilter(task.status, filter)).map((task) => {
        const owner = agents.find((agent) => agent.id === task.assigneeAgentId);
        return <div className={"collab-task-row" + (task.status === "running" ? " selected" : "")} key={task.id}><strong><i className={"task-mark " + task.status}>{task.status === "done" ? "✓" : ""}</i>{task.title}</strong><span className="collab-task-owner">{owner && <AgentAvatar avatarId={owner.avatarId} runtimeKind={owner.runtimeKind} decorative />}{owner?.name ?? "未指派"}</span><span className={"collab-status " + task.status}><i />{TASK_STATUS_LABELS[task.status]}</span><time>{formatDateTime(task.updatedAt)}</time><details className="collab-task-menu"><summary aria-label={task.title + "操作"}>⋯</summary><div><button onClick={() => showTask(task)}>查看任务详情</button>{!["done","waiting_approval","in_review"].includes(task.status) && <button onClick={() => requestHandoff(task.id)}>交接此任务</button>}{task.status === "in_review" && <button disabled={approvingTaskId !== null} onClick={() => void mutateTask(task.id, "ma_approve_task")}>批准交付</button>}{["failed","cancelled","changes_requested"].includes(task.status) && <button disabled={approvingTaskId !== null} onClick={() => void mutateTask(task.id, "ma_retry_task")}>重试任务</button>}</div></details></div>;
      })}</div>)}
      {!taskSnapshot?.tasks.some((task) => taskMatchesFilter(task.status, filter)) && <p className="collab-empty">{projectsLoading || (selectedProjectId && !currentSnapshot) ? "正在加载项目任务…" : taskSnapshot?.tasks.length ? "当前筛选暂无任务" : "暂无协作任务"}</p>}</section>
      <details className="collab-activity-disclosure"><summary><CaretRightIcon size={16} aria-hidden="true" /><strong>最近活动</strong><span>{activity.length} 条活动 · {health?.handoffs.length ?? 0} 条交接</span></summary><aside className="collab-side-panels"><section className="collab-activity-panel"><header><h3>最近活动</h3><button onClick={() => setTab("records")}>查看全部活动</button></header><div>{activity.slice(0,4).map((event) => <article key={event.seq}><i className={String(event.payload.tone ?? "")} /><time>{formatDateTime(event.createdAt)}</time><p>{activityLabel(event.eventType)}{eventTask(event) && <button onClick={() => showTask(eventTask(event)!)}> · {eventTask(event)!.title}</button>}</p></article>)}{!activity.length && <p className="collab-empty">暂无活动记录</p>}</div></section>
      <section className="collab-handoff-panel"><header><h3>交接记录</h3></header>{health?.handoffs.slice(0,3).map((record) => <article key={record.id}><div><p>{sessionName(record.sourceId)} → {sessionName(record.targetId)}</p><small>{HANDOFF_LABELS[record.state] ?? record.state} · {record.consequence}</small></div><button onClick={() => requestHandoff(undefined,record.id)}>查看</button></article>)}{!health?.handoffs.length && <p className="collab-empty">暂无交接记录</p>}</section></aside></details></div>
      <p className="collab-sync-note"><ArrowClockwiseIcon size={16} />任务依赖与执行状态自动同步</p>
    </>}
    {tab === "team" && (() => {
      const query = teamSearch.trim().toLowerCase();
      const filteredAgents = enabledAgents.filter((agent) =>
        (teamRoleFilter === "all" || agent.role === teamRoleFilter)
        && (query === "" || `${agent.name} ${agent.description} ${RUNTIME_LABELS[agent.runtimeKind]}`.toLowerCase().includes(query)));
      const roleCount = (role: AgentRole) => enabledAgents.filter((agent) => agent.role === role).length;
      return <><div className={`collab-team-toolbar${memberUnavailableReason ? " collab-team-unavailable" : ""}`}><div className="collab-team-filter-row">
        {([["all", "全部"], ["orchestrator", ROLE_LABELS.orchestrator], ["worker", ROLE_LABELS.worker], ["reviewer", ROLE_LABELS.reviewer], ["integrator", ROLE_LABELS.integrator]] as const).map(([value, label]) => (
          <button key={value} type="button" className={`collab-team-filter-pill${teamRoleFilter === value ? " active" : ""}`} aria-pressed={teamRoleFilter === value} onClick={() => setTeamRoleFilter(value)}>{label}<b>{value === "all" ? enabledAgents.length : roleCount(value)}</b></button>
        ))}
        <div className="collab-team-search"><input aria-label="搜索成员" placeholder="搜索成员 / 职责…" value={teamSearch} onChange={(event) => setTeamSearch(event.target.value)} /></div>
        <button className="btn secondary" disabled={!currentSnapshot} title={memberUnavailableReason ?? undefined} aria-describedby={memberUnavailableReason ? "collab-member-unavailable" : undefined} onClick={() => { setEditorProjectId(currentSnapshot!.project.id); setEditingAgent(null); }}>新增成员</button>
      </div>{memberUnavailableReason ? <p id="collab-member-unavailable" role="status">{memberUnavailableReason}</p> : <p>默认团队可自由编辑，点击名片调整人格、模型与职责。{hiddenMemberCount > 0 && ` 共 ${teamAgents.length} 位，隐藏 ${hiddenMemberCount} 位（对应 Runtime 未启用）。`}</p>}
      </div><SettingsSection title="团队"><div className="collab-agent-card-grid">
        {filteredAgents.map((agent) => {
          const active = currentSnapshot?.runs.some((run) => run.agentId === agent.id && ["queued", "starting", "running", "waiting_input", "waiting_approval"].includes(run.status));
          const busyHint = "该成员有执行中的任务，请完成或停止任务后编辑";
          const editAgent = () => { setEditorProjectId(agent.projectId); setEditingAgent(agent); };
          const closeMenu = (button: HTMLButtonElement) => {
            const menu = button.closest("details");
            if (menu) { menu.open = false; menu.querySelector("summary")?.focus(); }
          };
          return <div className="collab-agent-card-cell" key={agent.id}>
            <AgentCard agent={agent} dimmed={!agent.enabled} onClick={active ? undefined : editAgent} />
            <details className="collab-member-actions" name={memberMenuGroup} onBlur={event => {
              if (!event.currentTarget.contains(event.relatedTarget)) event.currentTarget.open = false;
            }}>
              <summary aria-label={`${agent.name}的更多操作`} title="更多操作"><DotsThreeIcon size={19} /></summary>
              <div className="collab-member-action-list">
                <button type="button" className="btn secondary" disabled={active} title={active ? busyHint : undefined} onClick={event => {
                  closeMenu(event.currentTarget);
                  editAgent();
                }}>编辑</button>
                <button type="button" className="btn secondary collab-member-remove-action" onClick={event => {
                  closeMenu(event.currentTarget);
                  setRemovingAgent(agent);
                }}>从团队移除</button>
              </div>
            </details>
            {active && <span className="collab-agent-card-busy" title={busyHint}>执行中 · 暂不可编辑</span>}
          </div>;
        })}
        {currentSnapshot && !filteredAgents.length && <p className="collab-empty">{enabledAgents.length === 0 ? "暂无可用成员，点击右上角「新增成员」创建。" : "没有匹配的成员。"}</p>}
      </div></SettingsSection>{suggestedSkillSets.length > 0 && <SettingsSection title="Skill 配置建议">{suggestedSkillSets.map((set) => <article key={set.id}><strong>{set.name}</strong><p>{set.description}</p><button className="btn secondary" onClick={() => { if (!UI_PREVIEW) void applySkillSet(set); }}>应用配置集</button></article>)}</SettingsSection>}</>;
    })()}
    {creatingProject && <CollaborationProjectDialog preview={UI_PREVIEW} onClose={() => setCreatingProject(false)} onCreate={async (draft, gitSetup) => {
      const created = UI_PREVIEW ? { ...PREVIEW_PROJECT, ...draft, id: crypto.randomUUID(), status: "active" as const, createdAt: Date.now(), updatedAt: Date.now() } : await invoke<MultiAgentProject>("ma_create_project", { input: { ...draft, defaultBranch: "main" }, gitSetup });
      setProjects(current => [created, ...current.filter(project => project.id !== created.id)]);
      if (UI_PREVIEW) setSnapshot({ ...COLLABORATION_UI_PREVIEW_SNAPSHOT, project: created, agents: [], tasks: [], events: [], runs: [], agentSessions: [], messages: [], artifacts: [], approvals: [], dependencies: [], wakeups: [], runLogs: [], instructions: [], runtimeEvents: [], memories: [], reviewDecisions: [] });
      setError(null);
      selectProject(created.id);
      setTab("team");
    }} />}
    {editingAgent !== undefined && editorProjectId && <CollaborationAgentEditor agent={editingAgent} runtimeLocked={false} onClose={() => setEditingAgent(undefined)} onSave={async (draft: AgentDraft) => {
      if (!currentSnapshot || currentSnapshot.project.id !== editorProjectId) throw new Error("项目已切换，请关闭后重新编辑");
      if (currentSnapshot.agents.some((item) => item.id !== editingAgent?.id && item.name.toLocaleLowerCase() === draft.name.toLocaleLowerCase())) throw new Error("本项目已存在同名成员，请使用不同名称");
      if (UI_PREVIEW) {
        const updated: AgentProfile = editingAgent ? { ...editingAgent, ...draft, updatedAt: Date.now() } : { ...draft, id: crypto.randomUUID(), projectId: editorProjectId, permissionProfile: "default", maxConcurrency: 1, capabilities: [], createdAt: Date.now(), updatedAt: Date.now() };
        setSnapshot({ ...currentSnapshot, agents: editingAgent ? currentSnapshot.agents.map((item) => item.id === updated.id ? updated : item) : [...currentSnapshot.agents, updated] });
        return;
      }
      if (editingAgent) await invoke("ma_update_agent", { input: { ...draft, projectId: editorProjectId, agentId: editingAgent.id } });
      else await invoke("ma_create_agent", { input: { ...draft, projectId: editorProjectId, permissionProfile: "default", maxConcurrency: 1 } });
      if (selectionRef.current === editorProjectId) await refreshSnapshot();
    }} />}

    {removingAgent && currentSnapshot?.project.id === removingAgent.projectId && <CollaborationAgentRemovalDialog
      agent={removingAgent} previewBlockers={previewRemovalBlockers} onClose={() => setRemovingAgent(null)}
      onRemoved={() => {
        const { id, projectId } = removingAgent;
        setRemovingAgent(null);
        if (selectionRef.current !== projectId) return;
        setSnapshot(previous => previous?.project.id === projectId ? {
          ...previous, agents: previous.agents.map(agent => agent.id === id ? { ...agent, enabled: false, removedAt: Date.now() } : agent),
        } : previous);
        void refreshSnapshot();
      }} />}
    <CollaborationSupportPanel initialGlobal={initialDiagnosticGlobal} snapshot={currentSnapshot} enabledRuntimes={visibleRuntimeOrder} onChanged={() => { if (!UI_PREVIEW) void refreshSnapshot(); }} view={tab} request={handoffRequest} onHealthChange={setHealth} onHandoffClose={() => setHandoffRequest(null)} />
    {taskDetail && currentSnapshot?.project.id === taskDetail.projectId && createPortal(<Modal className="collaboration-handoff-dialog collab-task-detail-dialog" onClose={() => setTaskDetail(null)} ariaLabel="协作任务详情"><button className="collab-modal-close" aria-label="关闭任务详情" onClick={() => setTaskDetail(null)}>×</button><h3>{currentSnapshot.tasks.find(task => task.id === taskDetail.taskId)?.title ?? "任务已不可用"}</h3><CollaborationTaskDetails snapshot={currentSnapshot} taskId={taskDetail.taskId} onChanged={refreshSnapshot}/></Modal>, document.body)}
    {detail && <Modal className="collaboration-handoff-dialog" onClose={() => setDetail(null)} ariaLabel={detail.title}><button className="collab-modal-close" aria-label="关闭详情" onClick={() => setDetail(null)}>×</button><h3>{detail.title}</h3><pre>{detail.content}</pre></Modal>}
  </div>;
}
