import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { ArrowClockwise, ArrowElbowDownLeft, ArrowSquareOut, Broom, CaretDown, CaretRight, Check, CheckCircle, ChatCircleText, ClockCounterClockwise, DotsThree, FileArrowDown, GearSix, Lightning, ListChecks, Paperclip, Pause, PencilSimple, Plus, Robot, SidebarSimple, SlidersHorizontal, Stop, Trash, User, WarningCircle, X } from "@phosphor-icons/react";
import { AgentAvatar } from "./AgentAvatar";
import { ConversationMentionHighlight } from "./ConversationMentionHighlight";
import { ResizableConversationComposer } from "./ResizableConversationComposer";
import { ConversationMarkdown } from "./ConversationMarkdown";
import { ConversationMessageCopy } from "./ConversationMessageCopy";
import { ConversationImageAttachment } from "./ConversationImageAttachment";
import { ConversationVideoAttachment } from "./ConversationVideoAttachment";
import { ConversationApprovalRequests } from "./ConversationApprovalRequests";
import { ConversationMemberActivity } from "./ConversationMemberActivity";
import { ConversationMemberModels } from "./ConversationMemberModels";
import { conversationMemberActivity } from "../conversationActivity";
import { ConversationPlanCard } from "./ConversationPlanCard";
import { ConversationHistoryDialog, conversationTitle } from "./ConversationHistoryDialog";
import { ConversationSwitcher } from "./ConversationSwitcher";
import { ConversationDelegationDetails } from "./ConversationDelegationDetails";
import { ConversationTaskView } from "./ConversationTaskView";
import { AgentMentionMenu, type AgentMentionOption } from "./AgentMentionMenu";
import { CollaborationProjectDialog } from "./CollaborationProjectDialog";
import { CollaborationProjectNavigation } from "./CollaborationProjectNavigation";
import { GrokExecutionDrawer } from "./GrokExecutionDrawer";
import { UnverifiedRunNotice } from "./UnverifiedRunNotice";
import { Modal } from "./ui/Modal";
import { conversationChatDispatch, conversationIsActive, conversationMentionToken, conversationRecipients, conversationRequestIntent, conversationRequestMode, CONVERSATION_STATUS, hasConversationPlanGoal, latestConversationRound, orderedConversationMessages } from "../collaborationConversation";
import { RUNTIME_LABELS } from "../runtimeLogos";
import { visibleProjectTasks } from "../collaborationTasks";
import { hasMentionToken, mentionTextSegments, selectMentionTokens } from "../multiAgentMention";
import { useRuntimeAvailability } from "../runtimeAvailability";
import type { AgentProfile, AgentRun, ConversationAttachment, ConversationDesignDelegation, ConversationExecutionPlan, ConversationMemberModelSummary, ConversationMessage, ConversationOriginalSession, ConversationRound, ConversationSendInput, ConversationSnapshot, MultiAgentProject, MultiAgentSnapshot, MultiAgentTask, SessionRecord } from "../types";
import "../styles/collaboration-conversation.css";
import "../styles/team-work-views.css";
import "../styles/conversation-member-activity.css";
import "../styles/conversation-policy.css";
import "../styles/multi-agent-grok-noir.css";

