import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { ArrowClockwise, ArrowSquareOut, Broom, CaretDown, CaretRight, Check, CheckCircle, ChatCircleText, ClockCounterClockwise, DotsThree, FileArrowDown, GearSix, PaperPlaneTilt, Pause, Plus, Robot, Stop, User, WarningCircle } from "@phosphor-icons/react";
import { AgentAvatar } from "./AgentAvatar";
import { ConversationMentionHighlight } from "./ConversationMentionHighlight";
import { ResizableConversationComposer } from "./ResizableConversationComposer";
import { ConversationMarkdown } from "./ConversationMarkdown";
import { ConversationMessageCopy } from "./ConversationMessageCopy";
import { ConversationApprovalRequests } from "./ConversationApprovalRequests";
import { ConversationMemberActivity } from "./ConversationMemberActivity";
import { conversationMemberActivity } from "../conversationActivity";
import { ConversationPlanCard } from "./ConversationPlanCard";
import { ConversationHistoryDialog } from "./ConversationHistoryDialog";
import { ConversationDelegationDetails } from "./ConversationDelegationDetails";
import { ConversationTaskView } from "./ConversationTaskView";
import { AgentMentionMenu, type AgentMentionOption } from "./AgentMentionMenu";
import { CollaborationProjectDialog } from "./CollaborationProjectDialog";
import { UnverifiedRunNotice } from "./UnverifiedRunNotice";
import { Modal } from "./ui/Modal";
import { conversationIsActive, conversationMentionToken, conversationRecipients, conversationRequestIntent, conversationRequestMode, CONVERSATION_STATUS, hasConversationPlanGoal, latestConversationRound, orderedConversationMessages } from "../collaborationConversation";
import { RUNTIME_LABELS } from "../runtimeLogos";
import { visibleProjectTasks } from "../collaborationTasks";
import { hasMentionToken, mentionTextSegments, selectMentionTokens } from "../multiAgentMention";
import { useRuntimeAvailability } from "../runtimeAvailability";
import type { AgentProfile, ConversationDesignDelegation, ConversationExecutionPlan, ConversationMessage, ConversationMode, ConversationOriginalSession, ConversationSendInput, ConversationSnapshot, ConversationTaskRules, MultiAgentProject, MultiAgentSnapshot, SessionRecord } from "../types";
import "../styles/collaboration-conversation.css";
import "../styles/team-work-views.css";
import "../styles/conversation-member-activity.css";
import "../styles/conversation-policy.css";

