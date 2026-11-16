import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { ArrowClockwise, ArrowSquareOut, CaretDown, Check, CheckCircle, ChatCircleText, GearSix, ListChecks, PaperPlaneTilt, Pause, Plus, Robot, Stop, User, WarningCircle } from "@phosphor-icons/react";
import { AgentAvatar } from "./AgentAvatar";
import { AgentMentionMenu, type AgentMentionOption } from "./AgentMentionMenu";
import { CollaborationProjectDialog } from "./CollaborationProjectDialog";
import { UnverifiedRunNotice } from "./UnverifiedRunNotice";
import { conversationIsActive, conversationMentionToken, conversationRecipients, CONVERSATION_STATUS, latestConversationRound, orderedConversationMessages, TASK_STATUS } from "../collaborationConversation";
import { RUNTIME_LABELS } from "../runtimeLogos";
import { visibleProjectTasks } from "../collaborationTasks";
import { hasMentionToken, selectMentionTokens } from "../multiAgentMention";
import { useRuntimeAvailability } from "../runtimeAvailability";
import type { AgentProfile, ConversationExecutionPlan, ConversationMode, ConversationSendInput, ConversationSnapshot, MultiAgentProject, MultiAgentSnapshot, SessionRecord } from "../types";
import "../styles/collaboration-conversation.css";

interface Props {
  sessions?: SessionRecord[];
  active?: boolean;
  defaultCwd: string | null;
  focusedProjectId?: string | null;
  focusedProjectRequest?: number;
  onOpenWorkspace?: (projectId: string) => void;
  onOpenSession?: (sessionId: string) => void | Promise<void>;
  onOpenProjectSettings?: (projectId?: string, target?: "tasks") => void;
}
const PROJECT_KEY = "crewvela-multi-agent-project";
const ROLE: Record<string, string> = { orchestrator: "方案", worker: "开发", reviewer: "审查", integrator: "集成" };
const draftKey = (id: string) => `crewvela-conversation-draft:${id}`;
const pendingKey = (id: string) => `crewvela-conversation-pending:${id}`;
function stored(key: string): string { try { return localStorage.getItem(key) ?? ""; } catch { return ""; } }
function save(key: string, value: string) { try { if (value) localStorage.setItem(key, value); else localStorage.removeItem(key); } catch { /* The current draft remains available in the editor. */ } }
interface PendingSend { projectId: string; clientMessageId: string; body: string; participantAgentIds: string[]; mode: ConversationMode; interrupt: boolean }
function storedPending(id: string): PendingSend | null {
  try {
    const item = JSON.parse(stored(pendingKey(id)));
    return item?.projectId === id && typeof item.clientMessageId === "string" && typeof item.body === "string" && Array.isArray(item.participantAgentIds) && ["discussion", "execution_plan"].includes(item.mode) && typeof item.interrupt === "boolean" ? item : null;
  } catch { return null; }
}
function projectFromStorage(): string | null { return stored(PROJECT_KEY) || null; }