interface Props {
  sessions?: SessionRecord[];
  active?: boolean;
  defaultCwd: string | null;
  focusedProjectId?: string | null;
  focusedProjectRequest?: number;
  projectOverview?: boolean;
  onOpenSession?: (sessionId: string | null, original?: ConversationOriginalSession) => void | Promise<void>;
  onOpenProjectSettings?: (projectId?: string, target?: "tasks") => void;
}
const PROJECT_KEY = "crewvela-multi-agent-project";
const MAX_MEMBER_MENU_WIDTH = 304;
const ROLE: Record<string, string> = { orchestrator: "方案", worker: "开发", reviewer: "审查", integrator: "集成" };
const draftKey = (project: string, chat?: string) => `crewvela-conversation-draft:${chat ? `${project}:${chat}` : project}`;
const pendingKey = (project: string, chat?: string) => `crewvela-conversation-pending:${chat ? `${project}:${chat}` : project}`;
function stored(key: string): string { try { return localStorage.getItem(key) ?? ""; } catch { return ""; } }
function save(key: string, value: string) { try { if (value) localStorage.setItem(key, value); else localStorage.removeItem(key); } catch { /* The current draft remains available in the editor. */ } }
interface PendingSend extends ConversationSendInput { interrupt: boolean; preserveDraft?: boolean; rememberRecipientId?: string }
interface PlanIssue { kind: "git" | "member" | "codex_login"; message: string }
const CODEX_LOGIN_REQUIRED = "CrewVela 的 Codex 专用目录尚未登录";
const PLAN_VALIDATION_FAILURE = "真实成员已回复，但分工计划未通过校验：";
const MAX_ATTACHMENTS = 8;
const DOCUMENT_EXTENSIONS = ["pdf", "docx", "txt", "md", "csv"];
const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "webp", "gif"];
const VIDEO_EXTENSIONS = ["mp4", "mov", "webm"];
const attachmentDraftKey = (projectId: string, conversationId: string) => `crewvela.team-attachment-draft.${projectId}.${conversationId}`;
const memberSelectionKey = (projectId: string, conversationId: string) => `crewvela.team-member-selection.${projectId}.${conversationId}`;
const chatRecipientKey = (projectId: string, conversationId: string) => `crewvela.team-chat-recipient.${projectId}.${conversationId}`;
function storedMemberSelection(projectId: string, conversationId: string): string[] | null {
  try {
    const value: unknown = JSON.parse(stored(memberSelectionKey(projectId, conversationId)));
    return Array.isArray(value) ? value.filter((id): id is string => typeof id === "string") : null;
  } catch { return null; }
}
const fileName = (path: string) => path.split(/[\\/]/u).pop() || path;
function storedAttachmentDraft(projectId: string, conversationId: string): string[] {
  try {
    const files: unknown = JSON.parse(stored(attachmentDraftKey(projectId, conversationId)));
    return Array.isArray(files) ? files.filter((path): path is string => typeof path === "string").slice(0, MAX_ATTACHMENTS) : [];
  } catch { return []; }
}
const planCanRetry = (round: ConversationRound) => ["failed", "needs_decision", "completed"].includes(round.status);
const hasPlanJson = (body: string) => /^(?:\{\s*(?:"|$)|\[\s*(?:[{"]|$))/u.test(body.trimStart())
  || /^\s*```[ \t]*(?:json\b|\r?\n\s*[\[{])/imu.test(body);
function sendPayload(item: PendingSend): ConversationSendInput {
  return { projectId: item.projectId, expectedConversationId: item.expectedConversationId, clientMessageId: item.clientMessageId,
    body: item.body, participantAgentIds: item.participantAgentIds, mode: item.mode, taskRules: item.taskRules,
    ...(item.attachmentIds?.length ? { attachmentIds: item.attachmentIds } : {}),
    ...(item.chatDispatch ? { chatDispatch: item.chatDispatch } : {}),
    ...(item.managed ? { managed: true } : {}),
    intent: item.intent ?? "chat", nativeCapabilities: item.nativeCapabilities ?? false,
    maxRounds: item.maxRounds ?? (item.intent === "exchange" ? 3 : 1), maxDurationSeconds: item.maxDurationSeconds ?? 600 };
}
function storedPending(id: string, chat?: string): PendingSend | null {
  try {
    const item = JSON.parse(stored(pendingKey(id, chat)));
    return item?.projectId === id && typeof item.clientMessageId === "string" && typeof item.body === "string" && Array.isArray(item.participantAgentIds) && ["discussion", "execution_plan"].includes(item.mode) && typeof item.interrupt === "boolean" ? item : null;
  } catch { return null; }
}
function projectFromStorage(): string | null { return stored(PROJECT_KEY) || null; }

/** One persisted project conversation. It deliberately has no general-command or global-broadcast route. */
export function MultiAgentControl({ sessions = [], active = true, defaultCwd, focusedProjectId, focusedProjectRequest, projectOverview = false, onOpenSession, onOpenProjectSettings }: Props) {
  const [execDrawerOpen, setExecDrawerOpen] = useState(false);
  const [expandedUserMsgs, setExpandedUserMsgs] = useState<Record<string, boolean>>({});
  const { ready: runtimeReady, error: runtimeError, isRuntimeEnabled, refresh: refreshAvailability } = useRuntimeAvailability();
  const [projects, setProjects] = useState<MultiAgentProject[]>([]);
  const [projectId, setProjectId] = useState<string | null>(focusedProjectId ?? projectFromStorage);
  const [snapshot, setSnapshot] = useState<MultiAgentSnapshot | null>(null);
  const [conversation, setConversation] = useState<ConversationSnapshot | null>(null);
  const [activityNow, setActivityNow] = useState(Date.now);
  const [draft, setDraft] = useState("");
  const [draftFiles, setDraftFiles] = useState<string[]>([]);
  const [pickingFiles, setPickingFiles] = useState(false);
  const [archivedRuleId, setArchivedRuleId] = useState("");
  const [requestedView, setWorkView] = useState<"chat" | "tasks" | "taskRecords">("chat");
  const workView = requestedView === "taskRecords" ? "tasks" : requestedView;
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [selectedDeliveryId, setSelectedDeliveryId] = useState<string | null>(null);
  const workViewId = useId();
  const [memberSelection, setMemberSelection] = useState<string[] | null>(null);
  const [chatRecipientId, setChatRecipientId] = useState<string>();
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [healthy, setHealthy] = useState(false);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<PendingSend | null>(null);
  const [membersOpen, setMembersOpen] = useState<"header" | "composer" | null>(null);
  const [composerToolsOpen, setComposerToolsOpen] = useState(false);
  const [composerToolsKeyboard, setComposerToolsKeyboard] = useState(false);
  const [modelMemberId, setModelMemberId] = useState<string>();
  const [membersPosition, setMembersPosition] = useState({ left: 0, top: 0, width: MAX_MEMBER_MENU_WIDTH, maxHeight: 320 });
  const membersId = useId();
  const modelBarId = useId();
  const sendScopeId = useId();
  const composerToolsId = useId();
  const attachmentFormatsId = useId();
  const headerMemberTrigger = useRef<HTMLButtonElement>(null);
  const composerMemberTrigger = useRef<HTMLButtonElement>(null);
  const composerToolsTrigger = useRef<HTMLButtonElement>(null);
  const composerToolsMenu = useRef<HTMLDivElement>(null);
  const membersMenu = useRef<HTMLDivElement>(null);
  const [creatingProject, setCreatingProject] = useState(false);
  const [composingMention, setComposingMention] = useState(false);
  const [mentionCursor, setMentionCursor] = useState(0);
  const [mentionOpen, setMentionOpen] = useState(false);
  const [mentionIndex, setMentionIndex] = useState(0);
  const [mentionMultiple, setMentionMultiple] = useState(false);
  const [mentionSelection, setMentionSelection] = useState<string[]>([]);
  useEffect(() => { if (!mentionOpen) { setMentionMultiple(false); setMentionSelection([]); } }, [mentionOpen]);
  const [showOlder, setShowOlder] = useState(false);
  const [historyMenuOpen, setHistoryMenuOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [planIssues, setPlanIssues] = useState<PlanIssue[]>([]);
  const [loggingIn, setLoggingIn] = useState(false);
  const [loginRetryRound, setLoginRetryRound] = useState<string | null>(null);
  const loginInFlight = useRef(false);
  const loginContext = useRef(0);
  const planAfterLogin = useRef<(() => Promise<void>) | null>(null);
  const planForm = useRef("");
  const [blockedPlanId, setBlockedPlanId] = useState<string | null>(null);
  const [exportNotice, setExportNotice] = useState<{ conversationId: string; filename: string } | null>(null);
  const exporting = useRef(false);
  const [openDelegation, setOpenDelegation] = useState<{ conversationId: string; itemId: string } | null>(null);
  const delegationTrigger = useRef<HTMLButtonElement | null>(null);
  const [historySwitchError, setHistorySwitchError] = useState("");
  const [clearConfirmation, setClearConfirmation] = useState<{ projectId: string; projectName: string; expectedConversationId: string; expectedMessageIds: string[] } | null>(null);
  const [clearError, setClearError] = useState("");
  const [sessionAction, setSessionAction] = useState<{ kind: "rename"; id: string; title: string }
    | { kind: "delete"; id: string; label: string; messageIds: string[] } | null>(null);
  const [sessionActionError, setSessionActionError] = useState("");
  const cancelDelete = useRef<HTMLButtonElement>(null);
  const historyTrigger = useRef<HTMLButtonElement>(null);
  const historyMenu = useRef<HTMLDivElement>(null);
  const cancelClear = useRef<HTMLButtonElement>(null);
  const historyMenuId = useId();
  const request = useRef(0);
  const generation = useRef(0);
  const mutation = useRef(false);
  const operationError = useRef("");
  const currentProject = useRef(projectId); currentProject.current = projectId;
  const currentConversation = useRef<string | undefined>(undefined);
  const editor = useRef<HTMLTextAreaElement>(null);
  const transcript = useRef<HTMLDivElement>(null);
  const followMessages = useRef(true);
  const mounted = useRef(true);
  const setOperationError = (value: string) => { operationError.current = value; setError(value); };
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; request.current++; generation.current++; }; }, []);

  const adoptConversation = useCallback((value: ConversationSnapshot) => {
    const { projectId, id } = value.conversation;
    if (currentConversation.current !== id) {
      loginContext.current++;
      planAfterLogin.current = null;
      setBlockedPlanId(null);
      setLoginRetryRound(null);
      if (!currentConversation.current) {
        for (const storageKey of [draftKey, pendingKey]) {
          const legacy = stored(storageKey(projectId));
          if (legacy && !stored(storageKey(projectId, id))) save(storageKey(projectId, id), legacy);
          if (legacy) save(storageKey(projectId), "");
        }
      }
      currentConversation.current = id;
      setDraft(stored(draftKey(projectId, id)));
      setDraftFiles(storedAttachmentDraft(projectId, id));
      setMemberSelection(storedMemberSelection(projectId, id));
      setChatRecipientId(stored(chatRecipientKey(projectId, id)) || undefined);
      setPending(storedPending(projectId, id));
      setShowOlder(false); setArchivedRuleId(""); setMembersOpen(null); setMentionOpen(false); setMentionSelection([]);
      setPlanIssues([]);
      followMessages.current = true;
    }
    setConversation(value);
  }, []);

  const chooseProject = useCallback((id: string | null) => {
    if (currentProject.current === id) return;
    loginContext.current++;
    planAfterLogin.current = null;
    setBlockedPlanId(null);
    setLoginRetryRound(null);
    setComposingMention(false);
    request.current++; generation.current++;
    currentProject.current = id; currentConversation.current = undefined; setProjectId(id); setSnapshot(null); setConversation(null); setHealthy(false); setLoading(false);
    operationError.current = ""; setError(""); setMemberSelection(null); setMembersOpen(null); setShowOlder(false); setMentionOpen(false);
    setChatRecipientId(undefined);
    setHistoryMenuOpen(false); setHistoryOpen(false); setClearConfirmation(null); setClearError("");
    setSessionAction(null); setSessionActionError("");
    setWorkView("chat"); setSelectedTaskId(null); setSelectedDeliveryId(null);
    setArchivedRuleId("");
    setPlanIssues([]);
    setDraft(id ? stored(draftKey(id)) : ""); setDraftFiles([]); setPending(id ? storedPending(id) : null); followMessages.current = true;
    if (id) save(PROJECT_KEY, id);
  }, []);
  const loadProjects = useCallback(async () => {
    const value = await invoke<MultiAgentProject[]>("ma_list_projects");
    if (!mounted.current) return;
    setProjects(value);
    if (!currentProject.current || !value.some(item => item.id === currentProject.current)) {
      chooseProject(value.find(item => defaultCwd && item.rootPath === defaultCwd)?.id ?? value[0]?.id ?? null);
    }
  }, [chooseProject, defaultCwd]);
  useEffect(() => {
    if (!active) return;
    void loadProjects().catch(reason => { if (mounted.current) setError(`读取协作项目失败：${String(reason)}`); });
  }, [active, loadProjects]);
  useEffect(() => {
    if (!focusedProjectId) return;
    chooseProject(focusedProjectId);
    setWorkView("chat");
  }, [focusedProjectId, focusedProjectRequest, chooseProject]);
  useEffect(() => {
    const changed = (event: Event) => { const id = (event as CustomEvent<{ projectId?: string }>).detail?.projectId; if (id) chooseProject(id); };
    window.addEventListener("crewvela-multi-agent-project-selected", changed);
    return () => window.removeEventListener("crewvela-multi-agent-project-selected", changed);
  }, [chooseProject]);
  const refresh = useCallback(async () => {
    if (!projectId || mutation.current) return;
    const sequence = ++request.current;
    const id = projectId;
    setLoading(true);
    const [project, chat] = await Promise.allSettled([
      Promise.resolve().then(() => invoke<MultiAgentSnapshot>("ma_snapshot", { projectId: id })),
      Promise.resolve().then(() => invoke<ConversationSnapshot>("ma_conversation_snapshot", { projectId: id })),
    ]);
    if (!mounted.current || currentProject.current !== id || sequence !== request.current) return;
    setLoading(false);
    if (project.status === "fulfilled" && project.value.project.id === id) setSnapshot(project.value);
    if (chat.status === "fulfilled" && chat.value.conversation.projectId === id) adoptConversation(chat.value);
    const failures = [project, chat].filter((result): result is PromiseRejectedResult => result.status === "rejected").map(result => String(result.reason));
    if ((project.status === "fulfilled" && project.value.project.id !== id) || (chat.status === "fulfilled" && chat.value.conversation.projectId !== id)) failures.push("返回的项目身份不匹配");
    setHealthy(!failures.length);
    if (failures.length) setError(`刷新协作状态失败：${failures.join("；")}`);
    else setError(operationError.current);
  }, [adoptConversation, projectId]);
  useEffect(() => {
    if (!projectId || !active) return;
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1500);
    return () => { clearInterval(timer); request.current++; };
  }, [active, projectId, refresh]);
  const agents = useMemo(() => snapshot?.agents.filter(agent => agent.projectId === projectId && agent.enabled) ?? [], [snapshot, projectId]);
  const supported = conversation?.supportedDiscussionRuntimes ?? [];
  const manualMembers = useMemo(() => new Set(snapshot?.agentSessions.filter(session => session.projectId === projectId && session.controlMode === "manual").map(session => session.agentId) ?? []), [snapshot, projectId]);
  const eligibleMembers = agents.filter(agent => supported.includes(agent.runtimeKind) && isRuntimeEnabled(agent.runtimeKind)).map(agent => agent.id);
  const availableRecipients = eligibleMembers.filter(id => !manualMembers.has(id));
  const discussionOnlyMembers = new Set(agents.filter(agent => agent.runtimeKind === "antigravity").map(agent => agent.id));
  const eligiblePlanMembers = eligibleMembers.filter(id => !discussionOnlyMembers.has(id));
  const availablePlanMembers = availableRecipients.filter(id => !discussionOnlyMembers.has(id));
  const chatRecipient = agents.find(agent => agent.id === chatRecipientId && availableRecipients.includes(agent.id));
  const rememberChatRecipient = useCallback((project: string, chat: string, recipient?: string) => {
    save(chatRecipientKey(project, chat), recipient ?? "");
    if (mounted.current && currentProject.current === project && currentConversation.current === chat) setChatRecipientId(recipient);
  }, []);
  const selected = runtimeReady && !runtimeError
    ? memberSelection?.filter(id => eligiblePlanMembers.includes(id)) ?? availablePlanMembers
    : [];
  const selectionCount = (id: string) => {
    if (id === conversation?.conversation.id) return selected.length;
    return (projectId ? storedMemberSelection(projectId, id) : null)?.filter(member => eligiblePlanMembers.includes(member)).length
      ?? availablePlanMembers.length;
  };
  const setSelected = (next: string[] | ((current: string[]) => string[])) => {
    if (!projectId || !conversation || !runtimeReady || runtimeError) return;
    const ids = typeof next === "function" ? next(selected) : next;
    save(memberSelectionKey(projectId, conversation.conversation.id), JSON.stringify(ids));
    setMemberSelection(ids);
  };
  planForm.current = JSON.stringify([draft, draftFiles, selected, archivedRuleId, snapshot?.project.rootPath,
    snapshot?.project.defaultBranch, conversation?.rounds.length]);
  useEffect(() => { if (!active) { setMembersOpen(null); setHistoryMenuOpen(false); setHistoryOpen(false); setClearConfirmation(null); setMentionOpen(false); setSessionAction(null); } }, [active]);
  useEffect(() => { setComposerToolsOpen(false); }, [active, projectId, conversation?.conversation.id, workView]);
  useEffect(() => {
    const menu = composerToolsMenu.current;
    const trigger = composerToolsTrigger.current;
    if (!composerToolsOpen || !menu || !trigger) return;
    const items = () => Array.from(menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)'));
    (items()[0] ?? menu).focus();
    const outside = (event: Event) => {
      if (event.target instanceof Node && !menu.contains(event.target) && !trigger.contains(event.target)) {
        setComposerToolsOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.isComposing) return;
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        setComposerToolsOpen(false);
        trigger.focus();
      } else if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
        event.preventDefault();
        setComposerToolsKeyboard(true);
        const buttons = items();
        if (!buttons.length) return;
        const index = buttons.findIndex(item => item === document.activeElement);
        const direction = event.key === "ArrowDown" ? 1 : -1;
        const next = event.key === "Home" || (index < 0 && direction === 1) ? 0
          : event.key === "End" || index < 0 ? buttons.length - 1
          : (index + direction + buttons.length) % buttons.length;
        buttons[next]?.focus();
      }
    };
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("focusin", outside);
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("focusin", outside);
      window.removeEventListener("keydown", onKeyDown, true);
    };
  }, [composerToolsOpen]);
  useEffect(() => {
    if (!historyMenuOpen) return;
    const frame = requestAnimationFrame(() => historyMenu.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus());
    const outside = (event: Event) => {
      if (event.target instanceof Node && !historyMenu.current?.contains(event.target) && !historyTrigger.current?.contains(event.target)) setHistoryMenuOpen(false);
    };
    const key = (event: KeyboardEvent) => {
      if (event.isComposing) return;
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); setHistoryMenuOpen(false); historyTrigger.current?.focus(); }
      if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
        event.preventDefault();
        const items = Array.from(historyMenu.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? []);
        const index = items.findIndex(item => item === document.activeElement);
        const next = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : (index + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
        items[next]?.focus();
      }
    };
    document.addEventListener("pointerdown", outside, true); document.addEventListener("focusin", outside); window.addEventListener("keydown", key, true);
    return () => { cancelAnimationFrame(frame); document.removeEventListener("pointerdown", outside, true); document.removeEventListener("focusin", outside); window.removeEventListener("keydown", key, true); };
  }, [historyMenuOpen]);
  useLayoutEffect(() => {
    if (membersOpen !== "header") return;
    const place = () => {
      const anchor = headerMemberTrigger.current?.getBoundingClientRect();
      const panel = headerMemberTrigger.current?.closest(".collaboration-conversation")?.getBoundingClientRect();
      if (!anchor || !panel) return;
      const width = Math.min(MAX_MEMBER_MENU_WIDTH, Math.max(248, panel.width * .32), panel.width - 28, innerWidth - 16);
      const left = Math.max(8, panel.left + 14, Math.min(anchor.right - width, innerWidth - width - 8));
      const below = innerHeight - anchor.bottom - 16;
      const above = anchor.top - 16;
      const flipped = below < 160 && above > below;
      const maxHeight = Math.max(60, Math.min(340, flipped ? above : below));
      const height = Math.min(membersMenu.current?.scrollHeight ?? 300, maxHeight);
      setMembersPosition({ left, width, maxHeight,
        top: flipped ? anchor.top - height - 8 : anchor.bottom + 8 });
    };
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    const observer = new ResizeObserver(place);
    const panel = headerMemberTrigger.current?.closest(".collaboration-conversation");
    if (panel) observer.observe(panel);
    if (membersMenu.current) observer.observe(membersMenu.current);
    return () => { observer.disconnect(); window.removeEventListener("resize", place); window.removeEventListener("scroll", place, true); };
  }, [membersOpen, agents.length]);
  useEffect(() => { setModelMemberId(undefined); }, [active, projectId]);
  const modelMember = agents.find(agent => agent.id === modelMemberId);
  const closeModelBar = () => {
    setModelMemberId(undefined);
    composerToolsTrigger.current?.focus();
  };
  const runComposerTool = (run: () => void) => {
    setComposerToolsOpen(false);
    composerToolsTrigger.current?.focus();
    run();
  };
  const saveModelSummary = (agentId: string, value: ConversationMemberModelSummary | null) => {
    if (currentProject.current !== projectId) return;
    setConversation(current => {
      if (!current || current.conversation.projectId !== projectId) return current;
      const summaries = { ...current.memberModelSummaries };
      if (value) summaries[agentId] = value;
      else delete summaries[agentId];
      return { ...current, memberModelSummaries: summaries };
    });
    void refresh();
  };
  const memberModelChips = (agentId: string | undefined) => {
    const current = agentId ? conversation?.memberModelSummaries?.[agentId] : undefined;
    return current && <span className="conversation-member-model-summary" role="group" aria-label="成员当前配置" title="成员当前配置 · 下一轮聊天生效">
      {current.model && <small title={`当前模型：${current.model}`}>{current.model}</small>}
      {current.reasoning && <small title={`当前推理级别：${current.reasoning}`}>{current.reasoning}</small>}
    </span>;
  };
  useEffect(() => {
    if (!membersOpen) return;
    const trigger = membersOpen === "header" ? headerMemberTrigger.current : composerMemberTrigger.current;
    const frame = requestAnimationFrame(() => membersMenu.current?.querySelector<HTMLElement>('input:not(:disabled), button:not(:disabled)')?.focus());
    const outside = (event: Event) => {
      if (event.target instanceof Node && !membersMenu.current?.contains(event.target) && !trigger?.contains(event.target)) setMembersOpen(null);
    };
    const key = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.isComposing) return;
      event.preventDefault(); event.stopPropagation(); setMembersOpen(null); trigger?.focus();
    };
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("focusin", outside);
    window.addEventListener("keydown", key, true);
    return () => { cancelAnimationFrame(frame); document.removeEventListener("pointerdown", outside, true); document.removeEventListener("focusin", outside); window.removeEventListener("keydown", key, true); };
  }, [membersOpen]);
  const round = conversation?.rounds.find(conversationIsActive) ?? latestConversationRound(conversation?.rounds ?? []);
  const latestDiscussion = latestConversationRound(conversation?.rounds.filter(item => item.mode === "discussion") ?? []);
  const planningRounds = conversation?.rounds.filter(item => item.mode === "execution_plan") ?? [];
  const roundByRequest = new Map(conversation?.rounds.map(item => [item.userMessageId, item]));
  const planRoundById = new Map(planningRounds.map(item => [item.id, item]));
  const hasPendingPlan = !!conversation?.executionPlans.some(plan => plan.status === "pending");
  const mentionTokens = useMemo(() => agents.filter(agent => supported.includes(agent.runtimeKind) && isRuntimeEnabled(agent.runtimeKind))
    .map(agent => conversationMentionToken(agent, agents)), [agents, supported, isRuntimeEnabled]);
  const canGeneratePlan = hasConversationPlanGoal(draft, mentionTokens);
  const planToolDisabled = busy || (mentionOpen && mentionMultiple) || !healthy || !active || !runtimeReady || !!runtimeError ||
    !canGeneratePlan || !!pending || ["stopping", "unconfirmed"].includes(round?.status ?? "");
  const planHasMentions = planIssues.length > 0 && mentionTokens.some(token => hasMentionToken(draft, token));
  const discussing = conversationIsActive(round);
  const activityTicking = discussing || !!conversation?.messages.some(message=>message.senderType === "agent" && message.status === "needs_decision" && snapshot?.runs.some(run=>run.id === message.runId && ["starting","running","waiting_input","waiting_approval"].includes(run.status)));
  useEffect(() => {
    setActivityNow(Date.now());
    if (!active || !activityTicking) return;
    const timer = window.setInterval(() => setActivityNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [active, activityTicking, projectId]);
  const activityEvents = useMemo(() => {
    const events = new Map((snapshot?.runtimeEvents ?? []).filter(event => event.eventType === "agent_activity").map(event => [event.seq,event]));
    for (const event of conversation?.activityEvents ?? []) events.set(event.seq,event);
    return [...events.values()];
  }, [snapshot?.runtimeEvents, conversation?.activityEvents]);
  const managedTaskIds = new Set(conversation?.executionPlans.flatMap(plan => plan.managed ? plan.taskIds : []) ?? []);
  const conversationMessages = orderedConversationMessages(conversation?.messages ?? [], projectId ?? "");
  const currentTitle = conversationTitle(conversation?.conversation.title,
    conversationMessages.find(message => message.senderType === "user")?.body.trim().slice(0, 80));
  const messages = conversationMessages.filter(message => !message.taskId || !managedTaskIds.has(message.taskId));
  const newConversationBlocked = busy || !!pending || !healthy || !active || !conversation || !!conversation.rounds.some(conversationIsActive);
  const clearBlocked = newConversationBlocked || !conversationMessages.length;
  const deleteBlocked = newConversationBlocked || !!conversation?.executionPlans.some(plan => plan.status === "pending"
    || ["planning", "waiting_login", "running"].includes(plan.managed?.status ?? ""));
  const exportBlocked = !active || !conversation || conversation.conversation.projectId !== projectId || !messages.some(message => message.body.trim() || message.attachments?.length);
  const clearChanged = !!clearConfirmation && (conversation?.conversation.id !== clearConfirmation.expectedConversationId || conversationMessages.length !== clearConfirmation.expectedMessageIds.length || conversationMessages.some(message => !clearConfirmation.expectedMessageIds.includes(message.id)));
  const deleteChanged = sessionAction?.kind === "delete" && (conversation?.conversation.id !== sessionAction.id
    || conversationMessages.length !== sessionAction.messageIds.length
    || conversationMessages.some(message => !sessionAction.messageIds.includes(message.id)));
  useEffect(() => {
    if (!pending || !conversation || conversation.conversation.projectId !== pending.projectId || projectId !== pending.projectId) return;
    if (!conversation.messages.some(message => message.clientMessageId === pending.clientMessageId)) return;
    const conversationId = conversation.conversation.id;
    if (pending.rememberRecipientId) rememberChatRecipient(pending.projectId, conversationId, pending.rememberRecipientId);
    save(pendingKey(pending.projectId, conversationId), "");
    if (!pending.preserveDraft) {
      if (stored(draftKey(pending.projectId, conversationId)) === pending.body) save(draftKey(pending.projectId, conversationId), "");
      setDraft(value => value === pending.body ? "" : value);
      setArchivedRuleId("");
    }
    setPending(null); setOperationError("");
  }, [conversation, pending, projectId, rememberChatRecipient]);
  const visibleMessages = showOlder ? messages : messages.slice(-100);
  const planFailureInTranscript = !!round && round.mode === "execution_plan" && planCanRetry(round)
    && !conversation?.executionPlans.some(plan => plan.roundId === round.id)
    && visibleMessages.some(message => message.id === round.userMessageId);
  const tasks = visibleProjectTasks(snapshot?.tasks ?? []);
  const openProjectTasks = () => { if (snapshot && projectId && active) { setMembersOpen(null); onOpenProjectSettings?.(projectId, "tasks"); } };
  const waitingApproval = round?.status === "waiting_approval" ? snapshot?.approvals.find(approval => approval.projectId === projectId && approval.status === "pending" && messages.some(message => message.roundId === round.id && message.runId === approval.runId && message.senderType === "agent")) : undefined;
  const activeAgent = discussing ? agents.find(agent => conversation?.messages.some(message => message.roundId === round?.id && message.senderType === "agent" && message.agentId === agent.id && (waitingApproval ? message.runId === waitingApproval.runId : message.status === "running"))) : undefined;
  useEffect(() => {
    if (followMessages.current && transcript.current) transcript.current.scrollTop = transcript.current.scrollHeight;
  }, [messages.length, messages[messages.length - 1]?.updatedAt, projectId, conversation?.conversation.id]);
  const changeDraft = (value: string) => { setDraft(value); setPlanIssues([]); if (projectId) save(draftKey(projectId, currentConversation.current), value); };
  const changeDraftFiles = (paths: string[], id: string, conversationId: string) => {
    setDraftFiles(paths);
    save(attachmentDraftKey(id, conversationId), paths.length ? JSON.stringify(paths) : "");
  };
  const pickAttachments = async () => {
    const id = projectId, conversationId = currentConversation.current;
    if (!id || !conversationId || pickingFiles || busy || pending) return;
    setPickingFiles(true);
    setOperationError("");
    try {
      const selected = await open({ multiple: true, directory: false, title: "添加附件", filters: [
        { name: "图片、视频和文档", extensions: [...DOCUMENT_EXTENSIONS, ...IMAGE_EXTENSIONS, ...VIDEO_EXTENSIONS] },
        { name: "图片", extensions: IMAGE_EXTENSIONS },
        { name: "视频", extensions: VIDEO_EXTENSIONS },
        { name: "文档", extensions: DOCUMENT_EXTENSIONS },
      ] });
      if (!selected || !mounted.current || currentProject.current !== id || currentConversation.current !== conversationId) return;
      const paths = [...new Set([...draftFiles, ...(Array.isArray(selected) ? selected : [selected])])];
      if (paths.length > MAX_ATTACHMENTS) {
        setOperationError(`每条消息最多添加 ${MAX_ATTACHMENTS} 个附件。`);
        return;
      }
      changeDraftFiles(paths, id, conversationId);
    } catch (reason) {
      if (mounted.current && currentProject.current === id && currentConversation.current === conversationId) setOperationError(`添加附件失败：${String(reason)}`);
    } finally { if (mounted.current) setPickingFiles(false); }
  };
  const openAttachment = async (message: ConversationMessage, attachment: ConversationAttachment) => {
    try {
      await invoke("ma_conversation_open_attachment", { projectId: message.projectId, conversationId: message.conversationId, messageId: message.id, attachmentId: attachment.id });
    } catch (reason) {
      if (currentConversation.current === message.conversationId) setOperationError(`打开附件失败：${String(reason)}`);
    }
  };
  const mutationCall = async (command: string, args: Record<string, unknown>) => {
    if (mutation.current || !projectId || !active) return null;
    const id = projectId; const epoch = generation.current;
    mutation.current = true; setBusy(true); request.current++; setOperationError("");
    try {
      const value = await invoke<ConversationSnapshot>(command, args);
      if (mounted.current && currentProject.current === id && generation.current === epoch) {
        if (value.conversation.projectId !== id) throw Error("协作结果项目身份不匹配。");
        adoptConversation(value); setHealthy(true); followMessages.current = true;
      }
      return value;
    } catch (reason) {
      if (mounted.current && currentProject.current === id && generation.current === epoch) setOperationError(String(reason));
      throw reason;
    } finally { mutation.current = false; if (mounted.current) setBusy(false); }
  };
  const deliver = async (item: PendingSend) => {
    const conversationId = currentConversation.current;
    if (!conversationId) return;
    try {
      const input = sendPayload(item);
      const result = await mutationCall(item.interrupt ? "ma_conversation_interrupt" : "ma_conversation_send", { input });
      if (!result) return;
      if (item.rememberRecipientId) rememberChatRecipient(item.projectId, conversationId, item.rememberRecipientId);
      save(pendingKey(item.projectId, conversationId), "");
      if (!item.preserveDraft && stored(draftKey(item.projectId, conversationId)) === item.body) save(draftKey(item.projectId, conversationId), "");
      if (!item.preserveDraft && item.attachmentIds?.length) save(attachmentDraftKey(item.projectId, conversationId), "");
      if (mounted.current && currentProject.current === item.projectId && currentConversation.current === conversationId) {
        setPending(null);
        if (!item.preserveDraft) {
          setArchivedRuleId(""); setDraft(value => value === item.body ? "" : value); setMentionOpen(false);
          if (item.attachmentIds?.length) setDraftFiles([]);
        }
        void refresh();
      }
    } catch { /* Retain the same identity and payload for a safe status check/retry. */ }
  };
  const planRetryBlocked = busy || !!pending || discussing || !active || !healthy || !runtimeReady || !!runtimeError || hasPendingPlan;
  const loginCodex = async (resume?: () => Promise<unknown>) => {
    if (!conversation || !projectId || loginInFlight.current || mutation.current) return;
    const expectedConversationId = conversation.conversation.id, epoch = loginContext.current;
    const stillOpen = () => mounted.current && loginContext.current === epoch && currentConversation.current === expectedConversationId;
    loginInFlight.current = true;
    setLoggingIn(true);
    setOperationError("");
    try {
      await invoke("ma_conversation_codex_login", { projectId, expectedConversationId });
      if (stillOpen()) await resume?.();
    } catch (reason) {
      if (stillOpen()) setOperationError(String(reason));
    } finally {
      loginInFlight.current = false;
      if (mounted.current) setLoggingIn(false);
    }
  };
  useEffect(() => {
    if (!loginRetryRound || !conversation || !projectId || discussing || busy || mutation.current || pending || !healthy || !active) return;
    const turns = conversation.messages.filter(other => other.roundId === loginRetryRound &&
      other.senderType === "agent" && other.kind === "codex_login_required" && other.status === "failed")
      .map(other => ({ messageId: other.id, runId: other.runId }));
    setLoginRetryRound(null);
    if (turns.length) void mutationCall("ma_conversation_retry_codex_members", { input: {
      projectId, expectedConversationId: conversation.conversation.id, roundId: loginRetryRound, turns,
    } }).then(() => refresh()).catch(() => { /* The original failure remains available. */ });
  }, [loginRetryRound, conversation, projectId, discussing, busy, pending, healthy, active]);
  const retryPlan = async (failedRound: ConversationRound) => {
    if (planRetryBlocked || mutation.current || !projectId || !conversation) return;
    const conversationId = conversation.conversation.id;
    const stillOpen = () => mounted.current && currentProject.current === projectId && currentConversation.current === conversationId;
    mutation.current = true;
    setBusy(true);
    setOperationError("");
    setPlanIssues([]);
    let item: PendingSend;
    try {
      const input = await invoke<ConversationSendInput>("ma_conversation_plan_retry_input", {
        projectId, expectedConversationId: conversationId, roundId: failedRound.id, clientMessageId: crypto.randomUUID(),
      });
      if (!stillOpen()) return;
      const issues = await invoke<PlanIssue[]>("ma_conversation_plan_preflight", { input });
      if (!stillOpen()) return;
      if (issues.length) {
        setPlanIssues(issues);
        planAfterLogin.current = () => retryPlan(failedRound);
        setOperationError(`暂无法重试分工。${issues.map(issue => issue.message).join(" ")}`);
        return;
      }
      item = { ...input, interrupt: false, preserveDraft: true };
    } catch (reason) {
      if (stillOpen()) setOperationError(String(reason));
      return;
    } finally {
      mutation.current = false;
      if (mounted.current) setBusy(false);
    }
    save(pendingKey(projectId, conversationId), JSON.stringify(item));
    setPending(item);
    await deliver(item);
  };
  const editFailedPlan = (body: string) => {
    if (draft !== body && !draft.startsWith(`${body}\n\n`)) changeDraft(draft.trim() ? `${body}\n\n${draft}` : body);
    requestAnimationFrame(() => editor.current?.focus());
  };
  const submit = async (event?: FormEvent, bodyOverride?: string, requestPlan = false, managed = false) => {
    event?.preventDefault();
    if (mentionOpen && mentionMultiple) return;
    if (mutation.current || pickingFiles || !projectId || !conversation || !healthy || !active || !runtimeReady || runtimeError || pending || ["stopping", "unconfirmed"].includes(round?.status ?? "")) return;
    const body = (bodyOverride ?? draft).trim();
    const conversationId = conversation.conversation.id;
    const stillOpen = () => mounted.current && currentProject.current === projectId && currentConversation.current === conversationId;
    if ((!body && !draftFiles.length) || Array.from(body).length > 32000) { setOperationError("请输入消息或添加附件，消息最多 32000 字符。"); return; }
    try {
      const mode = conversationRequestMode(body, requestPlan ? "execution_plan" : "discussion", mentionTokens);
      const rule = archivedRuleId ? conversation?.archivedTaskRules?.find(value => value.id === archivedRuleId) : undefined;
      const chat = mode === "discussion";
      const ids = conversationRecipients(body, chat ? availableRecipients : selected, agents, projectId, chat ? chatRecipientId : undefined);
      if ((!chat || managed || archivedRuleId) && ids.some(id => discussionOnlyMembers.has(id))) throw Error("Antigravity CLI 目前仅支持团队普通对话。");
      if (ids.some(id => !isRuntimeEnabled(agents.find(agent => agent.id === id)!.runtimeKind))) throw Error("目标成员的 Runtime 已停用，请在协作设置中启用后重试。");
      if (ids.some(id => !supported.includes(agents.find(agent => agent.id === id)!.runtimeKind))) throw Error("所选成员此 Runtime 尚未接入群聊，请调整成员或在原会话中操作。");
      const manual = ids.filter(id => manualMembers.has(id)).map(id => agents.find(agent => agent.id === id)!.name);
      if (!discussing && manual.length) throw Error(`${manual.join("、")} 正由用户接管，请取消选择这些成员，或在成员设置中交还自动控制后重试。`);
      if (archivedRuleId && (!rule || (rule.agentId && !ids.includes(rule.agentId)))) throw Error("本次规则对应的成员未选中，或规则已不可用，请重新选择。");
      if (mode === "execution_plan" && !hasConversationPlanGoal(body, mentionTokens)) throw Error("请填写要完成的目标，再生成分工。");
      if (managed) {
        const target = snapshot?.project;
        if (discussing || !target || target.id !== projectId) throw Error("请等待当前讨论结束后再开始全自动托管。");
        if (rule) throw Error("全自动托管需要独立工程任务，请取消本次聊天附加规则。");
      }
      const routing = mode === "discussion"
        ? conversationChatDispatch(body, agents.filter(agent => ids.includes(agent.id)), rule?.agentId ?? undefined)
        : { intent: conversationRequestIntent(body) };
      const item: PendingSend = { projectId, expectedConversationId: conversation.conversation.id, clientMessageId: crypto.randomUUID(), body, participantAgentIds: ids, mode, ...routing, managed, nativeCapabilities: !managed, taskRules: rule ? { tools: "research", archivedRuleId: rule.id } : undefined, interrupt: discussing };
      if (chat && ids.length === 1 && mentionTokens.some(token => hasMentionToken(body, token))) item.rememberRecipientId = ids[0];
      setPlanIssues([]);
      if (mode === "execution_plan") {
        const checkedForm = planForm.current;
        mutation.current = true;
        setBusy(true);
        setOperationError("");
        try {
          const issues = await invoke<PlanIssue[]>("ma_conversation_plan_preflight", { input: sendPayload(item) });
          if (!stillOpen()) return;
          if (managed && planForm.current !== checkedForm) return;
          if (issues.length) {
            planAfterLogin.current = async () => {
              if (planForm.current === checkedForm) await submit(undefined, bodyOverride, requestPlan, managed);
              else setPlanIssues([]);
            };
            setPlanIssues(issues);
            return;
          }
        } finally {
          mutation.current = false;
          if (mounted.current) setBusy(false);
        }
      }
      if (managed && snapshot) {
        const members = ids.map(id => agents.find(agent => agent.id === id)!);
        const developer = members.find(member => member.role === "worker");
        const reviewer = members.find(member => member.role === "reviewer");
        const division = members.length === 1
          ? `单人托管\n${members[0].name} 负责规划、开发、测试和联调。本轮未安排独立审查。`
          : members.length === 2
            ? `双人托管\n${developer?.name ?? "开发成员"} 负责规划、开发、测试和联调。\n${reviewer?.name ?? "审查成员"} 独立只读审查最终候选提交，退回修复后重新审查。`
            : `团队托管\n${members.map(member => `${member.name} · ${ROLE[member.role]}`).join("\n")}\n每项开发由独立成员只读审查，集成成员负责最终联调。`;
        const trust = members.some(member => member.runtimeKind === "claude_code") ? "\nClaude Code 会确认本轮工作树的信任提示。" : "";
        if (!window.confirm(`${division}\n\n允许本轮成员在独立工作树的分工范围内执行。分工生成后可查看验收命令。CrewVela 验收通过后自动合入 ${snapshot.project.defaultBranch} 分支。\n${snapshot.project.rootPath}${trust}`)) return;
      }
      if (draftFiles.length) {
        mutation.current = true;
        setBusy(true);
        setOperationError("");
        try {
          const attachments = await invoke<ConversationAttachment[]>("ma_conversation_stage_attachments", { projectId, conversationId, paths: draftFiles });
          if (!stillOpen()) return;
          item.attachmentIds = attachments.map(file => file.id);
        } finally {
          mutation.current = false;
          if (mounted.current) setBusy(false);
        }
      }
      save(pendingKey(projectId, item.expectedConversationId), JSON.stringify(item)); setPending(item); await deliver(item);
    } catch (reason) { if (stillOpen()) setOperationError(String(reason)); }
  };
  const control = async (action: "pause" | "cancel") => {
    setHistoryMenuOpen(false);
    try { await mutationCall("ma_conversation_control", { projectId, action, expectedConversationId: conversation?.conversation.id }); void refresh(); } catch { /* Error is visible in the panel. */ }
  };
  const startNewConversation = async () => {
    if (newConversationBlocked || !projectId || !conversation) return;
    const id = projectId, epoch = generation.current, previousId = conversation.conversation.id, previousDraft = draft;
    historyTrigger.current?.focus(); setHistoryMenuOpen(false);
    try {
      const result = await mutationCall("ma_conversation_new", { input: { projectId: id, expectedConversationId: conversation.conversation.id, expectedMessageIds: conversationMessages.map(message => message.id) } });
      if (!result || !mounted.current || currentProject.current !== id || generation.current !== epoch) return;
      const typedDuringRequest = stored(draftKey(id, previousId));
      if (typedDuringRequest !== previousDraft) {
        save(draftKey(id, previousId), previousDraft);
        save(draftKey(id, result.conversation.id), typedDuringRequest);
        setDraft(typedDuringRequest);
      }
      setWorkView("chat"); setArchivedRuleId(""); setShowOlder(false); setMembersOpen(null); setClearConfirmation(null); setClearError(""); setMentionOpen(false);
      editor.current?.focus();
    } catch { /* The failed action remains visible; the previous draft and conversation are retained. */ }
  };
  const switchConversation = async (conversationId: string) => {
    if (newConversationBlocked || !projectId || !conversation || conversationId === conversation.conversation.id) return false;
    const id = projectId, epoch = generation.current;
    setHistorySwitchError("");
    try {
      const result = await mutationCall("ma_conversation_switch", { input: { projectId: id, expectedConversationId: conversation.conversation.id, conversationId } });
      if (!result || currentProject.current !== id || generation.current !== epoch) return false;
      setHistoryOpen(false); setWorkView("chat");
      return true;
    } catch (reason) {
      if (currentProject.current === id && generation.current === epoch) setHistorySwitchError(String(reason));
      return false;
    }
  };
  const startRename = (id: string, label: string) => {
    setSessionActionError("");
    const title = label === "空白会话" ? "" : Array.from(label).slice(0, 60).join("");
    setSessionAction({ kind: "rename", id, title });
  };
  const confirmSessionAction = async (event: FormEvent) => {
    event.preventDefault();
    if (!sessionAction || !projectId || busy) return;
    const target = sessionAction, epoch = generation.current;
    if (target.kind === "rename" ? !target.title.trim() : deleteBlocked || deleteChanged) return;
    setSessionActionError("");
    try {
      const result = target.kind === "rename"
        ? await mutationCall("ma_conversation_rename", { input: { projectId, conversationId: target.id, title: target.title.trim() } })
        : await mutationCall("ma_conversation_delete", { input: { projectId, expectedConversationId: target.id, expectedMessageIds: target.messageIds } });
      if (!result) return;
      if (target.kind === "delete") {
        for (const key of [draftKey, pendingKey, attachmentDraftKey, memberSelectionKey, chatRecipientKey]) save(key(projectId, target.id), "");
      }
      if (!mounted.current || currentProject.current !== projectId || generation.current !== epoch) return;
      if (target.kind === "delete") setWorkView("chat");
      setSessionAction(null);
    } catch (reason) {
      if (mounted.current && currentProject.current === projectId && generation.current === epoch) setSessionActionError(String(reason));
    }
  };
  const confirmClear = async () => {
    if (!clearConfirmation || clearBlocked || clearChanged || clearConfirmation.projectId !== currentProject.current) return;
    const target = clearConfirmation;
    const epoch = generation.current;
    setClearError("");
    try {
      const result = await mutationCall("ma_conversation_clear", { input: { projectId: target.projectId, expectedConversationId: target.expectedConversationId, expectedMessageIds: target.expectedMessageIds } });
      if (result && mounted.current && currentProject.current === target.projectId && generation.current === epoch) {
        const keptDraft = stored(draftKey(target.projectId, target.expectedConversationId));
        save(draftKey(target.projectId, result.conversation.id), keptDraft);
        save(draftKey(target.projectId, target.expectedConversationId), "");
        save(chatRecipientKey(target.projectId, target.expectedConversationId), "");
        const keptSelection = stored(memberSelectionKey(target.projectId, target.expectedConversationId));
        save(memberSelectionKey(target.projectId, result.conversation.id), keptSelection);
        setMemberSelection(storedMemberSelection(target.projectId, result.conversation.id));
        setDraft(keptDraft); setClearConfirmation(null); setShowOlder(false);
      }
    } catch (reason) { if (mounted.current && currentProject.current === target.projectId && generation.current === epoch) setClearError(String(reason)); }
  };
  const exportConversation = async () => {
    if (exportBlocked || exporting.current || !projectId || !conversation) return;
    const conversationId = conversation.conversation.id;
    const stillOpen = () => mounted.current && currentProject.current === projectId && currentConversation.current === conversationId;
    exporting.current = true;
    historyTrigger.current?.focus();
    setHistoryMenuOpen(false);
    setExportNotice(null);
    setOperationError("");
    try {
      const filename = await invoke<string | null>("ma_conversation_export", { projectId, conversationId });
      if (filename && stillOpen()) setExportNotice({ conversationId, filename });
    } catch (reason) {
      if (stillOpen()) setOperationError(`导出会话失败：${String(reason)}`);
    } finally {
      exporting.current = false;
    }
  };
  const applyPlan = async (plan: ConversationExecutionPlan) => {
    if (!projectId) return;
    setBlockedPlanId(null);
    const id = projectId, epoch = generation.current;
    // Persist a stable operation identity before the request; uncertainty must not duplicate engineering tasks.
    const key = `crewvela-conversation-plan:${projectId}:${plan.id}`;
    const clientRequestId = stored(key) || crypto.randomUUID(); save(key, clientRequestId);
    try {
      const result = await mutationCall("ma_conversation_execute_plan", { input: { projectId, planId: plan.id, clientRequestId } });
      if (result?.executionPlans.some(item => item.id === plan.id && item.status === "applied") && currentProject.current === id && generation.current === epoch) { setSelectedTaskId(result.executionPlans.find(item => item.id === plan.id)?.taskIds[0] ?? null); setSelectedDeliveryId(null); setWorkView("tasks"); void refresh(); }
    } catch (reason) {
      if (String(reason).includes(CODEX_LOGIN_REQUIRED) && currentProject.current === id && generation.current === epoch)
        setBlockedPlanId(plan.id);
    }
  };
  const editPlan = (plan: ConversationExecutionPlan) => {
    const prefix = `请调整分工：${plan.summary}\n`;
    changeDraft(draft.startsWith(prefix) ? draft : `${prefix}${draft || "调整要求："}`);
    setWorkView("chat");
    requestAnimationFrame(() => { editor.current?.focus(); editor.current?.setSelectionRange(editor.current.value.length, editor.current.value.length); });
  };
  const returnToChat = (messageId?: string) => {
    setWorkView("chat"); setSelectedDeliveryId(null); followMessages.current = !messageId;
    if (messageId) { setShowOlder(true); requestAnimationFrame(() => transcript.current?.querySelector<HTMLElement>(`[data-message-id="${CSS.escape(messageId)}"]`)?.scrollIntoView({ block: "center" })); }
  };
  const updateExecution = async (action: "pause" | "resume") => {
    if (mutation.current || !projectId || !active || !healthy) return;
    const id = projectId, epoch = generation.current;
    mutation.current = true; setBusy(true); request.current++; setOperationError("");
    try {
      const receipt = await invoke<{ accepted: boolean; message: string }>(action === "pause" ? "ma_pause_project" : "ma_start_project", { projectId: id });
      if (!receipt.accepted) throw Error(receipt.message || "执行状态未确认，请查看记录。");
    } catch (reason) { if (mounted.current && currentProject.current === id && generation.current === epoch) setOperationError(String(reason)); }
    finally { mutation.current = false; if (mounted.current) setBusy(false); if (mounted.current && currentProject.current === id && generation.current === epoch) void refresh(); }
  };
  const openSessionRecord = async (id: string | null, original?: ConversationOriginalSession) => {
    if ((!id && !original) || !onOpenSession) { setOperationError("原会话记录暂不可用。"); return; }
    setOperationError("");
    try { await onOpenSession(id, original); } catch (reason) { setOperationError(`打开原会话失败：${String(reason)}`); }
  };
  const openManagedRunRecord = (run: AgentRun, task: MultiAgentTask) => openSessionRecord(null, {
    projectId: run.projectId,
    runId: run.id,
    source: "managed_task",
    agentName: snapshot?.agents.find(agent => agent.id === run.agentId)?.name ?? "历史成员",
    request: task.description,
    reply: run.outputSummary ?? "",
    error: run.error,
  });
  const openAgentSession = async (message: ConversationMessage, agentName: string) => {
    const belongsToAgent = (item: SessionRecord) => item.multiAgentProjectId === message.projectId
      && item.multiAgentAgentId === message.agentId && !item.retiredAgentRuntime;
    const binding = snapshot?.agentSessions.find(item => item.projectId === message.projectId && item.agentId === message.agentId);
    const session = sessions.find(item => item.id === binding?.appSessionId && belongsToAgent(item))
      ?? sessions.find(belongsToAgent);
    const run = snapshot?.runs.find(item => item.projectId === message.projectId && item.id === message.runId);
    // The host is reused across turns. Only show its terminal when it belongs to this run.
    const terminalMatches = session?.ptyId != null && !session.runtimeCommandHost
      && (!message.runId || (session.id === binding?.appSessionId && binding.currentRunId === message.runId));
    const original: ConversationOriginalSession | undefined = message.runId && !terminalMatches ? {
      projectId: message.projectId,
      runId: message.runId,
      agentName,
      request: messages.find(item => item.id === message.replyToMessageId)?.body ?? "",
      reply: message.senderType === "agent" ? message.body : run?.outputSummary ?? "",
      error: run?.error ?? (message.senderType === "system" ? message.body : null),
    } : undefined;
    await openSessionRecord(session?.id ?? binding?.appSessionId ?? null, original);
  };
  const mentionSegments = useMemo(() => mentionTextSegments(draft, mentionTokens), [draft, mentionTokens]);
  const highlightMentions = !composingMention && mentionSegments.some(segment => segment.mention);
  const mentionMatch = draft.slice(0, mentionCursor).match(/(?:^|\s)@([^@\n]*)$/u);
  const mentionCandidates: AgentMentionOption[] = agents.filter(agent => supported.includes(agent.runtimeKind) && isRuntimeEnabled(agent.runtimeKind)).map(agent => ({ key: agent.id, kind: "agent", agentId: agent.id, token: conversationMentionToken(agent, agents), description: ROLE[agent.role] ?? agent.role, section: "具体 Agent", runtimeKind: agent.runtimeKind, runtimeLabel: RUNTIME_LABELS[agent.runtimeKind], avatarId: agent.avatarId }));
  const mentionOptions = mentionCandidates.filter(option => !mentionMatch?.[1] || option.token.toLocaleLowerCase().includes(mentionMatch[1].toLocaleLowerCase())).sort((a, b) => Number(b.token.slice(1).toLocaleLowerCase().startsWith(mentionMatch?.[1]?.toLocaleLowerCase() ?? "")) - Number(a.token.slice(1).toLocaleLowerCase().startsWith(mentionMatch?.[1]?.toLocaleLowerCase() ?? "")));
  const selectedMentionOptions = mentionCandidates.filter(option => mentionSelection.includes(option.key));
  const toggleMentionSelection = (option: AgentMentionOption) => setMentionSelection(keys => keys.includes(option.key) ? keys.filter(key => key !== option.key) : [...keys, option.key]);
  const insertMention = (option: AgentMentionOption) => {
    const prefix = draft.slice(0, mentionCursor).replace(/@[^@\n]*$/u, option.token + " ");
    const next = prefix + draft.slice(mentionCursor); changeDraft(next); setMentionOpen(false);
    if (option.agentId) setSelected(ids => [...new Set([...ids, option.agentId!])]);
    requestAnimationFrame(() => { editor.current?.focus(); editor.current?.setSelectionRange(prefix.length, prefix.length); });
  };
  const insertSelectedMentions = () => {
    if (!mentionMatch || !selectedMentionOptions.length) return;
    const before = draft.slice(0, mentionCursor).replace(/@[^@\n]*$/u, "");
    const after = draft.slice(mentionCursor);
    const tokens = selectedMentionOptions.filter(option => !hasMentionToken(before + after, option.token)).map(option => option.token);
    const prefix = before + (tokens.length ? tokens.join(" ") + " " : "");
    changeDraft(prefix + after);
    setMentionCursor(prefix.length);
    setSelected(ids => [...new Set([...ids, ...selectedMentionOptions.map(option => option.agentId!)])]);
    setMentionOpen(false);
    requestAnimationFrame(() => { editor.current?.focus(); editor.current?.setSelectionRange(prefix.length, prefix.length); });
  };
  const allMentionsSelected = mentionOptions.length > 0 && mentionOptions.every(option => mentionMultiple ? mentionSelection.includes(option.key) : hasMentionToken(draft, option.token));
  const selectAllMentions = () => {
    if (mentionMultiple) {
      const keys = mentionOptions.map(option => option.key);
      setMentionSelection(current => allMentionsSelected ? current.filter(key => !keys.includes(key)) : [...new Set([...current, ...keys])]);
      return;
    }
    const withoutQuery = draft.slice(0, mentionCursor).replace(/@[^@\n]*$/u, "") + draft.slice(mentionCursor);
    const next = selectMentionTokens(withoutQuery, mentionOptions.map(option => option.token), !allMentionsSelected);
    const ids = mentionOptions.map(option => option.agentId!);
    setSelected(current => allMentionsSelected ? current.filter(id => !ids.includes(id)) : [...new Set([...current, ...ids])]);
    changeDraft(next); setMentionOpen(false);
    requestAnimationFrame(() => { editor.current?.focus(); editor.current?.setSelectionRange(next.length, next.length); });
  };
  const toggleMember = (agent: AgentProfile) => {
    setSelected(ids => ids.includes(agent.id) ? ids.filter(id => id !== agent.id) : [...ids, agent.id]);
  };
  const allRecipientsSelected = availablePlanMembers.length > 0
    && availablePlanMembers.length === selected.length && availablePlanMembers.every(id => selected.includes(id));
  const mentionCount = new Set(mentionTokens.filter(token => hasMentionToken(draft, token))).size;
  const planning = conversationRequestMode(draft, "discussion", mentionTokens) === "execution_plan";
  const showChatRecipient = !planning && !mentionCount && !!chatRecipientId;
  const sendScopeLabel = planning
    ? `分工 · ${mentionCount || selected.length} 位`
    : mentionCount ? `已 @ ${mentionCount} 位`
    : showChatRecipient ? (chatRecipient ? `发给 ${chatRecipient.name}` : "收件成员不可用")
    : `发给全体 · ${availableRecipients.length} 位`;
  const sendLabel = discussing ? "发送插话" : "发送消息";
  const renderPlan = (plan: ConversationExecutionPlan) => <ConversationPlanCard key={plan.id} plan={plan} snapshot={snapshot}
    blocked={busy || loggingIn || !healthy || discussing || !active} paused={snapshot?.project.status === "paused"}
    onCodexLogin={blockedPlanId === plan.id || plan.managed?.status === "waiting_login" ? () => void loginCodex(async () => {
      if (plan.managed) {
        await mutationCall("ma_conversation_resume_managed_after_login", { projectId, expectedConversationId: conversation?.conversation.id, planId: plan.id });
        void refresh();
      } else await applyPlan(plan);
    }) : undefined}
    onConfirm={() => void applyPlan(plan)} onAdjust={() => editPlan(plan)} onTasks={() => {
      setSelectedDeliveryId(plan.managed?.status === "completed" ? plan.id : null);
      setWorkView("tasks");
    }}
    onTaskRecords={id => {
      setSelectedDeliveryId(null);
      setSelectedTaskId(id);
      if (projectOverview && !delegation && window.innerWidth >= 980) {
        setExecDrawerOpen(true);
      } else {
        setWorkView("taskRecords");
      }
    }} />;

  const conversationId = conversation?.conversation.id;
  const delegationsByRun = useMemo(() => {
    const index = new Map<string, { item: ConversationDesignDelegation; phase: "source" | "delivery" | "resume" }>();
    for (const item of conversation?.delegations ?? []) {
      index.set(item.sourceRunId, { item, phase: "source" });
      if (item.deliveryRunId) index.set(item.deliveryRunId, { item, phase: "delivery" });
      if (item.resumeRunId && !index.has(item.resumeRunId)) index.set(item.resumeRunId, { item, phase: "resume" });
    }
    return index;
  }, [conversation?.delegations]);
  const delegation = active && workView === "chat" && openDelegation?.conversationId === conversationId
    ? conversation?.delegations?.find(item => item.id === openDelegation?.itemId) : undefined;
  const closeDelegation = () => {
    setOpenDelegation(null);
    requestAnimationFrame(() => delegationTrigger.current?.focus());
  };
  useEffect(() => {
    setOpenDelegation(value => value?.conversationId === conversationId && active && workView === "chat" ? value : null);
  }, [active, conversationId, workView]);
  const delegationLink = (item: ConversationDesignDelegation) => <button type="button"
    className="conversation-text-button delegation-view-link" aria-expanded={delegation?.id === item.id}
    onClick={event => {
      if (!conversationId) return;
      delegationTrigger.current = event.currentTarget;
      setOpenDelegation({ conversationId, itemId: item.id });
    }}>查看委派<CaretRight size={16} /></button>;

  const showExecDrawer = projectOverview && workView === "chat" && execDrawerOpen && !delegation;
  const projectTitleText = snapshot?.project.name ?? projects.find(project => project.id === projectId)?.name ?? "团队协作";
  const projectSelectEl = <span className="conversation-project-select"><select aria-label="协作项目" title={snapshot?.project.rootPath ? `${snapshot.project.name}\n项目目录：${snapshot.project.rootPath}` : projects.find(project => project.id === projectId)?.name} value={projectId ?? ""} onChange={event => chooseProject(event.target.value || null)}><option value="" disabled>选择协作项目</option>{projects.map(project => <option key={project.id} value={project.id}>{project.name}</option>)}</select><CaretDown size={15} aria-hidden="true" /></span>;
  const createProjectBtnEl = <button type="button" className="conversation-icon" aria-label="创建协作项目" title="创建协作项目" onClick={() => setCreatingProject(true)}><Plus size={16} /></button>;
  const settingsBtnEl = onOpenProjectSettings && <button type="button" className="conversation-icon" aria-label="管理项目与成员" title="管理项目与成员" onClick={() => onOpenProjectSettings(projectId ?? undefined)}><GearSix size={16} /></button>;
  const taskLinkEl = <button type="button" className="conversation-text-button conversation-task-link" disabled={!snapshot || !onOpenProjectSettings} onClick={openProjectTasks}>查看任务 · {tasks.length}<ArrowSquareOut size={14} /></button>;
  const membersTriggerEl = <div className="conversation-members" aria-label="当前团队成员"><button ref={headerMemberTrigger} type="button" className="conversation-members-trigger" aria-label={`选择参与成员，已选 ${selected.length} 位，共 ${agents.length} 位`} aria-expanded={membersOpen === "header"} aria-controls={membersOpen === "header" ? membersId : undefined} onClick={() => setMembersOpen(value => value === "header" ? null : "header")} disabled={!snapshot}>
    <span className="conversation-member-preview" aria-hidden="true">{agents.slice(0, delegation ? 4 : !projectOverview ? 2 : 3).map(agent => <span key={agent.id} data-selected={selected.includes(agent.id)}><AgentAvatar avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative /></span>)}</span><span>{agents.length}</span></button></div>;
  const modeTabsEl = <div className="conversation-modes" role="tablist" aria-label="协作视图">{([['chat', '团队聊天'], ['tasks', '任务执行']] as const).map(([value, label]) => <button type="button" role="tab" id={`${workViewId}-${value}-tab`} aria-label={label} aria-controls={`${workViewId}-${value}-panel`} key={value} aria-selected={workView === value} tabIndex={workView === value ? 0 : -1} onKeyDown={event => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault(); const next = event.key === "Home" ? "chat" : event.key === "End" ? "tasks" : workView === "chat" ? "tasks" : "chat";
    setWorkView(next); setSelectedDeliveryId(null); setMembersOpen(null); setHistoryMenuOpen(false);
    requestAnimationFrame(() => document.getElementById(`${workViewId}-${next}-tab`)?.focus());
  }} onClick={() => { setWorkView(value); setSelectedDeliveryId(null); setMembersOpen(null); setHistoryMenuOpen(false); }}>{value === "chat" ? "对话" : "任务"}{value === "tasks" && <span className="team-task-count">{tasks.length}</span>}</button>)}</div>;
  const historyActionsEl = <div className="conversation-history-actions"><button ref={historyTrigger} type="button" className="conversation-icon" aria-label="讨论记录操作" aria-haspopup="menu" aria-expanded={historyMenuOpen} aria-controls={historyMenuOpen ? historyMenuId : undefined} disabled={!projectId || busy || !active} onClick={() => { setMembersOpen(null); setHistoryMenuOpen(value => !value); }}><DotsThree size={19} /></button>
    {historyMenuOpen && <div ref={historyMenu} id={historyMenuId} className="conversation-history-menu" role="menu" aria-label="讨论记录操作">
    <button type="button" role="menuitem" disabled={newConversationBlocked} title={conversation?.rounds.some(conversationIsActive) ? "团队正在回复，结束后可新建会话" : pending ? "请先核对未确认的发送" : "开始新的团队会话"} onClick={() => void startNewConversation()}><Plus size={17} />新建会话</button>
    <button type="button" role="menuitem" disabled={!conversation} onClick={() => {
      if (!conversation) return;
      historyTrigger.current?.focus(); setHistoryMenuOpen(false); startRename(conversation.conversation.id, currentTitle);
    }}><PencilSimple size={17} />重命名会话</button>
    <button type="button" role="menuitem" disabled={!conversation} onClick={() => { historyTrigger.current?.focus(); setHistoryMenuOpen(false); setHistorySwitchError(""); setHistoryOpen(true); }}><ClockCounterClockwise size={17} />历史会话</button>
    <button type="button" role="menuitem" disabled={exportBlocked || exporting.current} title={exportBlocked ? "暂无可导出的消息" : "将当前团队会话保存为 Markdown"} onClick={() => void exportConversation()}><FileArrowDown size={17} />导出会话</button>
    <button type="button" role="menuitem" disabled={clearBlocked} title={conversation?.rounds.some(conversationIsActive) ? "讨论进行中，结束后可清空" : pending ? "请先核对未确认的发送" : !messages.length ? "暂无讨论记录" : undefined} onClick={() => {
      if (clearBlocked || !projectId || !conversation) return;
      historyTrigger.current?.focus(); setHistoryMenuOpen(false); setClearError("");
      setClearConfirmation({ projectId, projectName: projects.find(project => project.id === projectId)?.name ?? "当前项目", expectedConversationId: conversation.conversation.id, expectedMessageIds: conversationMessages.map(message => message.id) });
    }}><Broom size={17} />清空讨论记录</button>
    <button type="button" role="menuitem" className="conversation-delete-action" disabled={deleteBlocked} title={deleteBlocked ? "请先处理本会话的讨论、分工或托管" : undefined} onClick={() => {
      if (deleteBlocked || !conversation) return;
      historyTrigger.current?.focus(); setHistoryMenuOpen(false); setSessionActionError("");
      setSessionAction({ kind: "delete", id: conversation.conversation.id, label: currentTitle, messageIds: conversationMessages.map(message => message.id) });
    }}><Trash size={17} />删除会话</button>{discussing && <><button type="button" role="menuitem" disabled={busy || round?.status === "stopping"} onClick={()=>void control("pause")}><Pause size={17}/>暂停讨论</button><button type="button" role="menuitem" disabled={busy || round?.status === "stopping"} onClick={()=>void control("cancel")}><Stop size={17}/>结束本轮</button></>}</div>}
  </div>;
  const switcherEl = active && workView === "chat" && projectId && conversation && <ConversationSwitcher key={projectId}
    projectId={projectId} currentId={conversation.conversation.id} currentTitle={currentTitle} blocked={newConversationBlocked} busy={busy}
    memberCount={selectionCount} onSwitch={switchConversation} onRename={startRename} />;

  return <div className={`collaboration-project-host${projectOverview ? " has-project-navigation" : ""}`} data-multi-agent-style="grok_noir" data-surface={projectOverview ? "bot" : "docked"}>
    {projectOverview && <CollaborationProjectNavigation active={active} projects={projects} projectId={projectId}
      onSelect={chooseProject} onCreate={() => setCreatingProject(true)} onRename={async (id, name) => {
        const updated = await invoke<MultiAgentProject>("ma_rename_project", { projectId: id, name });
        if (mounted.current) {
          setProjects(items => items.map(item => item.id === updated.id ? { ...item, name: updated.name } : item));
          setSnapshot(current => current?.project.id === updated.id
            ? { ...current, project: { ...current.project, name: updated.name } } : current);
          if (projectId === updated.id && currentProject.current === updated.id) void refresh();
        }
        return updated;
      }} />}
    <div className={`conversation-delegation-layout${delegation ? " has-delegation" : showExecDrawer ? " has-grok-drawer" : ""}`}>
    <section className="collaboration-conversation team-work-views" aria-label="团队协作会话">
    <header className="conversation-header is-grok-noir">
      {projectOverview ? (
        <div className="grok-topbar">
          <div className="grok-top-left">
            <h1 className="collaboration-project-title" title={snapshot?.project.rootPath}>{projectTitleText}</h1>
            {switcherEl}
          </div>
          <div className="grok-top-center">{modeTabsEl}</div>
          <div className="grok-top-right">
            {!showExecDrawer && onOpenProjectSettings && <div className="grok-top-task-wrap">{taskLinkEl}</div>}
            {membersTriggerEl}
            {workView === "chat" && !delegation && (
              <button type="button" className={`conversation-icon grok-drawer-toggle${execDrawerOpen ? " is-active" : ""}`}
                aria-label={execDrawerOpen ? "收起右侧执行画布" : "展开右侧执行画布"}
                aria-pressed={execDrawerOpen}
                title={execDrawerOpen ? "收起右侧执行画布" : "展开右侧执行画布"}
                onClick={() => setExecDrawerOpen(value => !value)}>
                <SidebarSimple size={16} style={{ transform: "scaleX(-1)" }} aria-hidden="true" />
              </button>
            )}
            {historyActionsEl}
            {settingsBtnEl}
          </div>
        </div>
      ) : (
        <>
          <div className="grok-topbar is-docked">
            <div className="grok-top-left">{projectSelectEl}</div>
            <div className="grok-top-center">{modeTabsEl}</div>
            <div className="grok-top-right">
              {membersTriggerEl}
              {historyActionsEl}
              {settingsBtnEl}
            </div>
          </div>
          <div className="grok-docked-subbar">
            <div className="grok-docked-subbar-left">{switcherEl}</div>
            <div className="grok-docked-subbar-right">
              {onOpenProjectSettings && taskLinkEl}
              {createProjectBtnEl}
            </div>
          </div>
        </>
      )}
    </header>
    {membersOpen === "header" && createPortal(<div id={membersId} ref={membersMenu} className="conversation-member-menu-wrap"
      style={{ left:membersPosition.left, top:membersPosition.top, width:membersPosition.width, "--conversation-member-menu-height":`${membersPosition.maxHeight}px` } as React.CSSProperties}>
      <div className="conversation-member-menu" role="group" aria-label="选择参与成员"><header>参与成员</header>{agents.map(agent => {
      const eligible = supported.includes(agent.runtimeKind) && isRuntimeEnabled(agent.runtimeKind) && !manualMembers.has(agent.id);
      const discussionOnly = discussionOnlyMembers.has(agent.id);
      const reason = !runtimeReady ? "正在读取 Runtime 状态" : !isRuntimeEnabled(agent.runtimeKind) ? "Runtime 已停用" : !supported.includes(agent.runtimeKind) ? "此 Runtime 尚未接入群聊" : manualMembers.has(agent.id) ? "用户接管中" : discussionOnly ? "仅团队聊天" : "";
      return <div key={agent.id} className="conversation-member-entry">
        <label title={`${agent.name} · ${RUNTIME_LABELS[agent.runtimeKind]}${reason ? ` · ${reason}` : ""}`}><input type="checkbox" aria-label={agent.name} checked={selected.includes(agent.id)} disabled={discussionOnly || (!eligible && !selected.includes(agent.id)) || busy} onChange={() => toggleMember(agent)} /><AgentAvatar avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative /><span className="conversation-member-name"><strong>{agent.name}</strong><small>{ROLE[agent.role] ?? agent.role}{reason && ` · ${reason}`}{activeAgent?.id === agent.id && " · 正在回复"}</small></span><Check size={18} className="conversation-member-check" aria-hidden="true" /></label>
      </div>;
    })}{!agents.length && <p>还没有团队成员。{onOpenProjectSettings && <button type="button" className="conversation-text-button" onClick={() => { setMembersOpen(null); onOpenProjectSettings(projectId ?? undefined); }}>添加成员</button>}</p>}</div>
    </div>, document.body)}

    {(error || runtimeError) && <div className="conversation-error" role="alert"><WarningCircle size={17} /><span>{error || `读取 Runtime 启用状态失败：${runtimeError}`}</span><button type="button" disabled={busy || loading} onClick={() => { void loadProjects().catch(reason => setError(String(reason))); void refreshAvailability(); void refresh(); }}>重新读取</button></div>}
    {exportNotice && exportNotice.conversationId === conversation?.conversation.id && <div className="conversation-progress conversation-export-status" role="status">已导出：{exportNotice.filename}</div>}
    <div id={`${workViewId}-chat-panel`} className="team-chat-panel" role="tabpanel" aria-labelledby={`${workViewId}-chat-tab`} hidden={workView !== "chat"}><div className="conversation-transcript" role="log" aria-label="团队对话" aria-live="polite" ref={transcript} onScroll={event => { const node = event.currentTarget; followMessages.current = node.scrollHeight - node.scrollTop - node.clientHeight < 90; }}>
      <div className="conversation-transcript-inner">
      {!projectId ? <div className="conversation-empty"><ChatCircleText size={30} /><h2>让 Agent 组成团队</h2><p>创建一个协作项目，选择成员，在这里讨论或分工开发。</p><button type="button" className="conversation-primary" onClick={() => setCreatingProject(true)}>创建协作项目</button></div>
        : !conversation ? <p className="conversation-empty" role="status">{loading ? "正在读取协作会话…" : "协作会话尚未读取，请重试。"}</p>
        : null}
      {messages.length > 100 && !showOlder && <button type="button" className="conversation-text-button conversation-older" onClick={() => setShowOlder(true)}>查看更早的 {messages.length - 100} 条消息</button>}
      {visibleMessages.map((message, index) => {
        const agent = agents.find(item => item.id === message.agentId) ?? snapshot?.agents.find(item => item.id === message.agentId);
        const plan = conversation?.executionPlans.find(item => item.messageId === message.id);
        const requestRound = message.senderType === "user" ? roundByRequest.get(message.id) : undefined;
        const planning = requestRound?.mode === "execution_plan" ? requestRound : undefined;
        const recipientId = requestRound?.chatDispatch?.kind === "assigned" ? requestRound.chatDispatch.agentId
          : requestRound?.mode === "discussion" && requestRound.participantAgentIds.length === 1 ? requestRound.participantAgentIds[0] : undefined;
        const planned = planning && conversation?.executionPlans.find(item => item.roundId === planning.id);
        const planReplyRound = message.senderType === "agent" && message.roundId ? planRoundById.get(message.roundId) : undefined;
        const generatingPlan = !plan && conversationIsActive(planReplyRound);
        const rawPlanReply = !!message.body.trim() && (!!plan || (!!planReplyRound && hasPlanJson(message.body)));
        const planFailed = !!planning && !planned && planCanRetry(planning);
        const validationError = planFailed && planning?.stopReason?.startsWith(PLAN_VALIDATION_FAILURE);
        const planReason = validationError ? planning?.stopReason?.slice(PLAN_VALIDATION_FAILURE.length).replace(/；可补充约束后重试$/u, "") : planning?.stopReason;
        const openOriginal = message.agentId ? () => void openAgentSession(message, agent?.name ?? "历史成员") : undefined;
        const delegated = message.runId ? delegationsByRun.get(message.runId) : undefined;
        if (delegated && conversationId && delegated.item.checkpointAt
            && (delegated.phase !== "resume" || message.status === "completed")) {
          const { item, phase } = delegated;
          const stamp = (phase === "source" ? item.checkpointAt : phase === "delivery" ? item.acceptedAt : item.resumedAt) ?? message.createdAt;
          const body = phase === "source" ? item.brief : phase === "delivery" ? message.body || `已完成设计并提交「${item.filename ?? item.title}」。`
            : message.body || `已从检查点续跑「${item.title}」。`;
          return <article className="conversation-message agent delegation-message" data-message-id={message.id} key={message.id}>
            {agent ? <AgentAvatar className="conversation-avatar" avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative /> : <span className="conversation-avatar"><Robot size={21} /></span>}
            <div className="conversation-message-content">
              <header><strong>{agent?.name ?? "历史成员"}</strong>{memberModelChips(agent?.id)}<time dateTime={new Date(stamp).toISOString()}>{new Date(stamp).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })}</time></header>
              <p className="conversation-message-body">{body}{phase === "source" && delegationLink(item)}</p>
              <div className="delegation-message-actions">
                {openOriginal && <button type="button" className="conversation-text-button" onClick={openOriginal}>查看原会话<ArrowSquareOut size={13} /></button>}
                <ConversationMessageCopy body={message.body || body} />
              </div>
            </div>
          </article>;
        }
        if (delegation && !delegated && message.senderType === "system" && message.status === "running" && agent) {
          const task = tasks.find(task => task.id === message.taskId);
          return <article className="conversation-message delegation-message delegation-parallel" key={message.id} data-message-id={message.id}>
            <AgentAvatar className="conversation-avatar" avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative />
            <div className="conversation-message-content">
              <header><strong>{agent.name}</strong><span>· 执行中</span></header>
              <p className="conversation-message-body">{message.body || task?.description || task?.title}</p>
              {openOriginal && <div className="delegation-message-actions"><button type="button" className="conversation-text-button" onClick={openOriginal}>查看原会话<ArrowSquareOut size={13} /></button></div>}
            </div>
          </article>;
        }
        if (message.senderType === "system") return <div className={`conversation-event ${message.status}`} key={message.id}><span className="conversation-status-dot" /><p>{message.body || CONVERSATION_STATUS[message.status]}{delegated?.phase === "source" && delegationLink(delegated.item)}</p>{openOriginal && <button type="button" className="conversation-text-button" onClick={openOriginal}>查看原会话<ArrowSquareOut size={13} /></button>}</div>;
        const previous = visibleMessages[index - 1];
        const continuation = !!plan && previous?.senderType === "agent" && previous.agentId === message.agentId;
        const fromUser = message.senderType === "user";
        const attachments = !!message.attachments?.length && <div className="conversation-attachments" aria-label="消息附件">{message.attachments.map(file => {
          const source = { kind: "message" as const, projectId: message.projectId,
            conversationId: message.conversationId, messageId: message.id, attachmentId: file.id };
          if (file.mediaType.startsWith("image/")) return <ConversationImageAttachment key={file.id} name={file.name} source={source} onOpenFile={() => void openAttachment(message, file)} />;
          if (file.mediaType.startsWith("video/")) return <ConversationVideoAttachment key={file.id} name={file.name} source={source} />;
          return <button key={file.id} type="button" title={file.name} onClick={() => void openAttachment(message, file)}>
            <Paperclip size={15} /><span>{file.name}</span>
          </button>;
        })}</div>;
        return <article className={`conversation-message ${message.senderType} ${message.status}${continuation ? " plan-continuation" : ""}`} key={message.id} data-message-id={message.id}>
          {!fromUser && (agent ? <AgentAvatar className="conversation-avatar" avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative /> : <span className="conversation-avatar"><Robot size={21} /></span>)}
          <div className="conversation-message-content"><header>
            {!fromUser && <strong>{agent?.name ?? "历史成员"}</strong>}
            {!fromUser && memberModelChips(agent?.id)}
            {requestRound?.chatDispatch?.kind === "assigned" && requestRound.participantAgentIds.length > 1 && <small>发给 {requestRound.participantAgentIds.length} 位</small>}
            {recipientId && <small>接手成员 · {snapshot?.agents.find(item => item.id === recipientId)?.name ?? "历史成员"}</small>}
            {fromUser && <span className="conversation-avatar conversation-user-avatar" role="img" aria-label="我"><User size={21} aria-hidden="true" /></span>}
          </header>
            {fromUser ? ((!!message.body && !plan) || attachments) && (() => {
              const isLongUserMsg = !!message.body && !plan && (message.body.length > 160 || message.body.split("\n").length > 3);
              const isExpanded = !!expandedUserMsgs[message.id];
              return <div className="conversation-user-bubble">
                {!!message.body && !plan && (
                  isLongUserMsg ? (
                    <>
                      <div className={`grok-user-body${isExpanded ? "" : " is-clamped"}`}>
                        <ConversationMarkdown body={message.body} mentionTokens={mentionTokens} />
                      </div>
                      <button
                        type="button"
                        className="grok-more-link"
                        aria-expanded={isExpanded}
                        onClick={() => setExpandedUserMsgs(current => ({ ...current, [message.id]: !current[message.id] }))}
                      >
                        {isExpanded ? "收起完整需求" : "展开完整需求"}
                      </button>
                    </>
                  ) : <ConversationMarkdown body={message.body} mentionTokens={mentionTokens} />
                )}
                {attachments}
              </div>;
            })() : <>
              {!plan && <ConversationMemberActivity agentName={agent?.name ?? "历史成员"} activity={conversationMemberActivity(message,snapshot?.runs.find(run=>run.id===message.runId),activityEvents,snapshot?.approvals ?? [],activityNow)} now={activityNow} onOpen={message.runId ? openOriginal : undefined} onStop={message.roundId === round?.id && discussing && ["running","waiting_approval"].includes(message.status) ? ()=>void control("cancel") : undefined} stopDisabled={busy || !active || !healthy || round?.status === "stopping"} completedLabel={planReplyRound ? "规划成员已回复" : undefined}>{message.body && !generatingPlan && !rawPlanReply ? <ConversationMarkdown body={message.body}/> : null}</ConversationMemberActivity>}
              {attachments}
            </>}
            {message.kind === "codex_login_required" && message.status === "failed" && <div className="conversation-progress">
              <span>{loginRetryRound === message.roundId ? "已登录，等待本轮结束后重试失败成员" : loggingIn ? "正在等待 Codex 登录…" : CODEX_LOGIN_REQUIRED}</span>
              <button type="button" className="conversation-text-button" disabled={loggingIn || !!loginRetryRound || busy || !!pending || !healthy || !active || (discussing && (round?.id !== message.roundId || !!planReplyRound))}
                onClick={() => void loginCodex(async () => {
                  if (planReplyRound) await retryPlan(planReplyRound);
                  else setLoginRetryRound(message.roundId);
                })}>登录 Codex</button>
            </div>}
            {message.queuedInput && <div className="conversation-progress"><span>{message.status === "queued" ? "等待前一轮确认停止后发送" : message.status === "cancelled" ? "这条消息尚未发送，已取消" : "这条消息尚未发送，等待你确认继续"}</span>{message.status === "needs_decision" && <button type="button" className="conversation-text-button" disabled={busy || !!pending || !healthy || !active || discussing} onClick={() => void mutationCall("ma_conversation_send", { input: message.queuedInput }).then(() => refresh()).catch(() => {})}>继续发送</button>}</div>}
            {plan && renderPlan(plan)}
            {rawPlanReply && !generatingPlan && <details className="conversation-raw-plan">
              <summary>查看原始回复</summary>
              <pre>{message.body}</pre>
              <ConversationMessageCopy body={message.body} />
            </details>}
            {planning?.managedTarget && !planned && <div className="conversation-progress conversation-plan-progress">
              <strong role="status">{conversationIsActive(planning) ? "全自动托管 · 正在生成分工" : "全自动托管 · 未交付"}</strong>
              {!conversationIsActive(planning) && planning.stopReason && <p>{planning.stopReason}</p>}
            </div>}
            {planning && !planning.managedTarget && <div className="conversation-progress conversation-plan-progress">
              <strong role="status">{planned?.status === "applied" ? "分工已确认，可在「任务执行」查看进度"
                : planned?.status === "pending" ? "方案已生成，待确认后执行"
                : planned?.status === "superseded" ? "该分工已被更新"
                : planFailed ? validationError ? "分工未通过校验，开发尚未开始" : "分工未生成，开发尚未开始"
                : ["queued", "running"].includes(planning.status) ? "正在拟定分工，开发尚未开始" : `生成分工 · ${CONVERSATION_STATUS[planning.status]}`}</strong>
              {planReason && <p>{planReason}</p>}
              {planFailed && <div className="conversation-plan-actions">
                <button type="button" className="conversation-text-button" disabled={planRetryBlocked} title={hasPendingPlan ? "已有待确认分工，请先处理当前方案" : undefined} onClick={() => void retryPlan(planning)}>重试生成分工</button>
                <button type="button" className="conversation-text-button" onClick={() => editFailedPlan(message.body)}>编辑分工请求</button>
              </div>}
            </div>}
            {message.senderType === "agent" && message.runId && <ConversationApprovalRequests
              approvals={(snapshot?.approvals ?? []).filter(approval => approval.projectId === message.projectId && approval.runId === message.runId && approval.status === "pending" && approval.approvalType === "runtime_permission" && (!message.taskId || approval.taskId === message.taskId))}
              disabled={busy || !active || !healthy || message.roundId !== round?.id || round?.status === "stopping"}
              onChanged={() => void refresh()}
              onOpen={openOriginal}
              onStop={message.roundId === round?.id && discussing ? () => void control("cancel") : undefined}
            />}
            {!plan && !generatingPlan && !rawPlanReply && !!message.body.trim() && <ConversationMessageCopy body={message.body}/>}
          </div>
        </article>;
      })}
      {conversation?.executionPlans.filter(plan => !messages.some(message => message.id === plan.messageId)).map(renderPlan)}
      {snapshot?.runs.filter(run => run.sessionLocator?.stopUnverified === true).map(run => <UnverifiedRunNotice key={run.id} run={run} taskTitle={snapshot.tasks.find(task => task.id === run.taskId)?.title} onConfirmed={refresh} />)}
      </div>
    </div>

    <footer className="conversation-footer">
      {planIssues.length > 0 && <div className="conversation-plan-issues" role="alert">
        <strong>暂无法生成分工，开发尚未开始</strong>
        <ul>{planIssues.map(issue => <li key={issue.message}>{issue.message}</li>)}</ul>
        {planIssues.some(issue => issue.kind === "codex_login") && <button type="button" className="conversation-text-button"
          disabled={busy || loggingIn || !healthy || !active} onClick={() => void loginCodex(planAfterLogin.current ?? undefined)}>{loggingIn ? "等待登录…" : "登录 Codex"}</button>}
        {planIssues.some(issue => issue.kind === "member") && <>
          {planHasMentions && <p>正文里指定了 @成员，请把缺少的成员也加入 @ 列表。</p>}
          <button type="button" className="conversation-text-button" onClick={() => {
            if (planHasMentions) editor.current?.focus();
            else setMembersOpen("composer");
          }}>{planHasMentions ? "编辑 @成员" : "调整参与成员"}</button>
        </>}
      </div>}
      {round && !planFailureInTranscript && ((!["running", "queued", "completed"].includes(round.status)) || round.stopReason) && <div className="conversation-progress" role="status"><span className={`conversation-status-dot ${round.status}`} /><span>{activeAgent ? `${activeAgent.name} · ` : ""}{round.status === "completed" ? "已回复" : CONVERSATION_STATUS[round.status]}{round.stopReason ? ` · ${round.stopReason}` : ""}</span>{discussing && <button className="conversation-text-button" type="button" disabled={busy || round.status === "stopping"} onClick={() => void control("pause")}><Pause size={13} />{round.status === "unconfirmed" ? "重新核对停止" : "暂停讨论"}</button>}{discussing && <button type="button" className="conversation-text-button" aria-label="结束本轮讨论" disabled={busy || round.status === "stopping"} onClick={() => void control("cancel")}><Stop size={13} />结束本轮</button>}{["paused", "cancelled"].includes(round.status) && <button className="conversation-text-button" type="button" disabled={busy || !!pending || !healthy} onClick={() => editor.current?.focus()}>继续输入</button>}</div>}
      {pending && <div className="conversation-send-pending" role="status"><span>{busy ? "正在发送…" : "本次发送结果未确认，重新核对会使用同一消息编号。"}</span>{!busy && <button type="button" onClick={() => void deliver(pending)}>核对并重试</button>}</div>}
      {active && workView === "chat" && projectId && modelMember && <ConversationMemberModels
        id={modelBarId} key={`${projectId}:${modelMember.id}`} projectId={projectId} member={modelMember} members={agents}
        onSelectMember={setModelMemberId} onClose={closeModelBar} onSaved={saveModelSummary} />}
      <ResizableConversationComposer onSubmit={event => void submit(event)}>
         {!!draftFiles.length && <div className="conversation-attachments conversation-draft-attachments" aria-label="待发送附件">{draftFiles.map(path => {
           const name = fileName(path);
           const extension = name.split(".").pop()?.toLowerCase() ?? "";
           const removeDisabled = busy || !!pending || pickingFiles;
           const remove = () => { if (projectId && currentConversation.current) changeDraftFiles(draftFiles.filter(file => file !== path), projectId, currentConversation.current); };
           const source = { kind: "draft" as const, path };
           const key = `${conversation?.conversation.id}:${path}`;
           if (IMAGE_EXTENSIONS.includes(extension)) return <ConversationImageAttachment key={key} name={name} source={source} onRemove={remove} removeDisabled={removeDisabled} />;
           if (VIDEO_EXTENSIONS.includes(extension)) return <ConversationVideoAttachment key={key} name={name} source={source} onRemove={remove} removeDisabled={removeDisabled} />;
           return <button key={path} type="button" title={name} aria-label={`移除附件 ${name}`} disabled={removeDisabled} onClick={remove}>
             <Paperclip size={15} /><span>{name}</span><X size={12} />
           </button>;
         })}</div>}
        <div className="conversation-editor multi-agent-composer-editor" data-highlighted={highlightMentions}>
          {highlightMentions && <ConversationMentionHighlight textareaRef={editor} segments={mentionSegments}/> }
          <textarea ref={editor} onCompositionStart={() => setComposingMention(true)} onCompositionEnd={() => setComposingMention(false)} aria-label={discussing ? "给团队插话" : "给团队发消息"} role="combobox" aria-expanded={mentionOpen && !!mentionMatch && (mentionOptions.length > 0 || mentionMultiple)} aria-controls="conversation-mentions" aria-activedescendant={mentionOpen && mentionOptions.length ? `conversation-mentions-option-${Math.min(mentionIndex, mentionOptions.length - 1)}` : undefined} value={draft} placeholder="给团队发消息，@ 指定成员" disabled={!projectId} onChange={event => { changeDraft(event.target.value); setMentionCursor(event.target.selectionStart); setMentionIndex(0); setMentionOpen(true); }} onSelect={event => { if (event.currentTarget.selectionStart !== mentionCursor) { setMentionCursor(event.currentTarget.selectionStart); setMentionOpen(true); } }} onKeyDown={event => {
            if (event.nativeEvent.isComposing || event.keyCode === 229) return;
            if (mentionOpen && mentionMatch && (mentionOptions.length || mentionMultiple)) {
              if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); setMentionOpen(false); return; }
              if (mentionOptions.length && ["ArrowDown", "ArrowUp"].includes(event.key)) { event.preventDefault(); setMentionIndex(value => (value + (event.key === "ArrowDown" ? 1 : -1) + mentionOptions.length) % mentionOptions.length); return; }
              if (mentionMultiple && mentionOptions.length && event.key === " ") { event.preventDefault(); toggleMentionSelection(mentionOptions[Math.min(mentionIndex, mentionOptions.length - 1)]); return; }
              if (event.key === "Enter" || event.key === "Tab") { event.preventDefault(); if (mentionMultiple) insertSelectedMentions(); else insertMention(mentionOptions[Math.min(mentionIndex, mentionOptions.length - 1)]); return; }
            }
            if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); void submit(); }
          }} />
        </div>
        {mentionOpen && mentionMatch && (mentionOptions.length > 0 || mentionMultiple) && <AgentMentionMenu id="conversation-mentions" options={mentionOptions} activeIndex={Math.min(mentionIndex, mentionOptions.length - 1)} onHighlight={setMentionIndex} onSelect={mentionMultiple ? toggleMentionSelection : insertMention} allSelected={allMentionsSelected} onSelectAll={selectAllMentions} multiple={mentionMultiple} selectedKeys={mentionSelection} selectedCount={selectedMentionOptions.length} onMultipleChange={enabled => { if (enabled !== mentionMultiple) { setMentionMultiple(enabled); setMentionSelection([]); } }} onConfirm={insertSelectedMentions} />}
        <div className="conversation-composer-actions">
          {!!conversation?.archivedTaskRules?.length && <label className="conversation-task-rule">本次规则<select aria-label="本次规则" title={conversation.archivedTaskRules.find(rule => rule.id === archivedRuleId)?.instructions ?? "仅在本条请求生效，不自动延续到下一条"} value={archivedRuleId} disabled={busy || !!pending} onChange={event => setArchivedRuleId(event.target.value)}><option value="">无附加规则</option>{conversation.archivedTaskRules.map(rule => <option key={rule.id} value={rule.id}>{rule.agentId ? agents.find(agent => agent.id === rule.agentId)?.name ?? "历史成员" : "项目"} · {rule.label} · {new Date(rule.createdAt).toLocaleString("zh-CN")}</option>)}</select></label>}
          <div className="conversation-composer-plan-actions">
            <div className="conversation-composer-tools">
              <button ref={composerToolsTrigger} type="button" className="conversation-icon conversation-composer-tools-trigger"
                aria-label="聊天工具" title="聊天工具" aria-haspopup="menu" aria-expanded={composerToolsOpen}
                aria-controls={composerToolsOpen ? composerToolsId : undefined} disabled={!active}
                onClick={event => {
                  setComposerToolsKeyboard(event.detail === 0);
                  setComposerToolsOpen(value => !value);
                }}
                onKeyDown={event => {
                  if (event.key === "ArrowDown" && !composerToolsOpen) {
                    event.preventDefault();
                    setComposerToolsKeyboard(true);
                    setComposerToolsOpen(true);
                  }
                }}><Plus size={16} weight="bold" aria-hidden="true" /></button>
              {composerToolsOpen && <div className="conversation-composer-tools-popup" data-keyboard={composerToolsKeyboard}>
                <div ref={composerToolsMenu} id={composerToolsId} className="conversation-composer-tools-menu"
                  role="menu" aria-label="聊天工具" tabIndex={-1}>
                  <button type="button" role="menuitem" tabIndex={-1} className="conversation-composer-attachment-tool" aria-describedby={attachmentFormatsId}
                    disabled={!projectId || !conversation || !active || busy || !!pending || pickingFiles}
                    onClick={() => runComposerTool(() => void pickAttachments())}>
                    <Paperclip size={17} aria-hidden="true" />添加附件
                  </button>
                  <button type="button" role="menuitem" tabIndex={-1} title={canGeneratePlan ? undefined : "请填写要完成的目标，再生成分工"}
                    disabled={planToolDisabled}
                    onClick={() => runComposerTool(() => void submit(undefined, undefined, true))}>
                    <ListChecks size={17} aria-hidden="true" />生成分工
                  </button>
                  <button type="button" role="menuitem" tabIndex={-1} title="选择至少一位开发成员，按本轮分工自动执行并验收"
                    disabled={planToolDisabled || discussing || archivedRuleId !== ""}
                    onClick={() => runComposerTool(() => void submit(undefined, undefined, true, true))}>
                    <Lightning size={17} aria-hidden="true" />全自动托管
                  </button>
                  <button type="button" role="menuitem" tabIndex={-1} aria-controls={modelMember ? modelBarId : undefined}
                    disabled={!agents.length || !active}
                    onClick={() => runComposerTool(() => {
                      setMembersOpen(null);
                      setModelMemberId(modelMember ? undefined : selected[0] ?? agents[0]?.id);
                    })}><SlidersHorizontal size={17} aria-hidden="true" />模型与推理</button>
                </div>
                <span id={attachmentFormatsId} className="conversation-composer-attachment-tooltip" role="tooltip">图片、视频和文档</span>
              </div>}
            </div>
            <button ref={composerMemberTrigger} type="button" className="conversation-target" aria-expanded={membersOpen === "composer"}
              aria-label={`分工与托管 · 已选 ${selected.length} 位`} aria-controls={membersOpen === "composer" ? membersId : undefined}
              title="选择分工与托管成员。消息中有 @ 时，优先使用点名成员。"
              onClick={() => setMembersOpen(value => value === "composer" ? null : "composer")} disabled={!agents.length}>
              <User size={16} /><span><b className="conversation-target-prefix">分工与托管 · </b>已选 {selected.length} 位</span><CaretDown size={12} />
            </button>
          </div>
           <div className="conversation-composer-send-actions">
            {showChatRecipient && conversation && projectId ? <button type="button" className="conversation-send-scope-reset"
              title={`${sendScopeLabel} · 切回发给全体`} aria-label={`${sendScopeLabel}，切回发给全体`} disabled={busy || !!pending}
              onClick={() => {
                rememberChatRecipient(projectId, conversation.conversation.id);
                editor.current?.focus();
              }}>
              <span id={sendScopeId} className="conversation-send-scope">{sendScopeLabel}</span><X size={13} aria-hidden="true" />
            </button> : <span id={sendScopeId} className="conversation-send-scope">{sendScopeLabel}</span>}
           <button type="submit" className="conversation-send" aria-label={sendLabel} title={sendLabel} aria-describedby={sendScopeId}
             disabled={busy || pickingFiles || (mentionOpen && mentionMultiple) || !healthy || !active || !runtimeReady || !!runtimeError || !projectId || (!draft.trim() && !draftFiles.length) || !!pending || ["stopping", "unconfirmed"].includes(round?.status ?? "")}>
             <ArrowElbowDownLeft size={17} aria-hidden="true" />
           </button></div></div>
        {membersOpen === "composer" && <div id={membersId} ref={membersMenu} className="conversation-recipient-menu" aria-label="选择参与成员"><header><strong>分工与托管成员</strong>
            <div className="conversation-recipient-actions">
              <button type="button" disabled={busy || !availablePlanMembers.length} onClick={() => setSelected(allRecipientsSelected ? [] : availablePlanMembers)}>{allRecipientsSelected ? "取消全选" : "全选"}</button>
            </div>
          </header>{agents.filter(agent => !discussionOnlyMembers.has(agent.id)).map(agent => <label key={agent.id}><input type="checkbox" checked={selected.includes(agent.id)} disabled={!supported.includes(agent.runtimeKind) || !isRuntimeEnabled(agent.runtimeKind) || (manualMembers.has(agent.id) && !selected.includes(agent.id)) || busy} onChange={() => toggleMember(agent)} /><AgentAvatar avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative /><span>{agent.name}<small>{RUNTIME_LABELS[agent.runtimeKind]}{!isRuntimeEnabled(agent.runtimeKind) ? " · Runtime 已停用" : !supported.includes(agent.runtimeKind) ? " · 此 Runtime 尚未接入群聊" : manualMembers.has(agent.id) ? " · 用户接管中" : ""}</small></span></label>)}</div>}
      </ResizableConversationComposer>
    </footer></div>
    {workView === "tasks" && <div id={`${workViewId}-tasks-panel`} className="team-tasks-panel" role="tabpanel" aria-labelledby={`${workViewId}-tasks-tab`}><ConversationTaskView
      snapshot={snapshot} tasks={tasks} plans={conversation?.executionPlans ?? []}
      retainedPlans={conversation?.retainedExecutionPlans ?? []} renderRetainedDelivery={renderPlan}
      deliveryPlan={[...(conversation?.executionPlans ?? []), ...(conversation?.retainedExecutionPlans ?? [])].find(plan => plan.id === selectedDeliveryId)}
      discussion={latestDiscussion} selectedId={selectedTaskId} initialRecordTaskId={requestedView === "taskRecords" ? selectedTaskId : null}
      blocked={busy || !healthy || !active} onSelect={setSelectedTaskId} onExecution={action => void updateExecution(action)}
      onChat={returnToChat} onFullRecords={onOpenProjectSettings ? openProjectTasks : undefined}
      onOpenRun={onOpenSession ? (run, task) => void openManagedRunRecord(run, task) : undefined} onRefresh={refresh} /></div>}

    {creatingProject && <CollaborationProjectDialog onClose={() => setCreatingProject(false)} onCreate={async (input, gitSetup) => {
      const project = await invoke<MultiAgentProject>("ma_create_project", { input: { ...input, defaultBranch: "main" }, gitSetup });
      setProjects(items => [project, ...items.filter(item => item.id !== project.id)]); chooseProject(project.id);
    }} />}
    {historyOpen && active && projectId && conversation && createPortal(<ConversationHistoryDialog projectId={projectId} conversationId={conversation.conversation.id}
      blocked={newConversationBlocked} busy={busy} switchError={historySwitchError} onSwitch={id => void switchConversation(id)}
      onClose={() => { if (!mutation.current) setHistoryOpen(false); }} />, document.body)}
    {sessionAction && active && createPortal(<Modal className="conversation-clear-dialog conversation-session-dialog"
      ariaLabel={sessionAction.kind === "rename" ? "重命名会话" : "删除会话"}
      initialFocusRef={sessionAction.kind === "delete" ? cancelDelete : undefined}
      onClose={() => { if (!mutation.current) setSessionAction(null); }}>
      <h2>{sessionAction.kind === "rename" ? "重命名会话" : "删除会话？"}</h2>
      <form onSubmit={event => void confirmSessionAction(event)}>
        {sessionAction.kind === "rename"
          ? <label>会话名称<input value={sessionAction.title} maxLength={60} required disabled={busy}
            onFocus={event => event.currentTarget.select()}
            onChange={event => { const title = event.currentTarget.value; setSessionAction(current => current?.kind === "rename" ? { ...current, title } : current); }} /></label>
          : <p>确认删除「{sessionAction.label}」？删除后将进入空白会话。项目任务、审批、交付记录和 Agent 原会话会保留。</p>}
        {deleteChanged && <p role="alert">讨论记录已变化，请取消后重新确认。</p>}
        {sessionActionError && <p className="conversation-clear-error" role="alert">{sessionActionError}</p>}
        <div className="conversation-session-dialog-actions">
          <button ref={sessionAction.kind === "delete" ? cancelDelete : undefined} type="button" disabled={busy} onClick={() => setSessionAction(null)}>取消</button>
          {sessionAction.kind === "rename"
            ? <button type="submit" className="conversation-clear-confirm" disabled={busy || !sessionAction.title.trim()}>{busy ? "正在保存…" : "保存"}</button>
            : <button type="submit" className="conversation-delete-confirm" disabled={deleteBlocked || deleteChanged}>{busy ? "正在删除…" : "删除会话"}</button>}
        </div>
      </form>
    </Modal>, document.body)}
    {clearConfirmation && active && createPortal(<Modal className="conversation-clear-dialog" ariaLabel="清空讨论记录" initialFocusRef={cancelClear} onClose={() => { if (!mutation.current) setClearConfirmation(null); }}>
      <h2>清空讨论记录？</h2>
      <p>将清空「{clearConfirmation.projectName}」当前会话的讨论记录。项目、成员、任务、执行日志和 Agent 原会话保留。</p>
      {clearChanged ? <p role="alert">讨论记录已变化，请取消后重新确认。</p> : conversation?.rounds.some(conversationIsActive) ? <p role="alert">讨论正在进行，暂不能清空记录。</p> : null}
      {clearError && <p className="conversation-clear-error" role="alert">{clearError}</p>}
      <div><button ref={cancelClear} type="button" disabled={busy} onClick={() => setClearConfirmation(null)}>取消</button><button type="button" className="conversation-clear-confirm" disabled={clearBlocked || clearChanged} onClick={() => void confirmClear()}>{busy ? "正在清空…" : "清空记录"}</button></div>
    </Modal>, document.body)}
    </section>
    {delegation && projectId && conversationId && <ConversationDelegationDetails key={delegation.id}
      item={delegation} agents={agents} projectId={projectId} conversationId={conversationId} onClose={closeDelegation} />}
    {showExecDrawer && <GrokExecutionDrawer
      snapshot={snapshot}
      tasks={tasks}
      plans={conversation?.executionPlans ?? []}
      retainedPlans={conversation?.retainedExecutionPlans ?? []}
      selectedTaskId={selectedTaskId}
      blocked={busy || !healthy || !active}
      onSelectTask={setSelectedTaskId}
      onClose={() => setExecDrawerOpen(false)}
      onExecution={action => void updateExecution(action)}
      onOpenTasksView={() => { setSelectedDeliveryId(null); setWorkView("tasks"); }}
      onFullRecords={onOpenProjectSettings ? openProjectTasks : undefined}
      onOpenRun={onOpenSession ? (run, task) => void openManagedRunRecord(run, task) : undefined}
      onRefresh={refresh}
    />}
    </div>
  </div>;
}