interface Props {
  sessions?: SessionRecord[];
  active?: boolean;
  defaultCwd: string | null;
  focusedProjectId?: string | null;
  focusedProjectRequest?: number;
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
interface PendingSend { projectId: string; expectedConversationId?: string; clientMessageId: string; body: string; participantAgentIds: string[]; mode: ConversationMode; intent?: "chat" | "exchange"; taskRules?: ConversationTaskRules; nativeCapabilities?: boolean; interrupt: boolean }
interface PlanIssue { kind: "git" | "member"; message: string }
function sendPayload(item: PendingSend): ConversationSendInput {
  return { projectId: item.projectId, expectedConversationId: item.expectedConversationId, clientMessageId: item.clientMessageId,
    body: item.body, participantAgentIds: item.participantAgentIds, mode: item.mode, taskRules: item.taskRules,
    intent: item.intent ?? "chat", nativeCapabilities: item.nativeCapabilities ?? false,
    maxRounds: item.intent === "exchange" ? 3 : 1, maxDurationSeconds: 600 };
}
function storedPending(id: string, chat?: string): PendingSend | null {
  try {
    const item = JSON.parse(stored(pendingKey(id, chat)));
    return item?.projectId === id && typeof item.clientMessageId === "string" && typeof item.body === "string" && Array.isArray(item.participantAgentIds) && ["discussion", "execution_plan"].includes(item.mode) && typeof item.interrupt === "boolean" ? item : null;
  } catch { return null; }
}
function projectFromStorage(): string | null { return stored(PROJECT_KEY) || null; }

/** One persisted project conversation. It deliberately has no general-command or global-broadcast route. */
export function MultiAgentControl({ sessions = [], active = true, defaultCwd, focusedProjectId, focusedProjectRequest, onOpenSession, onOpenProjectSettings }: Props) {
  const { ready: runtimeReady, error: runtimeError, prefs: runtimePrefs, isRuntimeEnabled, refresh: refreshAvailability } = useRuntimeAvailability();
  const [projects, setProjects] = useState<MultiAgentProject[]>([]);
  const [projectId, setProjectId] = useState<string | null>(focusedProjectId ?? projectFromStorage);
  const [snapshot, setSnapshot] = useState<MultiAgentSnapshot | null>(null);
  const [conversation, setConversation] = useState<ConversationSnapshot | null>(null);
  const [activityNow, setActivityNow] = useState(Date.now);
  const [draft, setDraft] = useState("");
  const [archivedRuleId, setArchivedRuleId] = useState("");
  const [workView, setWorkView] = useState<"chat" | "tasks">("chat");
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const workViewId = useId();
  const [selected, setSelected] = useState<string[]>([]);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [healthy, setHealthy] = useState(false);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<PendingSend | null>(null);
  const [membersOpen, setMembersOpen] = useState<"header" | "composer" | null>(null);
  const [membersPosition, setMembersPosition] = useState({ left: 0, top: 0, width: MAX_MEMBER_MENU_WIDTH, maxHeight: 320 });
  const membersId = useId();
  const headerMemberTrigger = useRef<HTMLButtonElement>(null);
  const composerMemberTrigger = useRef<HTMLButtonElement>(null);
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
  const [exportNotice, setExportNotice] = useState<{ conversationId: string; filename: string } | null>(null);
  const exporting = useRef(false);
  const [openDelegation, setOpenDelegation] = useState<{ conversationId: string; itemId: string } | null>(null);
  const delegationTrigger = useRef<HTMLButtonElement | null>(null);
  const [historySwitchError, setHistorySwitchError] = useState("");
  const [clearConfirmation, setClearConfirmation] = useState<{ projectId: string; projectName: string; expectedConversationId: string; expectedMessageIds: string[] } | null>(null);
  const [clearError, setClearError] = useState("");
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
  const initializedMembers = useRef<string | null>(null);
  const editor = useRef<HTMLTextAreaElement>(null);
  const transcript = useRef<HTMLDivElement>(null);
  const followMessages = useRef(true);
  const mounted = useRef(true);
  const setOperationError = (value: string) => { operationError.current = value; setError(value); };
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; request.current++; generation.current++; }; }, []);

  const adoptConversation = useCallback((value: ConversationSnapshot) => {
    const { projectId, id } = value.conversation;
    if (currentConversation.current !== id) {
      if (!currentConversation.current) {
        for (const storageKey of [draftKey, pendingKey]) {
          const legacy = stored(storageKey(projectId));
          if (legacy && !stored(storageKey(projectId, id))) save(storageKey(projectId, id), legacy);
          if (legacy) save(storageKey(projectId), "");
        }
      }
      currentConversation.current = id;
      setDraft(stored(draftKey(projectId, id)));
      setPending(storedPending(projectId, id));
      setShowOlder(false); setArchivedRuleId(""); setMembersOpen(null); setMentionOpen(false); setMentionSelection([]);
      setPlanIssues([]);
      followMessages.current = true;
    }
    setConversation(value);
  }, []);

  const chooseProject = useCallback((id: string | null) => {
    if (currentProject.current === id) return;
    setComposingMention(false);
    request.current++; generation.current++; initializedMembers.current = null;
    currentProject.current = id; currentConversation.current = undefined; setProjectId(id); setSnapshot(null); setConversation(null); setHealthy(false); setLoading(false);
    operationError.current = ""; setError(""); setSelected([]); setMembersOpen(null); setShowOlder(false); setMentionOpen(false);
    setHistoryMenuOpen(false); setHistoryOpen(false); setClearConfirmation(null); setClearError("");
    setWorkView("chat"); setSelectedTaskId(null);
    setArchivedRuleId("");
    setPlanIssues([]);
    setDraft(id ? stored(draftKey(id)) : ""); setPending(id ? storedPending(id) : null); followMessages.current = true;
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
  useEffect(() => { if (!active) { setMembersOpen(null); setHistoryMenuOpen(false); setHistoryOpen(false); setClearConfirmation(null); setMentionOpen(false); } }, [active]);
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
      setMembersPosition({ left, width, maxHeight, top: flipped ? anchor.top - height - 8 : anchor.bottom + 8 });
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
  useEffect(() => {
    if (!conversation || !snapshot || !projectId || !runtimeReady || runtimeError) return;
    const eligible = agents.filter(agent => supported.includes(agent.runtimeKind) && isRuntimeEnabled(agent.runtimeKind)).map(agent => agent.id);
    if (initializedMembers.current !== projectId) { initializedMembers.current = projectId; setSelected(eligible.filter(id => !manualMembers.has(id))); }
    else setSelected(ids => ids.filter(id => eligible.includes(id)));
  }, [agents, conversation, snapshot, projectId, runtimeReady, runtimeError, runtimePrefs, isRuntimeEnabled, manualMembers]);
  const round = latestConversationRound(conversation?.rounds ?? []);
  const latestDiscussion = latestConversationRound(conversation?.rounds.filter(item => item.mode === "discussion") ?? []);
  const planningRounds = new Map(conversation?.rounds.filter(item => item.mode === "execution_plan").map(item => [item.userMessageId, item]));
  const mentionTokens = useMemo(() => agents.filter(agent => supported.includes(agent.runtimeKind) && isRuntimeEnabled(agent.runtimeKind))
    .map(agent => conversationMentionToken(agent, agents)), [agents, supported, isRuntimeEnabled]);
  const canGeneratePlan = hasConversationPlanGoal(draft, mentionTokens);
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
  const messages = orderedConversationMessages(conversation?.messages ?? [], projectId ?? "");
  const newConversationBlocked = busy || !!pending || !healthy || !active || !conversation || !!conversation.rounds.some(conversationIsActive);
  const clearBlocked = newConversationBlocked || !messages.length;
  const exportBlocked = !active || !conversation || conversation.conversation.projectId !== projectId || !messages.some(message => message.body.trim());
  const clearChanged = !!clearConfirmation && (conversation?.conversation.id !== clearConfirmation.expectedConversationId || messages.length !== clearConfirmation.expectedMessageIds.length || messages.some(message => !clearConfirmation.expectedMessageIds.includes(message.id)));
  useEffect(() => {
    if (!pending || !conversation || conversation.conversation.projectId !== pending.projectId || projectId !== pending.projectId) return;
    if (!conversation.messages.some(message => message.clientMessageId === pending.clientMessageId)) return;
    const conversationId = conversation.conversation.id;
    save(pendingKey(pending.projectId, conversationId), "");
    if (stored(draftKey(pending.projectId, conversationId)) === pending.body) save(draftKey(pending.projectId, conversationId), "");
    setDraft(value => value === pending.body ? "" : value);
    setPending(null); setArchivedRuleId(""); setOperationError("");
  }, [conversation, pending, projectId]);
  const visibleMessages = showOlder ? messages : messages.slice(-100);
  const tasks = visibleProjectTasks(snapshot?.tasks ?? []);
  const openProjectTasks = () => { if (snapshot && projectId && active) { setMembersOpen(null); onOpenProjectSettings?.(projectId, "tasks"); } };
  const waitingApproval = round?.status === "waiting_approval" ? snapshot?.approvals.find(approval => approval.projectId === projectId && approval.status === "pending" && messages.some(message => message.roundId === round.id && message.runId === approval.runId && message.senderType === "agent")) : undefined;
  const activeAgent = discussing ? agents.find(agent => conversation?.messages.some(message => message.roundId === round?.id && message.senderType === "agent" && message.agentId === agent.id && (waitingApproval ? message.runId === waitingApproval.runId : message.status === "running"))) : undefined;
  useEffect(() => {
    if (followMessages.current && transcript.current) transcript.current.scrollTop = transcript.current.scrollHeight;
  }, [messages.length, messages[messages.length - 1]?.updatedAt, projectId, conversation?.conversation.id]);
  const changeDraft = (value: string) => { setDraft(value); setPlanIssues([]); if (projectId) save(draftKey(projectId, currentConversation.current), value); };
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
      save(pendingKey(item.projectId, conversationId), "");
      if (stored(draftKey(item.projectId, conversationId)) === item.body) save(draftKey(item.projectId, conversationId), "");
      if (mounted.current && currentProject.current === item.projectId && currentConversation.current === conversationId) {
        setPending(null); setArchivedRuleId(""); setDraft(value => value === item.body ? "" : value);
        setMentionOpen(false); void refresh();
      }
    } catch { /* Retain the same identity and payload for a safe status check/retry. */ }
  };
  const submit = async (event?: FormEvent, bodyOverride?: string, requestPlan = false) => {
    event?.preventDefault();
    if (mentionOpen && mentionMultiple) return;
    if (mutation.current || !projectId || !conversation || !healthy || !active || !runtimeReady || runtimeError || pending || ["stopping", "unconfirmed"].includes(round?.status ?? "")) return;
    const body = (bodyOverride ?? draft).trim();
    const conversationId = conversation.conversation.id;
    const stillOpen = () => mounted.current && currentProject.current === projectId && currentConversation.current === conversationId;
    if (!body || Array.from(body).length > 32000) { setOperationError("请输入 1–32000 字符的消息。"); return; }
    try {
      const ids = conversationRecipients(body, selected, agents, projectId);
      if (ids.some(id => !isRuntimeEnabled(agents.find(agent => agent.id === id)!.runtimeKind))) throw Error("目标成员的 Runtime 已停用，请在协作设置中启用后重试。");
      if (ids.some(id => !supported.includes(agents.find(agent => agent.id === id)!.runtimeKind))) throw Error("所选成员此 Runtime 尚未接入群聊，请调整成员或在原会话中操作。");
      const manual = ids.filter(id => manualMembers.has(id)).map(id => agents.find(agent => agent.id === id)!.name);
      if (!discussing && manual.length) throw Error(`${manual.join("、")} 正由用户接管，请取消选择这些成员，或在成员设置中交还自动控制后重试。`);
      const rule = archivedRuleId ? conversation?.archivedTaskRules?.find(value => value.id === archivedRuleId) : undefined;
      if (archivedRuleId && (!rule || (rule.agentId && !ids.includes(rule.agentId)))) throw Error("本次规则对应的成员未选中，或规则已不可用，请重新选择。");
      const mode = conversationRequestMode(body, requestPlan ? "execution_plan" : "discussion", mentionTokens);
      if (mode === "execution_plan" && !hasConversationPlanGoal(body, mentionTokens)) throw Error("请填写要完成的目标，再生成分工。");
      const item: PendingSend = { projectId, expectedConversationId: conversation.conversation.id, clientMessageId: crypto.randomUUID(), body, participantAgentIds: ids, mode, intent: conversationRequestIntent(body), nativeCapabilities: true, taskRules: rule ? { tools: "research", archivedRuleId: rule.id } : undefined, interrupt: discussing };
      setPlanIssues([]);
      if (mode === "execution_plan") {
        mutation.current = true;
        setBusy(true);
        setOperationError("");
        try {
          const issues = await invoke<PlanIssue[]>("ma_conversation_plan_preflight", { input: sendPayload(item) });
          if (!stillOpen()) return;
          if (issues.length) { setPlanIssues(issues); return; }
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
      const result = await mutationCall("ma_conversation_new", { input: { projectId: id, expectedConversationId: conversation.conversation.id, expectedMessageIds: messages.map(message => message.id) } });
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
    if (newConversationBlocked || !projectId || !conversation || conversationId === conversation.conversation.id) return;
    const id = projectId, epoch = generation.current;
    setHistorySwitchError("");
    try {
      const result = await mutationCall("ma_conversation_switch", { input: { projectId: id, expectedConversationId: conversation.conversation.id, conversationId } });
      if (!result || currentProject.current !== id || generation.current !== epoch) return;
      setHistoryOpen(false); setWorkView("chat");
    } catch (reason) {
      if (currentProject.current === id && generation.current === epoch) setHistorySwitchError(String(reason));
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
    const id = projectId, epoch = generation.current;
    // Persist a stable operation identity before the request; uncertainty must not duplicate engineering tasks.
    const key = `crewvela-conversation-plan:${projectId}:${plan.id}`;
    const clientRequestId = stored(key) || crypto.randomUUID(); save(key, clientRequestId);
    try {
      const result = await mutationCall("ma_conversation_execute_plan", { input: { projectId, planId: plan.id, clientRequestId } });
      if (result?.executionPlans.some(item => item.id === plan.id && item.status === "applied") && currentProject.current === id && generation.current === epoch) { setSelectedTaskId(result.executionPlans.find(item => item.id === plan.id)?.taskIds[0] ?? null); setWorkView("tasks"); void refresh(); }
    } catch { /* Backend deduplicates the retained identity. */ }
  };
  const editPlan = (plan: ConversationExecutionPlan) => {
    const prefix = `请调整分工：${plan.summary}\n`;
    changeDraft(draft.startsWith(prefix) ? draft : `${prefix}${draft || "调整要求："}`);
    setWorkView("chat");
    requestAnimationFrame(() => { editor.current?.focus(); editor.current?.setSelectionRange(editor.current.value.length, editor.current.value.length); });
  };
  const returnToChat = (messageId?: string) => {
    setWorkView("chat"); followMessages.current = !messageId;
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
    const id = session?.id ?? binding?.appSessionId ?? null;
    if ((!id && !original) || !onOpenSession) { setOperationError("原会话记录暂不可用。"); return; }
    setOperationError("");
    try { await onOpenSession(id, original); } catch (reason) { setOperationError(`打开原会话失败：${String(reason)}`); }
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
  const toggleMember = (agent: AgentProfile) => setSelected(ids => ids.includes(agent.id) ? ids.filter(id => id !== agent.id) : [...ids, agent.id]);
  const renderPlan = (plan: ConversationExecutionPlan) => <ConversationPlanCard key={plan.id} plan={plan} agents={snapshot?.agents ?? []} blocked={busy || !healthy || discussing || !active} onConfirm={() => void applyPlan(plan)} onAdjust={() => editPlan(plan)} onTasks={() => setWorkView("tasks")} />;

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

  return <div className={`conversation-delegation-layout${delegation ? " has-delegation" : ""}`}>
    <section className="collaboration-conversation team-work-views" aria-label="团队协作会话">
    <header className="conversation-header">
      <div className="conversation-heading"><span className="conversation-project-select"><select aria-label="协作项目" title={snapshot?.project.rootPath ? `${snapshot.project.name}\n项目目录：${snapshot.project.rootPath}` : projects.find(project => project.id === projectId)?.name} value={projectId ?? ""} onChange={event => chooseProject(event.target.value || null)}><option value="" disabled>选择协作项目</option>{projects.map(project => <option key={project.id} value={project.id}>{project.name}</option>)}</select><CaretDown size={15} aria-hidden="true" /></span>
        <button type="button" className="conversation-icon" aria-label="创建协作项目" onClick={() => setCreatingProject(true)}><Plus size={18} /></button>
        {onOpenProjectSettings && <button type="button" className="conversation-icon" aria-label="管理项目与成员" onClick={() => onOpenProjectSettings(projectId ?? undefined)}><GearSix size={18} /></button>}
      </div>
      <div className="conversation-context-row"><div className="conversation-context"><button type="button" className="conversation-text-button conversation-task-link" disabled={!snapshot || !onOpenProjectSettings} onClick={openProjectTasks}>查看任务 · {tasks.length}<ArrowSquareOut size={14} /></button></div>
        <div className="conversation-members" aria-label="当前团队成员"><button ref={headerMemberTrigger} type="button" className="conversation-members-trigger" aria-label={`选择参与成员，已选 ${selected.length} 位，共 ${agents.length} 位`} aria-expanded={membersOpen === "header"} aria-controls={membersOpen === "header" ? membersId : undefined} onClick={() => setMembersOpen(value => value === "header" ? null : "header")} disabled={!snapshot}>
          <span className="conversation-member-preview" aria-hidden="true">{agents.slice(0, delegation ? 4 : 3).map(agent => <span key={agent.id} data-selected={selected.includes(agent.id)}><AgentAvatar avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative /></span>)}</span><span>成员 {agents.length}</span><CaretDown size={14} /></button></div>
      </div>
      <div className="conversation-tabs-row"><div className="conversation-modes" role="tablist" aria-label="协作视图">{([['chat', '团队聊天'], ['tasks', '任务执行']] as const).map(([value, label]) => <button type="button" role="tab" id={`${workViewId}-${value}-tab`} aria-label={label} aria-controls={`${workViewId}-${value}-panel`} key={value} aria-selected={workView === value} tabIndex={workView === value ? 0 : -1} onKeyDown={event => {
        if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
        event.preventDefault(); const next = event.key === "Home" ? "chat" : event.key === "End" ? "tasks" : workView === "chat" ? "tasks" : "chat";
        setWorkView(next); setMembersOpen(null); setHistoryMenuOpen(false);
        requestAnimationFrame(() => document.getElementById(`${workViewId}-${next}-tab`)?.focus());
      }} onClick={() => { setWorkView(value); setMembersOpen(null); setHistoryMenuOpen(false); }}>{label}{value === "tasks" && <span className="team-task-count">{tasks.length}</span>}</button>)}</div>
        <div className="conversation-history-actions"><button ref={historyTrigger} type="button" className="conversation-icon" aria-label="讨论记录操作" aria-haspopup="menu" aria-expanded={historyMenuOpen} aria-controls={historyMenuOpen ? historyMenuId : undefined} disabled={!projectId || busy || !active} onClick={() => { setMembersOpen(null); setHistoryMenuOpen(value => !value); }}><DotsThree size={21} /></button>
          {historyMenuOpen && <div ref={historyMenu} id={historyMenuId} className="conversation-history-menu" role="menu" aria-label="讨论记录操作">
          <button type="button" role="menuitem" disabled={newConversationBlocked} title={conversation?.rounds.some(conversationIsActive) ? "团队正在回复，结束后可新建会话" : pending ? "请先核对未确认的发送" : "开始新的团队会话"} onClick={() => void startNewConversation()}><Plus size={17} />新建会话</button>
          <button type="button" role="menuitem" disabled={!conversation} onClick={() => { historyTrigger.current?.focus(); setHistoryMenuOpen(false); setHistorySwitchError(""); setHistoryOpen(true); }}><ClockCounterClockwise size={17} />历史会话</button>
          <button type="button" role="menuitem" disabled={exportBlocked || exporting.current} title={exportBlocked ? "暂无可导出的消息" : "将当前团队会话保存为 Markdown"} onClick={() => void exportConversation()}><FileArrowDown size={17} />导出会话</button>
          <button type="button" role="menuitem" disabled={clearBlocked} title={conversation?.rounds.some(conversationIsActive) ? "讨论进行中，结束后可清空" : pending ? "请先核对未确认的发送" : !messages.length ? "暂无讨论记录" : undefined} onClick={() => {
            if (clearBlocked || !projectId || !conversation) return;
            historyTrigger.current?.focus(); setHistoryMenuOpen(false); setClearError("");
            setClearConfirmation({ projectId, projectName: projects.find(project => project.id === projectId)?.name ?? "当前项目", expectedConversationId: conversation.conversation.id, expectedMessageIds: messages.map(message => message.id) });
          }}><Broom size={17} />清空讨论记录</button>{discussing && <><button type="button" role="menuitem" disabled={busy || round?.status === "stopping"} onClick={()=>void control("pause")}><Pause size={17}/>暂停讨论</button><button type="button" role="menuitem" disabled={busy || round?.status === "stopping"} onClick={()=>void control("cancel")}><Stop size={17}/>结束本轮</button></>}</div>}
        </div>
      </div>
    </header>
    {membersOpen === "header" && createPortal(<div id={membersId} ref={membersMenu} className="conversation-member-menu" role="group" aria-label="选择参与成员" style={membersPosition}><header>参与成员</header>{agents.map(agent => {
      const eligible = supported.includes(agent.runtimeKind) && isRuntimeEnabled(agent.runtimeKind) && !manualMembers.has(agent.id);
      const reason = !runtimeReady ? "正在读取 Runtime 状态" : !isRuntimeEnabled(agent.runtimeKind) ? "Runtime 已停用" : !supported.includes(agent.runtimeKind) ? "此 Runtime 尚未接入群聊" : manualMembers.has(agent.id) ? "用户接管中" : "";
      return <label key={agent.id} className={eligible ? "" : "unavailable"} title={`${agent.name} · ${RUNTIME_LABELS[agent.runtimeKind]}${reason ? ` · ${reason}` : ""}`}><input type="checkbox" aria-label={agent.name} checked={selected.includes(agent.id)} disabled={(!eligible && !selected.includes(agent.id)) || busy} onChange={() => toggleMember(agent)} /><AgentAvatar avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative /><span className="conversation-member-name"><strong>{agent.name}</strong><small>{ROLE[agent.role] ?? agent.role}{reason && ` · ${reason}`}{activeAgent?.id === agent.id && " · 正在回复"}</small></span><Check size={18} className="conversation-member-check" aria-hidden="true" /></label>;
    })}{!agents.length && <p>还没有团队成员。{onOpenProjectSettings && <button type="button" className="conversation-text-button" onClick={() => { setMembersOpen(null); onOpenProjectSettings(projectId ?? undefined); }}>添加成员</button>}</p>}</div>, document.body)}

    {(error || runtimeError) && <div className="conversation-error" role="alert"><WarningCircle size={17} /><span>{error || `读取 Runtime 启用状态失败：${runtimeError}`}</span><button type="button" disabled={busy || loading} onClick={() => { void loadProjects().catch(reason => setError(String(reason))); void refreshAvailability(); void refresh(); }}>重新读取</button></div>}
    {exportNotice && exportNotice.conversationId === conversation?.conversation.id && <div className="conversation-progress conversation-export-status" role="status">已导出：{exportNotice.filename}</div>}
    <div id={`${workViewId}-chat-panel`} className="team-chat-panel" role="tabpanel" aria-labelledby={`${workViewId}-chat-tab`} hidden={workView !== "chat"}><div className="conversation-transcript" role="log" aria-label="团队对话" aria-live="polite" ref={transcript} onScroll={event => { const node = event.currentTarget; followMessages.current = node.scrollHeight - node.scrollTop - node.clientHeight < 90; }}>
      {!projectId ? <div className="conversation-empty"><ChatCircleText size={30} /><h2>让 Agent 组成团队</h2><p>创建一个协作项目，选择成员，在这里讨论或分工开发。</p><button type="button" className="conversation-primary" onClick={() => setCreatingProject(true)}>创建协作项目</button></div>
        : !conversation ? <p className="conversation-empty" role="status">{loading ? "正在读取协作会话…" : "协作会话尚未读取，请重试。"}</p>
        : null}
      {messages.length > 100 && !showOlder && <button type="button" className="conversation-text-button conversation-older" onClick={() => setShowOlder(true)}>查看更早的 {messages.length - 100} 条消息</button>}
      {visibleMessages.map((message, index) => {
        const agent = agents.find(item => item.id === message.agentId) ?? snapshot?.agents.find(item => item.id === message.agentId);
        const plan = conversation?.executionPlans.find(item => item.messageId === message.id);
        const planning = message.senderType === "user" ? planningRounds.get(message.id) : undefined;
        const planned = planning && conversation?.executionPlans.find(item => item.roundId === planning.id);
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
              <header><strong>{agent?.name ?? "历史成员"}</strong><time dateTime={new Date(stamp).toISOString()}>{new Date(stamp).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })}</time></header>
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
        return <article className={`conversation-message ${message.senderType} ${message.status}${continuation ? " plan-continuation" : ""}`} key={message.id} data-message-id={message.id}>
          {agent ? <AgentAvatar className="conversation-avatar" avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative /> : <span className="conversation-avatar">{message.senderType === "user" ? <User size={21} /> : <Robot size={21} />}</span>}
          <div className="conversation-message-content"><header><strong>{message.senderType === "user" ? "我" : agent?.name ?? "历史成员"}</strong></header>
            {!plan && (message.senderType === "agent" ? <ConversationMemberActivity agentName={agent?.name ?? "历史成员"} activity={conversationMemberActivity(message,snapshot?.runs.find(run=>run.id===message.runId),activityEvents,snapshot?.approvals ?? [],activityNow)} now={activityNow} onOpen={message.runId ? openOriginal : undefined} onStop={message.roundId === round?.id && discussing && ["running","waiting_approval"].includes(message.status) ? ()=>void control("cancel") : undefined} stopDisabled={busy || !active || !healthy || round?.status === "stopping"}>{message.body ? <ConversationMarkdown body={message.body}/> : null}</ConversationMemberActivity> : message.body ? <ConversationMarkdown body={message.body}/> : null)}
            {message.queuedInput && <div className="conversation-progress"><span>{message.status === "queued" ? "等待前一轮确认停止后发送" : message.status === "cancelled" ? "这条消息尚未发送，已取消" : "这条消息尚未发送，等待你确认继续"}</span>{message.status === "needs_decision" && <button type="button" className="conversation-text-button" disabled={busy || !!pending || !healthy || !active || discussing} onClick={() => void mutationCall("ma_conversation_send", { input: message.queuedInput }).then(() => refresh()).catch(() => {})}>继续发送</button>}</div>}
            {plan && renderPlan(plan)}
            {planning && <div className="conversation-progress">
              <span>{planned?.status === "applied" ? "分工已确认，可在「任务执行」查看进度"
                : planned?.status === "pending" ? "方案已生成，待确认后执行"
                : planned?.status === "superseded" ? "该分工已被更新"
                : ["completed", "needs_decision", "failed"].includes(planning.status) ? "分工未生成，开发尚未开始"
                : ["queued", "running"].includes(planning.status) ? "正在拟定分工，开发尚未开始" : `生成分工 · ${CONVERSATION_STATUS[planning.status]}`}</span>
              {planning.stopReason && <span>{planning.stopReason}</span>}
              {["failed", "needs_decision"].includes(planning.status) && <button type="button" className="conversation-text-button" onClick={() => {
                if (!draft.trim()) changeDraft(message.body);
                editor.current?.focus();
              }}>编辑分工请求</button>}
            </div>}
            {message.senderType === "agent" && message.runId && <ConversationApprovalRequests
              approvals={(snapshot?.approvals ?? []).filter(approval => approval.projectId === message.projectId && approval.runId === message.runId && approval.status === "pending" && approval.approvalType === "runtime_permission" && (!message.taskId || approval.taskId === message.taskId))}
              disabled={busy || !active || !healthy || message.roundId !== round?.id || round?.status === "stopping"}
              onChanged={() => void refresh()}
              onOpen={openOriginal}
              onStop={message.roundId === round?.id && discussing ? () => void control("cancel") : undefined}
            />}
            {!plan && !!message.body.trim() && <ConversationMessageCopy body={message.body}/>}
          </div>
        </article>;
      })}
      {conversation?.executionPlans.filter(plan => !messages.some(message => message.id === plan.messageId)).map(renderPlan)}
      {snapshot?.runs.filter(run => run.sessionLocator?.stopUnverified === true).map(run => <UnverifiedRunNotice key={run.id} run={run} taskTitle={snapshot.tasks.find(task => task.id === run.taskId)?.title} onConfirmed={refresh} />)}
    </div>

    <footer className="conversation-footer">
      {planIssues.length > 0 && <div className="conversation-plan-issues" role="alert">
        <strong>暂无法生成分工，开发尚未开始</strong>
        <ul>{planIssues.map(issue => <li key={issue.message}>{issue.message}</li>)}</ul>
        {planIssues.some(issue => issue.kind === "member") && <>
          {planHasMentions && <p>正文里指定了 @成员，请把缺少的成员也加入 @ 列表。</p>}
          <button type="button" className="conversation-text-button" onClick={() => {
            if (planHasMentions) editor.current?.focus();
            else setMembersOpen("composer");
          }}>{planHasMentions ? "编辑 @成员" : "调整参与成员"}</button>
        </>}
      </div>}
      {round && ((!["running", "queued", "completed"].includes(round.status)) || round.stopReason) && <div className="conversation-progress" role="status"><span className={`conversation-status-dot ${round.status}`} /><span>{activeAgent ? `${activeAgent.name} · ` : ""}{round.status === "completed" ? "已回复" : CONVERSATION_STATUS[round.status]}{round.stopReason ? ` · ${round.stopReason}` : ""}</span>{discussing && <button className="conversation-text-button" type="button" disabled={busy || round.status === "stopping"} onClick={() => void control("pause")}><Pause size={13} />{round.status === "unconfirmed" ? "重新核对停止" : "暂停讨论"}</button>}{discussing && <button type="button" className="conversation-text-button" aria-label="结束本轮讨论" disabled={busy || round.status === "stopping"} onClick={() => void control("cancel")}><Stop size={13} />结束本轮</button>}{["paused", "cancelled"].includes(round.status) && <button className="conversation-text-button" type="button" disabled={busy || !!pending || !healthy} onClick={() => editor.current?.focus()}>继续输入</button>}</div>}
      {pending && <div className="conversation-send-pending" role="status"><span>{busy ? "正在发送…" : "本次发送结果未确认，重新核对会使用同一消息编号。"}</span>{!busy && <button type="button" onClick={() => void deliver(pending)}>核对并重试</button>}</div>}
      <ResizableConversationComposer onSubmit={event => void submit(event)}>
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
        <div className="conversation-composer-actions"><button ref={composerMemberTrigger} type="button" className="conversation-target" aria-expanded={membersOpen === "composer"} aria-controls={membersOpen === "composer" ? membersId : undefined} onClick={() => setMembersOpen(value => value === "composer" ? null : "composer")} disabled={!agents.length}><User size={16} /><span>{`已选 ${selected.length} 位成员`}</span><CaretDown size={12} /></button>{!!conversation?.archivedTaskRules?.length && <label className="conversation-task-rule">本次规则<select aria-label="本次规则" title={conversation.archivedTaskRules.find(rule => rule.id === archivedRuleId)?.instructions ?? "仅在本条请求生效，不自动延续到下一条"} value={archivedRuleId} disabled={busy || !!pending} onChange={event => setArchivedRuleId(event.target.value)}><option value="">无附加规则</option>{conversation.archivedTaskRules.map(rule => <option key={rule.id} value={rule.id}>{rule.agentId ? agents.find(agent => agent.id === rule.agentId)?.name ?? "历史成员" : "项目"} · {rule.label} · {new Date(rule.createdAt).toLocaleString("zh-CN")}</option>)}</select></label>}<button type="button" className="conversation-text-button team-generate-plan" title={canGeneratePlan ? undefined : "请填写要完成的目标，再生成分工"} disabled={busy || (mentionOpen && mentionMultiple) || !healthy || !active || !runtimeReady || !!runtimeError || !canGeneratePlan || !!pending || ["stopping", "unconfirmed"].includes(round?.status ?? "")} onClick={() => void submit(undefined, undefined, true)}>生成分工</button><button type="submit" className="conversation-send" aria-label={discussing ? "发送插话" : "发送消息"} disabled={busy || (mentionOpen && mentionMultiple) || !healthy || !active || !runtimeReady || !!runtimeError || !projectId || !draft.trim() || !!pending || ["stopping", "unconfirmed"].includes(round?.status ?? "")}><PaperPlaneTilt size={21} weight="fill" /></button></div>
        {membersOpen === "composer" && <div id={membersId} ref={membersMenu} className="conversation-recipient-menu" aria-label="选择参与成员"><header><strong>参与成员</strong><button type="button" onClick={() => { const eligible = agents.filter(agent => supported.includes(agent.runtimeKind) && isRuntimeEnabled(agent.runtimeKind) && !manualMembers.has(agent.id)).map(agent => agent.id); setSelected(selected.length === eligible.length ? [] : eligible); }}>全选 / 取消全选</button></header>{agents.map(agent => <label key={agent.id}><input type="checkbox" checked={selected.includes(agent.id)} disabled={!supported.includes(agent.runtimeKind) || !isRuntimeEnabled(agent.runtimeKind) || (manualMembers.has(agent.id) && !selected.includes(agent.id)) || busy} onChange={() => toggleMember(agent)} /><AgentAvatar avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative /><span>{agent.name}<small>{RUNTIME_LABELS[agent.runtimeKind]}{!isRuntimeEnabled(agent.runtimeKind) ? " · Runtime 已停用" : !supported.includes(agent.runtimeKind) ? " · 此 Runtime 尚未接入群聊" : manualMembers.has(agent.id) ? " · 用户接管中" : ""}</small></span></label>)}</div>}
      </ResizableConversationComposer>
    </footer></div>
    {workView === "tasks" && <div id={`${workViewId}-tasks-panel`} className="team-tasks-panel" role="tabpanel" aria-labelledby={`${workViewId}-tasks-tab`}><ConversationTaskView snapshot={snapshot} tasks={tasks} plans={conversation?.executionPlans ?? []} discussion={latestDiscussion} selectedId={selectedTaskId} blocked={busy || !healthy || !active} onSelect={setSelectedTaskId} onExecution={action => void updateExecution(action)} onChat={returnToChat} onFullRecords={onOpenProjectSettings ? openProjectTasks : undefined} onRefresh={refresh} /></div>}

    {creatingProject && <CollaborationProjectDialog onClose={() => setCreatingProject(false)} onCreate={async (input, gitSetup) => {
      const project = await invoke<MultiAgentProject>("ma_create_project", { input: { ...input, defaultBranch: "main" }, gitSetup });
      setProjects(items => [project, ...items.filter(item => item.id !== project.id)]); chooseProject(project.id);
    }} />}
    {historyOpen && active && projectId && conversation && createPortal(<ConversationHistoryDialog projectId={projectId} conversationId={conversation.conversation.id}
      blocked={newConversationBlocked} busy={busy} switchError={historySwitchError} onSwitch={id => void switchConversation(id)}
      onClose={() => { if (!mutation.current) setHistoryOpen(false); }} />, document.body)}
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
  </div>;
}