/** One persisted project conversation. It deliberately has no general-command or global-broadcast route. */
export function MultiAgentControl({ sessions = [], active = true, defaultCwd, focusedProjectId, focusedProjectRequest, onOpenWorkspace, onOpenSession, onOpenProjectSettings }: Props) {
  const { ready: runtimeReady, error: runtimeError, prefs: runtimePrefs, isRuntimeEnabled, refresh: refreshAvailability } = useRuntimeAvailability();
  const [projects, setProjects] = useState<MultiAgentProject[]>([]);
  const [projectId, setProjectId] = useState<string | null>(focusedProjectId ?? projectFromStorage);
  const [snapshot, setSnapshot] = useState<MultiAgentSnapshot | null>(null);
  const [conversation, setConversation] = useState<ConversationSnapshot | null>(null);
  const [draft, setDraft] = useState("");
  const [mode, setMode] = useState<ConversationMode>("discussion");
  const [selected, setSelected] = useState<string[]>([]);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [healthy, setHealthy] = useState(false);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<PendingSend | null>(null);
  const [membersOpen, setMembersOpen] = useState<"header" | "composer" | null>(null);
  const [membersPosition, setMembersPosition] = useState({ left: 0, top: 0, width: 320, maxHeight: 320 });
  const membersId = useId();
  const headerMemberTrigger = useRef<HTMLButtonElement>(null);
  const composerMemberTrigger = useRef<HTMLButtonElement>(null);
  const membersMenu = useRef<HTMLDivElement>(null);
  const [creatingProject, setCreatingProject] = useState(false);
  const [mentionCursor, setMentionCursor] = useState(0);
  const [mentionOpen, setMentionOpen] = useState(false);
  const [mentionIndex, setMentionIndex] = useState(0);
  const [showOlder, setShowOlder] = useState(false);
  const request = useRef(0);
  const generation = useRef(0);
  const mutation = useRef(false);
  const operationError = useRef("");
  const currentProject = useRef(projectId); currentProject.current = projectId;
  const initializedMembers = useRef<string | null>(null);
  const editor = useRef<HTMLTextAreaElement>(null);
  const transcript = useRef<HTMLDivElement>(null);
  const followMessages = useRef(true);
  const mounted = useRef(true);
  const setOperationError = (value: string) => { operationError.current = value; setError(value); };
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; request.current++; generation.current++; }; }, []);

  const chooseProject = useCallback((id: string | null) => {
    request.current++; generation.current++; initializedMembers.current = null;
    currentProject.current = id; setProjectId(id); setSnapshot(null); setConversation(null); setHealthy(false); setLoading(false);
    operationError.current = ""; setError(""); setSelected([]); setMembersOpen(null); setShowOlder(false); setMentionOpen(false);
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
  useEffect(() => { if (focusedProjectId) chooseProject(focusedProjectId); }, [focusedProjectId, focusedProjectRequest, chooseProject]);
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
    if (chat.status === "fulfilled" && chat.value.conversation.projectId === id) setConversation(chat.value);
    const failures = [project, chat].filter((result): result is PromiseRejectedResult => result.status === "rejected").map(result => String(result.reason));
    if ((project.status === "fulfilled" && project.value.project.id !== id) || (chat.status === "fulfilled" && chat.value.conversation.projectId !== id)) failures.push("返回的项目身份不匹配");
    setHealthy(!failures.length);
    if (failures.length) setError(`刷新协作状态失败：${failures.join("；")}`);
    else setError(operationError.current);
  }, [projectId]);
  useEffect(() => {
    if (!projectId || !active) return;
    setDraft(stored(draftKey(projectId))); setPending(storedPending(projectId));
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1500);
    return () => { clearInterval(timer); request.current++; };
  }, [active, projectId, refresh]);
  const agents = useMemo(() => snapshot?.agents.filter(agent => agent.projectId === projectId && agent.enabled) ?? [], [snapshot, projectId]);
  const supported = conversation?.supportedDiscussionRuntimes ?? [];
  useEffect(() => { if (!active) setMembersOpen(null); }, [active]);
  useLayoutEffect(() => {
    if (membersOpen !== "header") return;
    const place = () => {
      const anchor = headerMemberTrigger.current?.getBoundingClientRect();
      const panel = headerMemberTrigger.current?.closest(".collaboration-conversation")?.getBoundingClientRect();
      if (!anchor || !panel) return;
      const width = Math.min(416, Math.max(300, panel.width * .41), panel.width - 28, innerWidth - 16);
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
    if (initializedMembers.current !== projectId) { initializedMembers.current = projectId; setSelected(eligible); setMode(latestConversationRound(conversation.rounds)?.mode ?? "discussion"); }
    else setSelected(ids => ids.filter(id => eligible.includes(id)));
  }, [agents, conversation, snapshot, projectId, runtimeReady, runtimeError, runtimePrefs, isRuntimeEnabled]);
  const round = latestConversationRound(conversation?.rounds ?? []);
  const discussing = conversationIsActive(round);
  const messages = orderedConversationMessages(conversation?.messages ?? [], projectId ?? "");
  useEffect(() => {
    if (!pending || !conversation || conversation.conversation.projectId !== pending.projectId || projectId !== pending.projectId) return;
    if (!conversation.messages.some(message => message.clientMessageId === pending.clientMessageId)) return;
    save(pendingKey(pending.projectId), "");
    if (stored(draftKey(pending.projectId)) === pending.body) save(draftKey(pending.projectId), "");
    setDraft(value => value === pending.body ? "" : value);
    setPending(null); setOperationError("");
  }, [conversation, pending, projectId]);
  const visibleMessages = showOlder ? messages : messages.slice(-100);
  const tasks = visibleProjectTasks(snapshot?.tasks ?? []);
  const openProjectTasks = () => { if (snapshot && projectId && active) { setMembersOpen(null); onOpenProjectSettings?.(projectId, "tasks"); } };
  const runningTasks = tasks.filter(task => ["claimed", "running", "waiting_approval", "in_review"].includes(task.status));
  const activeAgent = discussing ? agents.find(agent => conversation?.messages.some(message => message.roundId === round?.id && message.senderType === "agent" && message.agentId === agent.id && message.status === "running")) : undefined;
  useEffect(() => {
    if (followMessages.current && transcript.current) transcript.current.scrollTop = transcript.current.scrollHeight;
  }, [messages.length, messages[messages.length - 1]?.updatedAt, projectId]);
  const changeDraft = (value: string) => { setDraft(value); if (projectId) save(draftKey(projectId), value); };
  const mutationCall = async (command: string, args: Record<string, unknown>) => {
    if (mutation.current || !projectId || !active) return null;
    const id = projectId; const epoch = generation.current;
    mutation.current = true; setBusy(true); request.current++; setOperationError("");
    try {
      const value = await invoke<ConversationSnapshot>(command, args);
      if (mounted.current && currentProject.current === id && generation.current === epoch) {
        if (value.conversation.projectId !== id) throw Error("协作结果项目身份不匹配。");
        setConversation(value); setHealthy(true); followMessages.current = true;
      }
      return value;
    } catch (reason) {
      if (mounted.current && currentProject.current === id && generation.current === epoch) setOperationError(String(reason));
      throw reason;
    } finally { mutation.current = false; if (mounted.current) setBusy(false); }
  };
  const deliver = async (item: PendingSend) => {
    try {
      const result = await mutationCall(item.interrupt ? "ma_conversation_interrupt" : "ma_conversation_send", { input: item.interrupt
        ? { projectId: item.projectId, clientMessageId: item.clientMessageId, body: item.body }
        : { projectId: item.projectId, clientMessageId: item.clientMessageId, body: item.body, participantAgentIds: item.participantAgentIds, mode: item.mode, maxRounds: 1, maxDurationSeconds: 600 } satisfies ConversationSendInput });
      if (!result) return;
      save(pendingKey(item.projectId), "");
      if (stored(draftKey(item.projectId)) === item.body) save(draftKey(item.projectId), "");
      if (mounted.current && currentProject.current === item.projectId) {
        setPending(null); setDraft(value => { if (value === item.body) { save(draftKey(item.projectId), ""); return ""; } return value; });
        setMentionOpen(false); void refresh();
      }
    } catch { /* Retain the same identity and payload for a safe status check/retry. */ }
  };
  const submit = async (event?: FormEvent, bodyOverride?: string) => {
    event?.preventDefault();
    if (mutation.current || !projectId || !healthy || !active || !runtimeReady || runtimeError || pending || ["stopping", "unconfirmed"].includes(round?.status ?? "")) return;
    const body = (bodyOverride ?? draft).trim();
    if (!body || Array.from(body).length > 32000) { setOperationError("请输入 1–32000 字符的消息。"); return; }
    try {
      const ids = conversationRecipients(body, selected, agents, projectId);
      if (ids.some(id => !isRuntimeEnabled(agents.find(agent => agent.id === id)!.runtimeKind))) throw Error("目标成员的 Runtime 已停用，请在协作设置中启用后重试。");
      if (ids.some(id => !supported.includes(agents.find(agent => agent.id === id)!.runtimeKind))) throw Error("所选成员暂不支持受限讨论，请调整成员或在原会话中操作。");
      const item: PendingSend = { projectId, clientMessageId: crypto.randomUUID(), body, participantAgentIds: ids, mode, interrupt: discussing };
      save(pendingKey(projectId), JSON.stringify(item)); setPending(item); await deliver(item);
    } catch (reason) { setOperationError(String(reason)); }
  };
  const control = async (action: "pause" | "cancel") => {
    try { await mutationCall("ma_conversation_control", { projectId, action }); void refresh(); } catch { /* Error is visible in the panel. */ }
  };
  const applyPlan = async (plan: ConversationExecutionPlan) => {
    if (!projectId) return;
    // Persist a stable operation identity before the request; uncertainty must not duplicate engineering tasks.
    const key = `crewvela-conversation-plan:${projectId}:${plan.id}`;
    const clientRequestId = stored(key) || crypto.randomUUID(); save(key, clientRequestId);
    try { await mutationCall("ma_conversation_execute_plan", { input: { projectId, planId: plan.id, clientRequestId } }); void refresh(); } catch { /* Backend deduplicates the retained identity. */ }
  };
  const openAgentSession = async (agentId: string) => {
    const id = snapshot?.agentSessions.find(session => session.agentId === agentId)?.appSessionId
      ?? sessions.find(session => session.multiAgentProjectId === projectId && session.multiAgentAgentId === agentId && !session.retiredAgentRuntime)?.id;
    if (!id || !onOpenSession) { if (projectId && onOpenWorkspace) onOpenWorkspace(projectId); else setError("原会话尚未建立，可在任务详情中查看启动状态。"); return; }
    try { await onOpenSession(id); } catch (reason) { setError(`打开原会话失败：${String(reason)}`); }
  };
  const mentionMatch = draft.slice(0, mentionCursor).match(/(?:^|\s)@([^@\n]*)$/u);
  const mentionOptions: AgentMentionOption[] = agents.filter(agent => supported.includes(agent.runtimeKind) && isRuntimeEnabled(agent.runtimeKind) && (!mentionMatch?.[1] || conversationMentionToken(agent, agents).toLocaleLowerCase().includes(mentionMatch[1].toLocaleLowerCase()))).sort((a, b) => Number(b.name.toLocaleLowerCase().startsWith(mentionMatch?.[1]?.toLocaleLowerCase() ?? "")) - Number(a.name.toLocaleLowerCase().startsWith(mentionMatch?.[1]?.toLocaleLowerCase() ?? ""))).map(agent => ({ key: agent.id, kind: "agent", agentId: agent.id, token: conversationMentionToken(agent, agents), description: ROLE[agent.role] ?? agent.role, section: "具体 Agent", runtimeKind: agent.runtimeKind, avatarId: agent.avatarId }));
  const insertMention = (option: AgentMentionOption) => {
    const prefix = draft.slice(0, mentionCursor).replace(/@[^@\n]*$/u, option.token + " ");
    const next = prefix + draft.slice(mentionCursor); changeDraft(next); setMentionOpen(false);
    if (option.agentId) setSelected(ids => [...new Set([...ids, option.agentId!])]);
    requestAnimationFrame(() => { editor.current?.focus(); editor.current?.setSelectionRange(prefix.length, prefix.length); });
  };
  const allMentionsSelected = mentionOptions.length > 0 && mentionOptions.every(option => hasMentionToken(draft, option.token));
  const selectAllMentions = () => {
    const withoutQuery = draft.slice(0, mentionCursor).replace(/@[^@\n]*$/u, "") + draft.slice(mentionCursor);
    const next = selectMentionTokens(withoutQuery, mentionOptions.map(option => option.token), !allMentionsSelected);
    const ids = mentionOptions.map(option => option.agentId!);
    setSelected(current => allMentionsSelected ? current.filter(id => !ids.includes(id)) : [...new Set([...current, ...ids])]);
    changeDraft(next); setMentionOpen(false);
    requestAnimationFrame(() => { editor.current?.focus(); editor.current?.setSelectionRange(next.length, next.length); });
  };
  const toggleMember = (agent: AgentProfile) => setSelected(ids => ids.includes(agent.id) ? ids.filter(id => id !== agent.id) : [...ids, agent.id]);
  const renderPlan = (plan: ConversationExecutionPlan) => <section className="conversation-plan" aria-label={plan.status === "pending" ? "待执行分工" : "项目分工"} key={plan.id}>
    <h3><ListChecks size={19} />{plan.status === "pending" ? "待执行分工" : plan.status === "applied" ? "分工已确认" : "分工已更新"}</h3>
    <p>{plan.summary}</p>
    <ul>{plan.tasks.map((task, index) => {
      const actual = snapshot?.tasks.find(item => item.id === plan.taskIds[index]);
      return <li key={task.key}><strong>{agents.find(agent => agent.id === task.assigneeAgentId)?.name ?? task.assigneeAgentId}</strong><span>{task.title}</span><small className={actual?.status ?? ""}>{actual ? TASK_STATUS[actual.status] ?? actual.status : "待执行"}</small></li>;
    })}</ul>
    {plan.status === "pending" && <>{plan.tasks.map(task => <div className="conversation-plan-scope" key={task.key}><strong>{task.title}</strong><p>{task.workspaceMode === "worktree" ? "独立工作树" : "只读任务"} · 写入：{task.writePaths.length ? task.writePaths.join("、") : "无"}</p><p>验收命令：<code>{task.verificationCommand ?? "未指定命令"}</code></p><details><summary>验收要求与依赖</summary><p>{task.acceptanceCriteria}</p><p>前置任务：{task.dependsOn.map(key => plan.tasks.find(item => item.key === key)?.title ?? key).join("、") || "无"}</p>{task.reviewerAgentId && <p>审查：{agents.find(agent => agent.id === task.reviewerAgentId)?.name ?? task.reviewerAgentId}</p>}</details></div>)}<button type="button" className="conversation-primary" disabled={busy || !healthy || discussing || !active} onClick={() => void applyPlan(plan)}>按此分工执行</button></>}
  </section>;

  return <section className="collaboration-conversation" aria-label="团队协作会话">
    <header className="conversation-header">
      <div className="conversation-heading"><span className="conversation-project-select"><select aria-label="协作项目" title={snapshot?.project.rootPath ? `项目目录：${snapshot.project.rootPath}` : undefined} value={projectId ?? ""} onChange={event => chooseProject(event.target.value || null)}><option value="" disabled>选择协作项目</option>{projects.map(project => <option key={project.id} value={project.id}>{project.name}</option>)}</select><CaretDown size={15} aria-hidden="true" /></span>
        <button type="button" className="conversation-icon" aria-label="创建协作项目" onClick={() => setCreatingProject(true)}><Plus size={18} /></button>
        {onOpenProjectSettings && <button type="button" className="conversation-icon" aria-label="管理项目与成员" onClick={() => onOpenProjectSettings(projectId ?? undefined)}><GearSix size={18} /></button>}
      </div>
      <div className="conversation-context-row"><div className="conversation-context"><button type="button" className="conversation-text-button conversation-task-link" disabled={!snapshot || !onOpenProjectSettings} onClick={openProjectTasks}>查看任务 · {tasks.length}<ArrowSquareOut size={14} /></button></div>
        <div className="conversation-members" aria-label="当前团队成员"><button ref={headerMemberTrigger} type="button" className="conversation-members-trigger" aria-label={`选择参与成员，已选 ${selected.length} 位，共 ${agents.length} 位`} aria-expanded={membersOpen === "header"} aria-controls={membersOpen === "header" ? membersId : undefined} onClick={() => setMembersOpen(value => value === "header" ? null : "header")} disabled={!snapshot}>
          <span className="conversation-member-preview" aria-hidden="true">{agents.slice(0, 3).map(agent => <span key={agent.id} data-selected={selected.includes(agent.id)}><AgentAvatar avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative /></span>)}</span><span>成员 {agents.length}</span><CaretDown size={14} /></button></div>
      </div>
      <div className="conversation-modes" role="tablist" aria-label="协作方式">{([['discussion', '讨论'], ['execution_plan', '执行项目']] as const).map(([value, label]) => <button type="button" role="tab" key={value} aria-selected={mode === value} disabled={busy} onClick={() => setMode(value)}>{label}</button>)}</div>
    </header>
    {membersOpen === "header" && createPortal(<div id={membersId} ref={membersMenu} className="conversation-member-menu" data-wide={membersPosition.width > 350} role="group" aria-label="选择参与成员" style={membersPosition}><header>参与成员</header>{agents.map(agent => {
      const eligible = supported.includes(agent.runtimeKind) && isRuntimeEnabled(agent.runtimeKind);
      const reason = !runtimeReady ? "正在读取 Runtime 状态" : !isRuntimeEnabled(agent.runtimeKind) ? "Runtime 已停用" : !supported.includes(agent.runtimeKind) ? "暂不支持受限讨论" : "";
      return <label key={agent.id} className={eligible ? "" : "unavailable"} title={`${agent.name} · ${RUNTIME_LABELS[agent.runtimeKind]}${reason ? ` · ${reason}` : ""}`}><input type="checkbox" aria-label={agent.name} checked={selected.includes(agent.id)} disabled={!eligible || busy} onChange={() => toggleMember(agent)} /><AgentAvatar avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative /><span className="conversation-member-name"><strong>{agent.name}</strong><small>{ROLE[agent.role] ?? agent.role}{reason && ` · ${reason}`}{activeAgent?.id === agent.id && " · 正在回复"}</small></span><Check size={18} className="conversation-member-check" aria-hidden="true" /></label>;
    })}{!agents.length && <p>还没有团队成员。{onOpenProjectSettings && <button type="button" className="conversation-text-button" onClick={() => { setMembersOpen(null); onOpenProjectSettings(projectId ?? undefined); }}>添加成员</button>}</p>}</div>, document.body)}

    {(error || runtimeError) && <div className="conversation-error" role="alert"><WarningCircle size={17} /><span>{error || `读取 Runtime 启用状态失败：${runtimeError}`}</span><button type="button" disabled={busy || loading} onClick={() => { void loadProjects().catch(reason => setError(String(reason))); void refreshAvailability(); void refresh(); }}>重新读取</button></div>}
    <div className="conversation-transcript" role="log" aria-label="团队对话" aria-live="polite" ref={transcript} onScroll={event => { const node = event.currentTarget; followMessages.current = node.scrollHeight - node.scrollTop - node.clientHeight < 90; }}>
      {!projectId ? <div className="conversation-empty"><ChatCircleText size={30} /><h2>让 Agent 组成团队</h2><p>创建一个协作项目，选择成员，在这里讨论或分工开发。</p><button type="button" className="conversation-primary" onClick={() => setCreatingProject(true)}>创建协作项目</button></div>
        : !conversation ? <p className="conversation-empty" role="status">{loading ? "正在读取协作会话…" : "协作会话尚未读取，请重试。"}</p>
        : !messages.length ? <div className="conversation-empty"><ChatCircleText size={30} /><h2>和团队开始对话</h2><p>选择成员后发送话题。讨论会逐位收集观点、相互回应并汇总；执行项目会先生成分工，确认后才开始工程任务。</p></div> : null}
      {messages.length > 100 && !showOlder && <button type="button" className="conversation-text-button conversation-older" onClick={() => setShowOlder(true)}>查看更早的 {messages.length - 100} 条消息</button>}
      {visibleMessages.map((message, index) => {
        const agent = agents.find(item => item.id === message.agentId) ?? snapshot?.agents.find(item => item.id === message.agentId);
        const plan = conversation?.executionPlans.find(item => item.messageId === message.id);
        if (message.senderType === "system") return <div className={`conversation-event ${message.status}`} key={message.id}><span className="conversation-status-dot" /><p>{message.body || CONVERSATION_STATUS[message.status]}</p>{message.agentId && <button type="button" className="conversation-text-button" onClick={() => void openAgentSession(message.agentId!)}>查看原会话<ArrowSquareOut size={13} /></button>}</div>;
        const previous = visibleMessages[index - 1];
        const continuation = !!plan && previous?.senderType === "agent" && previous.agentId === message.agentId;
        return <article className={`conversation-message ${message.senderType} ${message.status}${continuation ? " plan-continuation" : ""}`} key={message.id} data-message-id={message.id}>
          {agent ? <AgentAvatar className="conversation-avatar" avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative /> : <span className="conversation-avatar">{message.senderType === "user" ? <User size={21} /> : <Robot size={21} />}</span>}
          <div className="conversation-message-content"><header><strong>{message.senderType === "user" ? "我" : agent?.name ?? "历史成员"}</strong>{message.senderType === "agent" && <small>{message.phase === "summary" ? "汇总" : message.phase === "responses" ? "回应" : "观点"}</small>}{message.senderType !== "user" && message.status !== "completed" && <span className={`conversation-message-status ${message.status}`}>{CONVERSATION_STATUS[message.status]}</span>}{message.senderType === "agent" && message.runId && <button className="conversation-text-button conversation-message-source" type="button" title="查看原会话" aria-label={`查看${agent?.name ?? "历史成员"}的原会话`} onClick={() => void openAgentSession(message.agentId!)}><ArrowSquareOut size={14} /></button>}</header>
            {!plan && (message.body ? <div className="conversation-message-body">{message.body}</div> : <p className="conversation-pending">{CONVERSATION_STATUS[message.status]}{message.status === "queued" ? "，尚未收到成员回复。" : message.status === "running" ? "，等待真实回复内容…" : "，没有已确认的回复内容。"}</p>)}
            {plan && renderPlan(plan)}
          </div>
        </article>;
      })}
      {conversation?.executionPlans.filter(plan => !messages.some(message => message.id === plan.messageId)).map(renderPlan)}
      {snapshot?.runs.filter(run => run.sessionLocator?.stopUnverified === true).map(run => <UnverifiedRunNotice key={run.id} run={run} taskTitle={snapshot.tasks.find(task => task.id === run.taskId)?.title} onConfirmed={refresh} />)}
    </div>

    <footer className="conversation-footer">
      {round && <div className="conversation-progress" role="status"><span className={`conversation-status-dot ${round.status}`} /><span>{activeAgent ? `${activeAgent.name} · ` : ""}{round.status === "completed" ? "本轮讨论结束" : CONVERSATION_STATUS[round.status]}{round.stopReason ? ` · ${round.stopReason}` : ""}</span>{discussing && <button className="conversation-text-button" type="button" disabled={busy || round.status === "stopping"} onClick={() => void control("pause")}><Pause size={13} />{round.status === "unconfirmed" ? "重新核对停止" : "暂停讨论"}</button>}{discussing && <button type="button" className="conversation-text-button" aria-label="结束本轮讨论" disabled={busy || round.status === "stopping"} onClick={() => void control("cancel")}><Stop size={13} />结束本轮</button>}{["paused", "cancelled"].includes(round.status) && <button className="conversation-text-button" type="button" disabled={busy || !!pending || !healthy} onClick={() => void submit(undefined, mode === "discussion" ? "继续讨论，请结合以上人工补充与已确认的上下文。" : "继续拟定项目分工，请结合以上人工补充与已确认的上下文。")}>{mode === "discussion" ? "继续讨论" : "继续拟定分工"}</button>}</div>}
      {runningTasks.length > 0 && <div className="conversation-progress"><ListChecks size={15} /><span>{runningTasks.length} 项工程任务仍在执行或等待处理</span><button className="conversation-text-button" type="button" disabled={!onOpenProjectSettings} onClick={openProjectTasks}>查看任务</button></div>}
      {pending && <div className="conversation-send-pending" role="status"><span>{busy ? "正在发送…" : "本次发送结果未确认，重新核对会使用同一消息编号。"}</span>{!busy && <button type="button" onClick={() => void deliver(pending)}>核对并重试</button>}</div>}
      <form className="conversation-composer multi-agent-composer" onSubmit={event => void submit(event)}>
        <div className="conversation-editor multi-agent-composer-editor">
          <textarea ref={editor} aria-label={discussing ? "给团队插话" : "给团队发消息"} role="combobox" aria-expanded={mentionOpen && !!mentionMatch && mentionOptions.length > 0} aria-controls="conversation-mentions" aria-activedescendant={mentionOpen && mentionOptions.length ? `conversation-mentions-option-${Math.min(mentionIndex, mentionOptions.length - 1)}` : undefined} value={draft} placeholder={discussing ? "输入补充，暂停后续讨论并交给团队…" : "输入需求，@ 指定 Agent；不指定则发给当前团队"} disabled={!projectId} onChange={event => { changeDraft(event.target.value); setMentionCursor(event.target.selectionStart); setMentionIndex(0); setMentionOpen(true); }} onSelect={event => { if (event.currentTarget.selectionStart !== mentionCursor) { setMentionCursor(event.currentTarget.selectionStart); setMentionOpen(true); } }} onKeyDown={event => {
            if (event.nativeEvent.isComposing || event.keyCode === 229) return;
            if (mentionOpen && mentionMatch && mentionOptions.length) {
              if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); setMentionOpen(false); return; }
              if (["ArrowDown", "ArrowUp"].includes(event.key)) { event.preventDefault(); setMentionIndex(value => (value + (event.key === "ArrowDown" ? 1 : -1) + mentionOptions.length) % mentionOptions.length); return; }
              if (event.key === "Enter" || event.key === "Tab") { event.preventDefault(); insertMention(mentionOptions[Math.min(mentionIndex, mentionOptions.length - 1)]); return; }
            }
            if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); void submit(); }
          }} />
          {mentionOpen && mentionMatch && mentionOptions.length > 0 && <AgentMentionMenu id="conversation-mentions" options={mentionOptions} activeIndex={Math.min(mentionIndex, mentionOptions.length - 1)} onHighlight={setMentionIndex} onSelect={insertMention} allSelected={allMentionsSelected} onSelectAll={selectAllMentions} />}
        </div>
        <div className="conversation-composer-actions"><button ref={composerMemberTrigger} type="button" className="conversation-target" aria-expanded={membersOpen === "composer"} aria-controls={membersOpen === "composer" ? membersId : undefined} onClick={() => setMembersOpen(value => value === "composer" ? null : "composer")} disabled={!agents.length}><User size={16} /><span>{selected.length === agents.length && selected.length ? "发给当前团队" : `已选 ${selected.length} 位成员`}</span><CaretDown size={12} /></button><small>{mode === "discussion" ? "讨论" : "先生成分工"}{discussing ? " · 人工插话" : ""}</small><button type="submit" className="conversation-send" aria-label={discussing ? "发送插话" : "发送消息"} disabled={busy || !healthy || !active || !runtimeReady || !!runtimeError || !projectId || !draft.trim() || !!pending || ["stopping", "unconfirmed"].includes(round?.status ?? "")}><PaperPlaneTilt size={21} weight="fill" /></button></div>
        {membersOpen === "composer" && <div id={membersId} ref={membersMenu} className="conversation-recipient-menu" aria-label="选择参与成员"><header><strong>参与成员</strong><button type="button" onClick={() => { const eligible = agents.filter(agent => supported.includes(agent.runtimeKind) && isRuntimeEnabled(agent.runtimeKind)).map(agent => agent.id); setSelected(selected.length === eligible.length ? [] : eligible); }}>全选 / 取消全选</button></header>{agents.map(agent => <label key={agent.id}><input type="checkbox" checked={selected.includes(agent.id)} disabled={!supported.includes(agent.runtimeKind) || !isRuntimeEnabled(agent.runtimeKind) || busy} onChange={() => toggleMember(agent)} /><AgentAvatar avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative /><span>{agent.name}<small>{RUNTIME_LABELS[agent.runtimeKind]}{!isRuntimeEnabled(agent.runtimeKind) ? " · Runtime 已停用" : !supported.includes(agent.runtimeKind) ? " · 暂不支持受限讨论" : ""}</small></span></label>)}</div>}
      </form>
      <p className="conversation-footnote"><WarningCircle size={13} />{discussing ? "插话会停止后续发言；正在执行的轮次需确认停止。" : mode === "discussion" ? "有界讨论，使用受限工具；工程任务需明确发起执行。" : "先生成待执行分工，确认成员、目录与验收要求后启动。"}</p>
    </footer>

    {creatingProject && <CollaborationProjectDialog onClose={() => setCreatingProject(false)} onCreate={async input => {
      const project = await invoke<MultiAgentProject>("ma_create_project", { input: { ...input, defaultBranch: "main" } });
      setProjects(items => [project, ...items.filter(item => item.id !== project.id)]); chooseProject(project.id);
    }} />}
  </section>;
}
