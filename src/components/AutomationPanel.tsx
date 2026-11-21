import { activeRun, currentRun, matchesRunFilter, planState, runStage, RUN_LABELS, type RunFilter } from "../automationExecution";
import "../styles/automation-execution.css";
import "../styles/automation-design.css";
import "../styles/settings-creative-modes.css";
import { SettingsStyleMenu } from "./SettingsStyleMenu";
import { buildAutomationAction, buildEditedAutomationAction, automationFormFromAction, eventAutomationTrigger, automationSessions, automationProjectRuntime, AUTOMATION_LABELS, type AutomationFormAction } from "../automationForm";
import { TERMINAL_RUNTIMES } from "../runtimeTaxonomy";
import { open as openDirectory } from "@tauri-apps/plugin-dialog";
import { FolderOpenIcon } from "@phosphor-icons/react/dist/csr/FolderOpen";
import { StackIcon } from "@phosphor-icons/react/dist/csr/Stack";
import { useRuntimeAvailability } from "../runtimeAvailability";
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Switch } from "./ui/Switch";
import { CalendarBlank, CheckCircle, XCircle, CircleNotch, MinusCircle, PauseCircle, TerminalWindow, Broadcast, ChatCircleText, ArrowSquareOut, ArrowClockwise, Play, Stop, Info, DotsThree } from "@phosphor-icons/react";
import { ClockIcon } from "@phosphor-icons/react/dist/csr/Clock";
import { CheckSquareIcon } from "@phosphor-icons/react/dist/csr/CheckSquare";
import { FileTextIcon } from "@phosphor-icons/react/dist/csr/FileText";
import { GearSixIcon } from "@phosphor-icons/react/dist/csr/GearSix";
import { LightningIcon } from "@phosphor-icons/react/dist/csr/Lightning";
import { PlusIcon } from "@phosphor-icons/react/dist/csr/Plus";
import { PencilSimpleIcon } from "@phosphor-icons/react/dist/csr/PencilSimple";
import { TrashIcon } from "@phosphor-icons/react/dist/csr/Trash";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import type {
  Automation,
  AutomationAction,
  AutomationRun,
  AutomationSettingsStyle,
  AutomationTrigger,
  NewAutomation,
  ProviderConfig,
  SessionRecord,
  AgentProfile,
  AgentRuntimeKind,
  RuntimeProbe,
  MultiAgentProject,
} from "../types";

interface Props {
  sessions: SessionRecord[];
  providers: ProviderConfig[];
  onClose?: () => void;
  embedded?: boolean;
  onFormOpenChange?: (mode: "create" | "edit" | null) => void;
  viewStyle?: AutomationSettingsStyle;
  onViewStyleChange?: (next: AutomationSettingsStyle) => void;
  onOpenAppearanceSettings?: () => void;
}

const AUTOMATION_STYLE_OPTIONS: ReadonlyArray<{ id: AutomationSettingsStyle; label: string }> = [
  { id: "classic", label: "经典列表" },
  { id: "sundial", label: "24 小时星盘" },
];

function sundialStarCoords(
  item: Automation,
  eventIndex: number,
  eventTotal: number,
): { x: number; y: number; isEvent: boolean } {
  const cx = 122;
  const cy = 122;
  const toPoint = (angleDeg: number, radius: number) => {
    const rad = ((angleDeg - 90) * Math.PI) / 180;
    return { x: cx + radius * Math.cos(rad), y: cy + radius * Math.sin(rad) };
  };
  if (item.trigger.kind === "daily" || item.trigger.kind === "weekly") {
    const hours = item.trigger.hour + item.trigger.minute / 60;
    return { ...toPoint((hours / 24) * 360, 86), isEvent: false };
  }
  if (item.trigger.kind === "once") {
    const dt = new Date(item.trigger.at);
    const hours = dt.getHours() + dt.getMinutes() / 60;
    return { ...toPoint((hours / 24) * 360, 86), isEvent: false };
  }
  if (typeof item.nextRunAt === "number" && Number.isFinite(item.nextRunAt)) {
    const dt = new Date(item.nextRunAt);
    const hours = dt.getHours() + dt.getMinutes() / 60;
    return { ...toPoint((hours / 24) * 360, 86), isEvent: false };
  }
  const slotAngle = (eventIndex / Math.max(1, eventTotal)) * 360 + 30;
  return { ...toPoint(slotAngle, 52), isEvent: true };
}

const pad = (value: number) => String(value).padStart(2, "0");

// ISO 1=周一..7=周日，与后端 AutomationTrigger::Weekly 的 weekdays 约定一致。
const WEEKDAY_LABELS: Array<{ value: number; label: string }> = [
  { value: 1, label: "一" },
  { value: 2, label: "二" },
  { value: 3, label: "三" },
  { value: 4, label: "四" },
  { value: 5, label: "五" },
  { value: 6, label: "六" },
  { value: 7, label: "日" },
];

function formatDateTime(ms: number): string {
  const date = new Date(ms);
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function truncate(value: string, max: number): string {
  const text = value.trim().replace(/\s+/g, " ");
  return text.length > max ? `${text.slice(0, max)}…` : text;
}

export function AutomationPanel({
  sessions,
  providers,
  onClose,
  embedded = false,
  onFormOpenChange,
  viewStyle = "classic",
  onViewStyleChange,
  onOpenAppearanceSettings,
}: Props) {
  const panelRoot = useRef<HTMLElement>(null);
  useEffect(() => {
    const dismissCardMenus = (event: PointerEvent) => {
      if (!(event.target instanceof Node)) return;
      panelRoot.current?.querySelectorAll<HTMLDetailsElement>("details.automation-card-menu[open]").forEach(menu => {
        if (!menu.contains(event.target as Node)) menu.open = false;
      });
    };
    // Capture outside pointers even when another panel stops event bubbling.
    document.addEventListener("pointerdown", dismissCardMenus, true);
    return () => document.removeEventListener("pointerdown", dismissCardMenus, true);
  }, []);
  const { ready: availabilityReady, error: availabilityError, prefs: runtimePreferences, isRuntimeEnabled } = useRuntimeAvailability();
  const [automations, setAutomations] = useState<Automation[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<{ item: Automation; initialRuntime: AgentRuntimeKind | "" } | null>(null);
  const nameInput = useRef<HTMLInputElement>(null);
  useEffect(() => { if (formOpen) nameInput.current?.focus(); }, [formOpen]);
  useEffect(() => {
    onFormOpenChange?.(formOpen ? editing ? "edit" : "create" : null);
    return () => onFormOpenChange?.(null);
  }, [formOpen, editing, onFormOpenChange]);
  const [busy, setBusy] = useState(false);
  const mutationInFlight = useRef(false);
  const listSequence = useRef(0);
  const [filter, setFilter] = useState<RunFilter>("all");
  const [selectedHistory, setSelectedHistory] = useState<string | null>(null);
  const historyInitialized = useRef(false);
  useEffect(() => {
    if (!historyInitialized.current && automations.length) { historyInitialized.current = true; setSelectedHistory(automations[0].id); }
  }, [automations]);
  const [confirmUnknownId, setConfirmUnknownId] = useState<string | null>(null);
  const manualRequests = useRef(new Map<string, string>());
  const openSession = (sessionId: string) => window.dispatchEvent(new CustomEvent("crewvela-automation-open-session", { detail: { sessionId } }));
  const runAction = async (item: Automation, run?: AutomationRun, stop = false, acknowledge = false) => {
    if (mutationInFlight.current) return;
    mutationInFlight.current = true; listSequence.current += 1; setBusy(true); setError(null);
    const key = `${item.id}:${run?.id ?? "manual"}`;
    const requestId = manualRequests.current.get(key) ?? crypto.randomUUID();
    manualRequests.current.set(key, requestId);
    try {
      const rows = await invoke<Automation[]>(acknowledge ? "acknowledge_automation_run" : stop ? "stop_automation_run" : "run_automation_now", acknowledge ? { id: item.id, runId: run?.id, confirmed: true } : stop ? { id: item.id, runId: run?.id } : { id: item.id, requestId, retryOf: run?.id ?? null });
      listSequence.current += 1; setAutomations(rows); manualRequests.current.delete(key); setConfirmUnknownId(null);
    } catch (reason) { setError(String(reason)); }
    finally { mutationInFlight.current = false; setBusy(false); }
  };
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);

  const [name, setName] = useState("");
  // "on_completed"/"on_failed" 是表单层合成值，buildTrigger 转成 on_session_event。
  const [triggerKind, setTriggerKind] = useState<
    AutomationTrigger["kind"] | "on_completed" | "on_failed"
  >("daily");
  const [onceAt, setOnceAt] = useState("");
  const [intervalMinutes, setIntervalMinutes] = useState("30");
  const [dailyTime, setDailyTime] = useState("09:00");
  const [weekdays, setWeekdays] = useState<number[]>([1, 2, 3, 4, 5]);
  const [weeklyTime, setWeeklyTime] = useState("09:00");
  const [cronExpr, setCronExpr] = useState("");
  const [cronPreview, setCronPreview] = useState<
    { state: "idle" } | { state: "ok"; runs: number[] } | { state: "error"; message: string }
  >({ state: "idle" });
  const [form, setForm] = useState<AutomationFormAction>({ runtime: "", mode: "new", source: "local", providerId: "", model: "", cwd: "", prompt: "", sessionId: "", projectId: "", agentId: "" });
  const patchForm = (patch: Partial<AutomationFormAction>) => setForm(current => ({ ...current, ...patch }));
  const [maxRuns, setMaxRuns] = useState("");
  const [maxFailures, setMaxFailures] = useState("3");
  const [overlap, setOverlap] = useState<"skip" | "allow">("skip");
  const [eventSource, setEventSource] = useState<"session" | "runtime" | "project" | "any">("session");
  const [eventSourceId, setEventSourceId] = useState("");
  const [eventRuntime, setEventRuntime] = useState<AgentRuntimeKind | "">("");
  const [targetProjects, setTargetProjects] = useState<MultiAgentProject[]>([]);
  const [targetAgents, setTargetAgents] = useState<AgentProfile[]>([]);
  const [probes, setProbes] = useState<RuntimeProbe[]>([]);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [probeLoading, setProbeLoading] = useState(false);
  const [probeRevision, setProbeRevision] = useState(0);
  const openCreate = () => {
    setEditing(null); setError(null); setName(""); setTriggerKind("daily"); setOnceAt(""); setIntervalMinutes("30");
    setDailyTime("09:00"); setWeekdays([1, 2, 3, 4, 5]); setWeeklyTime("09:00"); setCronExpr(""); setCronPreview({ state: "idle" });
    setForm({ runtime: "", mode: "new", source: "local", providerId: "", model: "", cwd: "", prompt: "", sessionId: "", projectId: "", agentId: "" });
    setMaxRuns(""); setMaxFailures("3"); setOverlap("skip"); setEventSource("session"); setEventSourceId(""); setEventRuntime(""); setFormOpen(true);
  };
  const openEdit = async (item: Automation) => {
    if (mutationInFlight.current) return;
    mutationInFlight.current = true; listSequence.current += 1; setBusy(true); setError(null);
    try {
      const projectId = "projectId" in item.action ? item.action.projectId : null;
      const agents = projectId ? (await invoke<{ agents: AgentProfile[] }>("ma_snapshot", { projectId })).agents : [];
      const values = automationFormFromAction(item.action, sessions, agents);
      setEditing({ item, initialRuntime: values.runtime }); setName(item.name); setForm(values);
      setMaxRuns(item.maxRuns?.toString() ?? ""); setMaxFailures(String(item.maxConsecutiveFailures)); setOverlap(item.overlapPolicy ?? "allow");
      setCronExpr(""); setCronPreview({ state: "idle" }); setEventSourceId(""); setEventRuntime(""); setEventSource("session");
      const trigger = item.trigger;
      if (trigger.kind === "on_session_event" || trigger.kind === "on_session_event_filtered") {
        setTriggerKind(trigger.event === "completed" ? "on_completed" : "on_failed");
        if (trigger.kind === "on_session_event") setEventSource("any");
        else if (trigger.runtimeKind) { setEventSource("runtime"); setEventRuntime(trigger.runtimeKind); }
        else { setEventSource(trigger.projectId ? "project" : "session"); setEventSourceId(trigger.projectId ?? trigger.sessionId ?? ""); }
      } else {
        setTriggerKind(trigger.kind);
        if (trigger.kind === "once") { const date = new Date(trigger.at); setOnceAt(`${date.getFullYear()}-${pad(date.getMonth()+1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${String(date.getMilliseconds()).padStart(3,"0")}`); }
        if (trigger.kind === "interval") setIntervalMinutes(String(trigger.minutes));
        if (trigger.kind === "daily") setDailyTime(`${pad(trigger.hour)}:${pad(trigger.minute)}`);
        if (trigger.kind === "weekly") { setWeeklyTime(`${pad(trigger.hour)}:${pad(trigger.minute)}`); setWeekdays([...trigger.weekdays]); }
        if (trigger.kind === "cron") setCronExpr(trigger.expr);
      }
      setFormOpen(true);
    } catch (reason) { setError(String(reason)); }
    finally { mutationInFlight.current = false; setBusy(false); }
  };
  const nativeBase: Partial<Record<AgentRuntimeKind, AgentRuntimeKind>> = { claude_chat: "claude_code", codex_chat: "codex", kimi_chat: "kimi_code", pi_chat: "pi", opencode_chat: "opencode" };
  const runtimeOptions = Object.entries(AUTOMATION_LABELS).filter(([kind]) => isRuntimeEnabled(kind as AgentRuntimeKind) && probes.some(p => p.kind === (nativeBase[kind as AgentRuntimeKind] ?? kind) && p.installed && !p.error));
  const availableSessions = form.runtime ? automationSessions(sessions, form.runtime) : [];
  const availableAgents = targetAgents.filter(a => a.enabled && a.runtimeKind === form.runtime && isRuntimeEnabled(a.runtimeKind));
  useEffect(() => {
    if (!formOpen) return;
    let alive = true;
    setProbeLoading(true); setProbeError(null);
    void invoke<RuntimeProbe[]>("ma_probe_runtimes").then(value => {
      if (!alive) return;
      setProbes(value);
      setForm(current => current.runtime ? current : { ...current, runtime: value.find(p => TERMINAL_RUNTIMES.includes(p.kind) && p.installed && !p.error && isRuntimeEnabled(p.kind))?.kind ?? "" });
    }).catch(reason => { if (alive) { setProbes([]); setProbeError(String(reason)); } }).finally(() => { if (alive) setProbeLoading(false); });
    void invoke<MultiAgentProject[]>("ma_list_projects").then(value => { if (alive) setTargetProjects(value); }).catch(reason => { if (alive) setError(`读取项目失败：${String(reason)}`); });
    return () => { alive = false; };
  }, [formOpen, probeRevision, availabilityReady, runtimePreferences, isRuntimeEnabled]);
  useEffect(() => {
    let alive = true;
    setTargetAgents([]);
    if (!form.projectId) return;
    void invoke<{ agents: AgentProfile[] }>("ma_snapshot", { projectId: form.projectId }).then(value => { if (alive) setTargetAgents(value.agents); }).catch(reason => { if (alive) setError(`读取成员失败：${String(reason)}`); });
    return () => { alive = false; };
  }, [form.projectId, probeRevision]);

  const refresh = useCallback(async () => {
    const sequence = ++listSequence.current;
    try {
      const latest = await invoke<Automation[]>("list_automations");
      if (sequence === listSequence.current) {
        setAutomations(latest);
        for (const [key,id] of manualRequests.current) if (latest.some(a => a.requestReceipts?.[id])) manualRequests.current.delete(key);
      }
    } catch (reason) {
      if (sequence === listSequence.current) setError(String(reason));
    }
  }, []);

  useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, 5000);
    return () => { window.clearInterval(timer); listSequence.current += 1; };
  }, [refresh]);

  // cron 表达式实时预览：输入防抖 500ms 后调后端解析，展示未来 3 个触发点。
  useEffect(() => {
    if (triggerKind !== "cron" || !cronExpr.trim()) {
      setCronPreview({ state: "idle" });
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      invoke<number[]>("preview_cron_trigger", { expr: cronExpr.trim() })
        .then((runs) => {
          if (!cancelled) setCronPreview({ state: "ok", runs });
        })
        .catch((reason) => {
          if (!cancelled) setCronPreview({ state: "error", message: String(reason) });
        });
    }, 500);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [cronExpr, triggerKind]);

  const buildTrigger = (): AutomationTrigger | string => {
    if (triggerKind === "once") {
      const at = new Date(onceAt).getTime();
      if (!onceAt || Number.isNaN(at)) return "请选择一次性触发的时间";
      return { kind: "once", at };
    }
    if (triggerKind === "on_completed" || triggerKind === "on_failed") {
      if (eventSource === "any" && editing?.item.trigger.kind === "on_session_event") return { kind: "on_session_event", event: triggerKind === "on_completed" ? "completed" : "failed" };
      if (eventSource === "any") return "请选择具体事件来源";
      try { return eventAutomationTrigger(triggerKind === "on_completed" ? "completed" : "failed", eventSource, eventSourceId, eventRuntime); }
      catch (reason) { return String(reason); }
    }
    if (triggerKind === "daily") {
      const [hour, minute] = dailyTime.split(":").map(Number);
      if (Number.isNaN(hour) || Number.isNaN(minute)) return "请选择每天触发的时间";
      return { kind: "daily", hour, minute };
    }
    if (triggerKind === "weekly") {
      const [hour, minute] = weeklyTime.split(":").map(Number);
      if (Number.isNaN(hour) || Number.isNaN(minute)) return "请选择每周触发的时间";
      if (weekdays.length === 0) return "每周触发至少选择一天";
      return { kind: "weekly", weekdays: [...weekdays].sort((a, b) => a - b), hour, minute };
    }
    if (triggerKind === "cron") {
      const expr = cronExpr.trim();
      if (!expr) return "请填写 cron 表达式";
      if (cronPreview.state === "error") return cronPreview.message;
      return { kind: "cron", expr };
    }
    const minutes = Number(intervalMinutes);
    if (!Number.isInteger(minutes) || minutes < 1 || minutes > 1440) {
      return "间隔分钟数必须在 1-1440 之间";
    }
    return { kind: "interval", minutes };
  };

  const buildAction = (): AutomationAction | string => {
    if (!availabilityReady) return availabilityError ?? "Runtime 启用配置尚未加载";
    if (probeLoading || !runtimeOptions.some(([kind]) => kind === form.runtime)) return "请选择本机可用的 Agent，或重新检测安装状态";
    try { return editing ? buildEditedAutomationAction(form, editing.item.action, editing.initialRuntime, isRuntimeEnabled, providers, sessions, targetAgents) : buildAutomationAction(form, isRuntimeEnabled, providers, sessions, targetAgents); }
    catch (reason) { return String(reason); }
  };
  const selectDirectory = async () => {
    try { const path = await openDirectory({ directory: true, multiple: false }); if (typeof path === "string") patchForm({ cwd: path }); }
    catch (reason) { setError(`选择目录失败：${String(reason)}`); }
  };

  const create = async () => {
    if (mutationInFlight.current) return;
    const trigger = buildTrigger();
    if (typeof trigger === "string") return setError(trigger);
    const action = buildAction();
    if (typeof action === "string") return setError(action);
    if (!name.trim()) return setError("请填写自动化名称");
    const runs = maxRuns.trim() ? Number(maxRuns) : null;
    if (runs !== null && (!Number.isInteger(runs) || runs < 1)) {
      return setError("最大触发次数至少为 1");
    }
    const failures = Number(maxFailures);
    if (!Number.isInteger(failures) || failures < 1 || failures > 10) return setError("熔断阈值必须在 1–10 之间");
    const input: NewAutomation = {
      name: name.trim(),
      trigger,
      action,
      maxRuns: runs,
      maxConsecutiveFailures: failures,
      overlapPolicy: overlap,
    };
    mutationInFlight.current = true;
    setBusy(true);
    setError(null);
    try {
      await invoke<Automation>(editing ? "update_automation" : "create_automation", editing ? { id: editing.item.id, input } : { input });
      setFormOpen(false);
      setEditing(null);
      setName("");
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      mutationInFlight.current = false;
      setBusy(false);
    }
  };

  const toggle = async (automation: Automation) => {
    if (mutationInFlight.current) return;
    mutationInFlight.current = true;
    setBusy(true);
    try {
      await invoke("set_automation_enabled", {
        id: automation.id,
        enabled: !automation.enabled,
      });
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      mutationInFlight.current = false;
      setBusy(false);
    }
  };

  const remove = async (id: string) => {
    if (mutationInFlight.current) return;
    mutationInFlight.current = true;
    setBusy(true);
    try {
      await invoke("delete_automation", { id });
      setConfirmDeleteId(null);
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      mutationInFlight.current = false;
      setBusy(false);
    }
  };

  const triggerText = (trigger: AutomationTrigger): string => {
    if (trigger.kind === "once") return `一次性 · ${formatDateTime(trigger.at)}`;
    if (trigger.kind === "daily") {
      return `每天 ${pad(trigger.hour)}:${pad(trigger.minute)}`;
    }
    if (trigger.kind === "weekly") {
      const days = [...trigger.weekdays].sort((a, b) => a - b);
      const key = days.join(",");
      const label =
        days.length === 7
          ? "每天"
          : key === "1,2,3,4,5"
            ? "工作日"
            : `周${days
                .map((day) => WEEKDAY_LABELS.find((item) => item.value === day)?.label ?? day)
                .join("、")}`;
      return `${label} ${pad(trigger.hour)}:${pad(trigger.minute)}`;
    }
    if (trigger.kind === "cron") return `cron · ${trigger.expr}`;
    if (trigger.kind === "on_session_event" || trigger.kind === "on_session_event_filtered") {
      const scope = trigger.kind === "on_session_event_filtered" ? trigger.sessionId ? ` · 会话 ${trigger.sessionId.slice(0, 8)}` : trigger.projectId ? ` · 项目 ${trigger.projectId.slice(0, 8)}` : ` · ${AUTOMATION_LABELS[trigger.runtimeKind!] ?? "所选 Agent"}` : " · 旧任务：任意会话";
      return (trigger.event === "completed" ? "会话完成时" : "会话失败时") + scope;
    }
    return `每 ${trigger.minutes} 分钟`;
  };

  const actionText = (action: AutomationAction): string => {
    if (action.kind === "run_agent") return `${AUTOMATION_LABELS[action.runtimeKind]} · ${action.providerId ? providers.find(p => p.id === action.providerId)?.name ?? action.providerId : "本地配置"}${action.model ? ` · ${action.model}` : ""} · 新建会话 · ${action.cwd}`;
    if (action.kind === "send_agent") return action.agentId ? `${AUTOMATION_LABELS[action.runtimeKind]} · 项目成员 ${action.agentId.slice(0, 8)}` : `${AUTOMATION_LABELS[action.runtimeKind]} · 复用「${sessions.find(s => s.id === action.sessionId)?.title ?? action.sessionId.slice(0, 8)}」`;
    if (action.kind === "broadcast_agent") return `${AUTOMATION_LABELS[action.runtimeKind]} · 全部运行会话 · “${truncate(action.body, 24)}”`;
    if (action.kind === "launch") {
      if (action.agentId) {
        const project = targetProjects.find((item) => item.id === action.projectId);
        const agent = targetAgents.find((item) => item.id === action.agentId);
        return `启动「${project?.name ?? action.projectId}」中的 Agent「${agent?.name ?? action.agentId}」· “${truncate(action.prompt, 24)}”`;
      }
      const provider = providers.find((item) => item.id === action.providerId);
      return `启动新会话（${provider?.name ?? action.providerId}）· “${truncate(action.prompt, 24)}”`;
    }
    if (action.kind === "message") {
      if (action.agentId) return `发给项目 Agent「${action.agentId.slice(0, 8)}」· “${truncate(action.body, 24)}”`;
      const target = sessions.find((item) => item.id === action.sessionId);
      return `发给「${target?.title ?? `${action.sessionId.slice(0, 8)}（已删除）`}」· “${truncate(action.body, 24)}”`;
    }
    return `广播全部 · “${truncate(action.body, 24)}”`;
  };

  const nextRunText = (automation: Automation): string => {
    if (!automation.enabled) {
      return automation.consecutiveFailures >= automation.maxConsecutiveFailures
        ? "已熔断停用"
        : "已停用";
    }
    if (automation.nextRunAt) return formatDateTime(automation.nextRunAt);
    if (automation.trigger.kind === "on_session_event" || automation.trigger.kind === "on_session_event_filtered") return "等待事件";
    return "触发已结束";
  };

  const localToday = new Date();
  const nextPlan = automations.filter(item => planState(item) === "计划已启用" && typeof item.nextRunAt === "number" && Number.isFinite(item.nextRunAt)).sort((a,b) => a.nextRunAt! - b.nextRunAt!)[0];
  const shortId = (run: AutomationRun) => run.id ? run.id.slice(0,8) : "历史记录";
  const displayStage = (stage: string) => stage === "running" ? "进行中" : RUN_LABELS[stage] ?? "结果未确认";
  const statusIcon = (stage: string) => stage === "completed" ? CheckCircle : stage === "failed" ? XCircle : stage === "waiting" ? PauseCircle : ["running","stopping","dispatching"].includes(stage) ? CircleNotch : ["queued","submitted","received"].includes(stage) ? ClockIcon : MinusCircle;
  const actionIcon = (action: AutomationAction) => action.kind === "run_agent" || action.kind === "launch" ? TerminalWindow : action.kind === "broadcast_agent" || action.kind === "broadcast" ? Broadcast : ChatCircleText;
  const compactAction = (action: AutomationAction) => {
    if (action.kind !== "run_agent") return actionText(action);
    const folder = action.cwd.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || action.cwd;
    return `${(AUTOMATION_LABELS[action.runtimeKind] ?? action.runtimeKind).replace(/ CLI$/, "")} · ${folder}`;
  };
  const viewHistory = (item: Automation) => setSelectedHistory(selectedHistory === item.id ? null : item.id);
  const headingId = embedded && !formOpen ? "settings-title" : "automation-title";

  const panel = (
      <section
        ref={panelRoot}
        className={`automation-dialog automation-design${embedded ? " embedded" : ""}`}
        data-automation-view={formOpen ? "form" : "list"}
        role={embedded ? undefined : "dialog"}
        aria-modal={embedded ? undefined : "true"}
        aria-labelledby={headingId}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="automation-content">
          <div className="automation-heading">
            <div className="automation-heading-copy">
              <div className="automation-title-row">
                <GearSixIcon size={24} weight="regular" aria-hidden="true" />
                <h2 id={headingId}>{formOpen ? editing ? "编辑定时任务" : "创建自动化" : "自动化"}</h2>
              </div>
              <p>{formOpen ? "选择 Agent 与执行任务，按时间或指定事件派发；记录保存在本机。" : "按计划运行任务，或在需要时随时执行。"}</p>
            </div>
            <div className="settings-heading-actions">
              {!formOpen && onViewStyleChange && (
                <SettingsStyleMenu
                  label="自动化页面模式"
                  value={viewStyle}
                  options={AUTOMATION_STYLE_OPTIONS}
                  onChange={onViewStyleChange}
                  onOpenAppearanceSettings={onOpenAppearanceSettings}
                />
              )}
              {!formOpen && <div className="automation-calendar"><span><CalendarBlank size={18} aria-hidden="true" /><time>{`${localToday.getFullYear()}-${pad(localToday.getMonth()+1)}-${pad(localToday.getDate())}`}</time></span><span title={nextPlan ? nextPlan.name : undefined}><ClockIcon size={18} aria-hidden="true" />{nextPlan ? `下次计划：${formatDateTime(nextPlan.nextRunAt!)}` : "暂无待触发计划"}</span></div>}
              {onClose && <button className="btn-icon" aria-label="关闭" title="关闭" disabled={busy} onClick={onClose}>
                <XIcon size={15} />
              </button>}
              <button className="btn automation-primary" disabled={busy} onClick={() => formOpen ? setFormOpen(false) : openCreate()}>
                <PlusIcon size={13} weight="bold" /> {formOpen ? "收起表单" : "新建自动化"}
              </button>
            </div>
          </div>

          <div className="automation-scroll-area">
            {error && !formOpen && <p className="automation-error" role="alert">{error}</p>}

            {formOpen && (
              <fieldset className="automation-form" disabled={busy} style={{ minWidth: 0, border: 0, margin: 0 }}>
                <section className="automation-section">
                  <h3><FileTextIcon size={18} aria-hidden="true" />基本信息</h3>
              <label>
                名称
                <input
                  ref={nameInput}
                  value={name}
                  maxLength={60}
                  placeholder="例如：每天早上巡检依赖更新"
                  onChange={(event) => setName(event.target.value)}
                />
              </label>
                </section>

                <section className="automation-section automation-agent-section">
                  <h3><StackIcon size={18} aria-hidden="true" />执行 Agent</h3>
                  {probeError && <p role="alert" className="automation-error">检测 Agent 失败：{probeError}</p>}
                  <label className="automation-setting-row"><span>执行 Agent</span><select aria-label="执行 Agent" value={form.runtime} disabled={!availabilityReady || probeLoading} onChange={e => {
                    const runtime = e.target.value as AgentRuntimeKind;
                    patchForm({ runtime, source: "local", providerId: "", model: "", sessionId: "", agentId: "", mode: (!TERMINAL_RUNTIMES.includes(runtime) && form.mode === "new") || (!automationProjectRuntime(runtime) && form.mode === "member") ? "session" : form.mode });
                  }}><option value="">{probeLoading ? "正在检测本机 Agent…" : "请选择 Agent"}</option>{form.runtime && !runtimeOptions.some(([kind]) => kind === form.runtime) && <option value={form.runtime} disabled>当前选择不可用</option>}{runtimeOptions.map(([kind, label]) => <option key={kind} value={kind}>{label}</option>)}</select><small>仅列出本机已安装、已启用的 Agent。<button type="button" className="automation-inline" disabled={probeLoading} onClick={() => setProbeRevision(n => n + 1)}>重新检测</button></small></label>
                </section>
                <section className="automation-section">
                  <h3><LightningIcon size={18} aria-hidden="true" />执行任务</h3>
                  <label className="automation-setting-row"><span>运行方式</span><select aria-label="运行方式" value={form.mode} onChange={e => patchForm({ mode: e.target.value as AutomationFormAction["mode"], sessionId: "", agentId: "" })}>
                    <option value="new" disabled={!form.runtime || !TERMINAL_RUNTIMES.includes(form.runtime)}>每次新建会话</option><option value="session">复用指定会话</option><option value="member" disabled={!automationProjectRuntime(form.runtime)}>交给项目指定成员</option><option value="broadcast">发送到此 Agent 的全部运行会话</option>
                  </select><small>启动和排队不代表任务完成。</small></label>
                  {form.mode === "new" && <label className="automation-setting-row"><span>工作目录</span><div className="automation-directory"><input aria-label="工作目录" value={form.cwd} placeholder="选择项目目录的完整路径" onChange={e => patchForm({ cwd: e.target.value })} /><button type="button" aria-label="选择工作目录" onClick={() => void selectDirectory()}><FolderOpenIcon size={18} /></button></div></label>}
                  {form.mode === "session" && <label className="automation-setting-row"><span>目标会话</span><select aria-label="目标会话" value={form.sessionId} onChange={e => patchForm({ sessionId: e.target.value })}><option value="">请选择具体会话</option>{availableSessions.map(s => <option key={s.id} value={s.id}>{s.title} · {s.cwd} · {s.status}</option>)}</select><small>已停止的会话需先恢复；执行时会再次核对身份。</small></label>}
                  {form.mode === "member" && <>
                    <label className="automation-setting-row"><span>目标项目</span><select aria-label="目标项目" value={form.projectId} onChange={e => patchForm({ projectId: e.target.value, agentId: "" })}><option value="">请选择项目</option>{targetProjects.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</select></label>
                    <label className="automation-setting-row"><span>目标成员</span><select aria-label="目标成员" value={form.agentId} onChange={e => patchForm({ agentId: e.target.value })}><option value="">请选择匹配该 Agent 的成员</option>{availableAgents.map(a => <option key={a.id} value={a.id}>{a.name}</option>)}</select><small>沿用成员目录；未运行成员交项目调度器执行，已有交互会话保留当前配置。</small></label>
                  </>}
                  {form.mode === "broadcast" && <p className="automation-scope-note">仅发送到所选 Agent 类型的未归档运行会话；执行时解析目标，包含其他工作区。不会广播给普通 Shell。</p>}
                  <label className="automation-setting-row automation-prompt-row"><span>Prompt</span><textarea aria-label="执行任务" rows={3} value={form.prompt} placeholder="检查当前项目的代码改动、测试与依赖，汇总问题和下一步建议。" onChange={e => patchForm({ prompt: e.target.value })} /></label>
                </section>
                <section className="automation-section">
                  <h3><ClockIcon size={18} aria-hidden="true" />触发与运行</h3>
              <div className="automation-form-row">
                <label>
                  触发
                  <select
                    value={triggerKind}
                    onChange={(event) => setTriggerKind(event.target.value as typeof triggerKind)}
                  >
                    <option value="once">一次性</option>
                    <option value="interval">每隔 N 分钟</option>
                    <option value="daily">每天</option>
                    <option value="weekly">每周 / 工作日</option>
                    <option value="cron">cron 表达式</option>
                    <option value="on_completed">会话完成时</option>
                    <option value="on_failed">会话失败时</option>
                  </select>
                </label>
                {(triggerKind === "on_completed" || triggerKind === "on_failed") && (
                  <p className="hint" style={{ margin: 0, alignSelf: "end" }}>
                    所选来源的会话{triggerKind === "on_completed" ? "执行完成" : "失败"}时触发；一批事件合并为一次
                  </p>
                )}
                {triggerKind === "once" && (
                  <label>
                    时间
                    <input
                      type="datetime-local"
                      step="0.001"
                      value={onceAt}
                      onChange={(event) => setOnceAt(event.target.value)}
                    />
                  </label>
                )}
                {triggerKind === "interval" && (
                  <label>
                    间隔（分钟）
                    <input
                      type="number"
                      min={1}
                      max={1440}
                      value={intervalMinutes}
                      onChange={(event) => setIntervalMinutes(event.target.value)}
                    />
                  </label>
                )}
                {triggerKind === "daily" && (
                  <label>
                    时间
                    <input
                      type="time"
                      value={dailyTime}
                      onChange={(event) => setDailyTime(event.target.value)}
                    />
                  </label>
                )}
                {triggerKind === "weekly" && (
                  <label>
                    时间
                    <input
                      type="time"
                      value={weeklyTime}
                      onChange={(event) => setWeeklyTime(event.target.value)}
                    />
                  </label>
                )}
                {triggerKind === "cron" && (
                  <label style={{ flex: 2 }}>
                    表达式（5 段：分 时 日 月 周）
                    <input
                      value={cronExpr}
                      placeholder="例如 */30 9-18 * * 1-5"
                      spellCheck={false}
                      onChange={(event) => setCronExpr(event.target.value)}
                    />
                  </label>
                )}
              </div>
              {triggerKind === "weekly" && (
                <div className="automation-weekdays" role="group" aria-label="选择星期">
                  {WEEKDAY_LABELS.map((item) => (
                    <button
                      key={item.value}
                      type="button"
                      className={weekdays.includes(item.value) ? "active" : ""}
                      aria-pressed={weekdays.includes(item.value)}
                      onClick={() =>
                        setWeekdays((current) =>
                          current.includes(item.value)
                            ? current.filter((day) => day !== item.value)
                            : [...current, item.value],
                        )
                      }
                    >
                      {item.label}
                    </button>
                  ))}
                </div>
              )}
              {triggerKind === "cron" && cronPreview.state !== "idle" && (
                <p className={`automation-cron-preview ${cronPreview.state}`}>
                  {cronPreview.state === "ok"
                    ? `接下来触发：${cronPreview.runs.map(formatDateTime).join("　·　")}`
                    : cronPreview.message}
                </p>
              )}
                  {(triggerKind === "on_completed" || triggerKind === "on_failed") && <div className="automation-event-source">
                    <label>事件来源<select aria-label="事件来源" value={eventSource} onChange={e => { setEventSource(e.target.value as typeof eventSource); setEventSourceId(""); }}><option value="session">指定会话</option><option value="runtime">指定 Agent 类型</option><option value="project">指定项目</option>{editing?.item.trigger.kind === "on_session_event" && <option value="any">全部会话（原有配置）</option>}</select></label>
                    {eventSource === "runtime" && <label>来源 Agent<select aria-label="来源 Agent" value={eventRuntime} onChange={e => setEventRuntime(e.target.value as AgentRuntimeKind)}><option value="">请选择来源 Agent</option>{runtimeOptions.map(([kind, label]) => <option key={kind} value={kind}>{label}</option>)}</select></label>}
                    {eventSource !== "runtime" && eventSource !== "any" && <label>来源目标<select aria-label="来源目标" value={eventSourceId} onChange={e => setEventSourceId(e.target.value)}><option value="">请选择具体来源</option>{eventSource === "session" ? sessions.filter(s => !s.retiredAgentRuntime && !s.archivedAt && (s.agentRuntimeKind || s.kind === "claude")).map(s => <option key={s.id} value={s.id}>{s.title}</option>) : targetProjects.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</select></label>}
                    <p>只响应所选来源的状态事件；请避免任务自身的完成事件不断触发下一轮，可在高级设置中限制次数。</p>
                  </div>}
                  <label className="automation-setting-row"><span>上一轮未结束</span><select aria-label="上一轮未结束" value={overlap} onChange={e => setOverlap(e.target.value as "skip" | "allow")}><option value="skip">跳过本轮</option><option value="allow">允许继续派发</option></select><small>{overlap === "skip" ? "任务或发送队列仍未结束时不重复启动。" : "可能并行新建会话；复用目标仍按其原生队列执行。"}</small></label>
                  <details className="automation-options"><summary>高级设置（最大次数、连续失败熔断）</summary><div className="automation-form-row"><label>最大触发次数（留空不限）<input aria-label="最大触发次数" type="number" min={1} value={maxRuns} onChange={e => setMaxRuns(e.target.value)} /></label><label>连续失败熔断<input aria-label="连续失败熔断" type="number" min={1} max={10} value={maxFailures} onChange={e => setMaxFailures(e.target.value)} /></label></div></details>
                </section>

              </fieldset>
            )}

            {!formOpen && <section className="automation-list-section automation-executions">
              <nav className="automation-filters" aria-label="执行状态筛选">
                {([["all", "全部"], ["active", "进行中"], ["completed", "已完成"], ["failed", "失败"], ["attention", "待处理"]] as const).map(([key, label]) => <button key={key} aria-pressed={filter === key} onClick={() => setFilter(key)}>{label}<span>{automations.filter(a => matchesRunFilter(a, key)).length}</span></button>)}
                <button className="automation-refresh" aria-label="刷新自动化" title="刷新自动化" disabled={busy} onClick={() => void refresh()}><ArrowClockwise size={18} /></button>
              </nav>
              {viewStyle === "sundial" ? (() => {
                const filtered = automations.filter((a) => matchesRunFilter(a, filter));
                const focused = filtered.find((a) => a.id === selectedHistory) ?? filtered[0] ?? null;
                const eventItems = filtered.filter((a) =>
                  a.trigger.kind !== "daily"
                  && a.trigger.kind !== "weekly"
                  && a.trigger.kind !== "once"
                  && !(typeof a.nextRunAt === "number" && Number.isFinite(a.nextRunAt)),
                );
                const nowHours = localToday.getHours() + localToday.getMinutes() / 60;
                const nowRad = (((nowHours / 24) * 360 - 90) * Math.PI) / 180;
                const nowX = 122 + 74 * Math.cos(nowRad);
                const nowY = 122 + 74 * Math.sin(nowRad);
                const focusedRun = focused ? currentRun(focused) : undefined;
                const focusedStage = runStage(focusedRun);
                const focusedActive = focused ? focused.recentRuns.some(activeRun) : false;
                const focusedTargetSession = focusedRun?.targets?.find((t) => sessions.some((s) => s.id === t.sessionId))?.sessionId;

                return (
                  <div className="auto-sundial-stage" aria-label="24 小时自动化星盘">
                    <section className="auto-sundial-dial-card">
                      <span className="general-coffee-kicker">24 小时任务星盘</span>
                      <svg viewBox="0 0 244 244" width="224" height="224" role="img" aria-label="24 小时任务刻度环">
                        <circle cx="122" cy="122" r="86" fill="none" stroke="var(--border-strong)" strokeWidth="1.5" />
                        <circle cx="122" cy="122" r="52" fill="none" stroke="var(--border)" strokeDasharray="3 4" />
                        {[0, 6, 12, 18].map((h) => {
                          const rad = (((h / 24) * 360 - 90) * Math.PI) / 180;
                          const x1 = 122 + 80 * Math.cos(rad);
                          const y1 = 122 + 80 * Math.sin(rad);
                          const x2 = 122 + 86 * Math.cos(rad);
                          const y2 = 122 + 86 * Math.sin(rad);
                          const tx = 122 + 104 * Math.cos(rad);
                          const ty = 122 + 104 * Math.sin(rad) + 3.5;
                          return (
                            <g key={h}>
                              <line x1={x1} y1={y1} x2={x2} y2={y2} stroke="var(--text-dim)" strokeWidth="1.4" />
                              <text x={tx} y={ty} textAnchor="middle" fill="var(--text-dim)" fontSize="10">{`${pad(h)}:00`}</text>
                            </g>
                          );
                        })}
                        <line x1="122" y1="122" x2={nowX} y2={nowY} stroke="var(--accent)" strokeWidth="2" strokeLinecap="round" opacity="0.65" />
                        <circle cx="122" cy="122" r="4.5" fill="var(--accent)" />
                        {filtered.map((item) => {
                          const eventIdx = Math.max(0, eventItems.findIndex((x) => x.id === item.id));
                          const pt = sundialStarCoords(item, eventIdx, eventItems.length);
                          const selected = focused?.id === item.id;
                          const fill = !item.enabled ? "var(--text-dim)" : pt.isEvent ? "#6aa6f8" : "var(--accent)";
                          return (
                            <g
                              key={item.id}
                              style={{ cursor: "pointer" }}
                              onClick={() => setSelectedHistory(item.id)}
                            >
                              <title>{`${item.name} · ${triggerText(item.trigger)}`}</title>
                              {selected && <circle cx={pt.x} cy={pt.y} r="13" fill="color-mix(in srgb, var(--accent) 18%, transparent)" />}
                              <circle cx={pt.x} cy={pt.y} r={selected ? 7.5 : 5.8} fill={fill} stroke="var(--bg-elev)" strokeWidth="2" />
                            </g>
                          );
                        })}
                      </svg>
                      <div className="auto-sundial-legend">
                        <span><i style={{ background: "var(--accent)" }} />外环定时</span>
                        <span><i style={{ background: "#6aa6f8" }} />内环事件</span>
                        <span><i style={{ background: "var(--text-dim)" }} />已暂停</span>
                      </div>
                    </section>

                    <section className="auto-sundial-detail-card">
                      {!focused ? (
                        <div style={{ display: "grid", gap: "10px", padding: "12px 0" }}>
                          <strong>{automations.length ? "此筛选下没有任务。" : "星盘还是空的"}</strong>
                          <p className="auto-sundial-sub">
                            {automations.length ? "切换到「全部」或调整筛选条件查看其他任务。" : "新建第一条定时或事件自动化，它会在星盘刻度上亮起一颗星点。"}
                          </p>
                          {!automations.length && (
                            <div>
                              <button type="button" className="btn automation-primary" disabled={busy} onClick={() => openCreate()}>
                                <PlusIcon size={13} weight="bold" /> 新建自动化
                              </button>
                            </div>
                          )}
                        </div>
                      ) : (
                        <>
                          <div className="auto-sundial-detail-head">
                            <div>
                              <h3>
                                <span>{focused.name}</span>
                                <span className="auto-sundial-time-chip">{triggerText(focused.trigger)}</span>
                              </h3>
                              <p className="auto-sundial-sub" title={actionText(focused.action)}>
                                {compactAction(focused.action)} · 下次：{nextRunText(focused)}
                              </p>
                            </div>
                            <Switch
                              className="automation-toggle"
                              label={`${focused.enabled ? "暂停后续计划" : "开启计划"} ${focused.name}`}
                              checked={focused.enabled}
                              disabled={busy}
                              onChange={() => void toggle(focused)}
                            />
                          </div>

                          <div className="auto-sundial-status-strip">
                            <div style={{ minWidth: 0 }}>
                              <div style={{ fontSize: "11.5px", color: "var(--text-dim)" }}>
                                当前状态 · <strong style={{ color: "var(--text)" }}>{displayStage(focusedStage)}</strong>
                              </div>
                              <div style={{ fontSize: "12.5px", marginTop: "2px" }} title={focusedRun?.detail}>
                                {focusedRun ? focusedRun.detail : "到点后自动运行，也可随时上发条手动试跑一次。"}
                              </div>
                            </div>
                            <div className="automation-card-buttons">
                              {focusedActive && focusedTargetSession && (
                                <button type="button" className="btn" onClick={() => openSession(focusedTargetSession)}>
                                  <ArrowSquareOut size={15} /> 打开会话
                                </button>
                              )}
                              {focusedActive && focusedRun?.id && focusedStage !== "unconfirmed" && (
                                <button type="button" className="btn" disabled={busy || focusedStage === "dispatching"} onClick={() => void runAction(focused, focusedRun, true)}>
                                  <Stop size={15} /> 停止本轮
                                </button>
                              )}
                              {!focusedActive && (
                                <button
                                  type="button"
                                  className="btn automation-primary"
                                  disabled={busy}
                                  onClick={() => void runAction(focused, focusedStage === "failed" || focusedStage === "cancelled" ? focusedRun : undefined)}
                                >
                                  {focusedStage === "failed" || focusedStage === "cancelled" ? <ArrowClockwise size={15} /> : <Play size={15} />}
                                  <span>{focusedStage === "failed" || focusedStage === "cancelled" ? "失败重试" : "上发条运行"}</span>
                                </button>
                              )}
                              <button type="button" className="btn" disabled={busy} onClick={() => void openEdit(focused)}>
                                <PencilSimpleIcon size={14} /> 编辑
                              </button>
                              <button
                                type="button"
                                className="btn"
                                disabled={busy || focusedActive}
                                aria-label={`删除 ${focused.name}`}
                                onClick={() => setConfirmDeleteId(focused.id)}
                              >
                                <TrashIcon size={14} />
                              </button>
                            </div>
                          </div>

                          {confirmDeleteId === focused.id && (
                            <div className="automation-delete-confirm">
                              <span>确认删除此计划及其记录？</span>
                              <button type="button" className="btn danger-solid" disabled={busy || focusedActive} onClick={() => void remove(focused.id)}>确认删除</button>
                              <button type="button" className="btn" onClick={() => setConfirmDeleteId(null)}>取消</button>
                            </div>
                          )}

                          <div className="auto-sundial-mini-grid">
                            {filtered.map((item) => (
                              <button
                                key={item.id}
                                type="button"
                                className={`auto-sundial-mini-btn${item.id === focused.id ? " active" : ""}`}
                                onClick={() => setSelectedHistory(item.id)}
                              >
                                <strong>
                                  <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{item.name}</span>
                                  <span style={{ color: item.enabled ? "var(--success)" : "var(--text-dim)" }}>●</span>
                                </strong>
                                <small>{triggerText(item.trigger)} · {compactAction(item.action)}</small>
                              </button>
                            ))}
                          </div>
                        </>
                      )}
                    </section>
                  </div>
                );
              })() : (
              <div className="automation-list">
                {!automations.some(a => matchesRunFilter(a, filter)) && <p className="automation-empty">{automations.length ? "此筛选下没有任务。" : "还没有自动化任务。"}</p>}
                {automations.filter(a => matchesRunFilter(a, filter)).map(item => {
                  const run = currentRun(item); const stage = runStage(run); const active = item.recentRuns.some(activeRun);
                  const Icon = actionIcon(item.action); const StateIcon = statusIcon(stage);
                  const targetSession = run?.targets?.find(t => sessions.some(s => s.id === t.sessionId))?.sessionId;
                  return <article key={item.id} className={`automation-card execution-${stage}${item.enabled ? "" : " plan-paused"}`} data-history-selected={selectedHistory === item.id} tabIndex={0} aria-label={`编辑定时任务 ${item.name}`} aria-disabled={busy} onClick={event => {
                    if (!(event.target instanceof Element) || event.target.closest("button, a, input, select, textarea, label, details")) return;
                    if (window.getSelection()?.toString()) return;
                    void openEdit(item);
                  }} onKeyDown={event => { if (event.target === event.currentTarget && (event.key === "Enter" || event.key === " ")) { event.preventDefault(); void openEdit(item); } }}>
                    <header><div className="automation-card-title"><span className="automation-task-icon"><Icon size={25} weight="regular" aria-hidden="true" /></span><div><h3><button type="button" title="编辑定时任务" disabled={busy} onClick={() => void openEdit(item)}>{item.name}</button></h3><p title={actionText(item.action)}>{compactAction(item.action)}</p></div></div>
                      <Switch className="automation-toggle" label={`${item.enabled ? "暂停后续计划" : "开启计划"} ${item.name}`} checked={item.enabled} disabled={busy} onChange={() => void toggle(item)} childrenPosition="before"><span title={planState(item)}>{planState(item)}</span></Switch>
                    </header>
                    <p className="automation-card-schedule">{item.enabled ? <CalendarBlank size={17} aria-hidden="true" /> : <PauseCircle size={17} aria-hidden="true" />}<span>{item.enabled ? `计划：${triggerText(item.trigger)} · 下次 ${nextRunText(item)}` : "已暂停：暂不触发后续计划"}</span></p>
                    <div className={`automation-run-status tone-${stage}`}><StateIcon className="automation-state-icon" size={21} aria-hidden="true" /><span className={`automation-status status-${stage}`}>{displayStage(stage)}</span><span className="automation-run-description" title={run?.detail}>{run?.id && <span className="automation-execution-id" title={run.id}>执行 {shortId(run)} · </span>}{run ? run.detail : "到点后执行，也可手动运行一次"}</span></div>
                    <footer><div className="automation-card-buttons">
                      {!item.enabled && stage === "unconfirmed" && <button className="btn" disabled={busy} onClick={() => void toggle(item)}><Play size={16} />开启计划</button>}
                      {active && targetSession && <button className={`btn${stage === "running" ? " automation-primary" : ""}`} onClick={() => openSession(targetSession)}><ArrowSquareOut size={16} />打开会话</button>}
                      {active && run?.id && stage !== "unconfirmed" && <button className="btn" disabled={busy || stage === "dispatching"} onClick={() => void runAction(item, run, true)}><Stop size={16} />停止本轮</button>}
                      {stage === "failed" && <button className="btn" aria-label={`执行记录 ${item.recentRuns.length}`} aria-expanded={selectedHistory === item.id} onClick={() => viewHistory(item)}><FileTextIcon size={16} />查看原因</button>}
                      {!active && <button className="btn automation-primary" disabled={busy} onClick={() => void runAction(item, stage === "failed" || stage === "cancelled" ? run : undefined)}>{stage === "failed" || stage === "cancelled" ? <ArrowClockwise size={16} /> : <Play size={16} />}{stage === "failed" || stage === "cancelled" ? "失败重试" : "立即执行"}</button>}
                      {stage !== "failed" && <button className={`btn automation-record-button${active ? " compact" : ""}`} aria-label={`执行记录 ${item.recentRuns.length}`} aria-expanded={selectedHistory === item.id} title="执行记录" onClick={() => viewHistory(item)}><FileTextIcon size={16} />{!active && "记录"}</button>}
                    </div>
                    <details className="automation-card-menu" onKeyDown={event => {if(event.key === "Escape"){event.preventDefault();event.stopPropagation();event.currentTarget.open=false;event.currentTarget.querySelector("summary")?.focus();}}}>
                      <summary aria-label={`${item.name} 更多操作`} title="更多操作"><DotsThree size={20} /></summary>
                      <div onClick={event => { const details=event.currentTarget.closest("details"); if(details)details.open=false; }}>
                        <p>计划触发 {item.runCount}{item.maxRuns ? `/${item.maxRuns}` : ""} 次{item.consecutiveFailures > 0 && ` · 连续失败 ${item.consecutiveFailures} 次`}</p>
                        <button className="btn" disabled={busy} onClick={() => void openEdit(item)}><PencilSimpleIcon size={15} aria-hidden="true" />编辑定时任务</button>
                        {targetSession && <button className="btn" onClick={() => openSession(targetSession)}><ArrowSquareOut size={15} />打开原会话</button>}
                        <button className="btn" onClick={() => setSelectedHistory(item.id)}><FileTextIcon size={15} />查看全部执行记录</button>
                        {stage === "unconfirmed" && run?.id && <><button className="btn" disabled={busy} onClick={() => void runAction(item,run,true)}><Stop size={15} />停止本轮</button><button className="btn" disabled={busy} onClick={() => setConfirmUnknownId(run.id!)}><CheckSquareIcon size={15} aria-hidden="true" />已核对原会话</button></>}
                        <button className="btn" disabled={busy || active} title={active ? "先核对或停止本轮" : "删除计划"} aria-label={`删除 ${item.name}`} onClick={() => setConfirmDeleteId(item.id)}><TrashIcon size={15} />删除计划</button>
                      </div>
                    </details></footer>
                    {confirmUnknownId === run?.id && <div className="automation-state-note" role="group" aria-label="确认核对"><p>请先检查原会话，确认原任务不再执行。此操作仅解除待核对状态，不代表成功，不会重发任务。</p><button className="btn" disabled={busy} onClick={() => void runAction(item,run,false,true)}>确认原任务不再执行</button><button className="btn" onClick={() => setConfirmUnknownId(null)}>取消</button></div>}
                    {confirmDeleteId === item.id && <div className="automation-delete-confirm"><span>确认删除此计划及其记录？</span><button className="btn danger-solid" disabled={busy || active} onClick={() => void remove(item.id)}>确认删除</button><button className="btn" onClick={() => setConfirmDeleteId(null)}>取消</button></div>}
                  </article>;
                })}
              </div>
              )}
              {!selectedHistory && <p className="automation-state-note"><Info size={16} aria-hidden="true" />暂停计划不停止当前执行；已入队不代表已完成。打开执行记录核对实际结果。</p>}
              {automations.filter(a => a.id === selectedHistory).map(item => <section className="automation-execution-history" key={item.id} aria-label={`执行记录 ${item.name}`}>
                <header><h3>执行记录 · {item.name}</h3><p><Info size={16} aria-hidden="true" />暂停计划不停止当前执行；已入队不代表已完成。</p></header>
                {!item.recentRuns.length && <p>尚无执行记录。</p>}
                {[...item.recentRuns].reverse().map((run,index) => {
                  const stage=runStage(run), StateIcon=statusIcon(stage);
                  const target=run.targets?.find(t => sessions.some(s => s.id === t.sessionId));
                  return <details key={run.id ?? index}><summary><span className="automation-history-id" title={run.id}>{shortId(run)}</span><time>{formatDateTime(run.at)}</time><span className={`automation-history-stage tone-${stage}`}><StateIcon size={17} aria-hidden="true" />{displayStage(stage)}</span><span className="automation-history-origin">{({manual:"手动执行",retry:"重试",event:"事件触发",schedule:"计划触发"} as Record<string,string>)[run.origin ?? ""] ?? "历史记录"}</span><span className="automation-history-detail" title={run.detail}>{run.detail}</span>{target && <button className="automation-history-session" aria-label={`打开执行 ${shortId(run)} 的会话`} onClick={event => {event.preventDefault();event.stopPropagation();openSession(target.sessionId);}}><ArrowSquareOut size={15} />打开会话</button>}</summary><p>执行编号：{run.id ?? "旧记录未提供"}{run.retryOf && ` · 重试自 ${run.retryOf}`}</p><p>{run.detail}</p>{run.targets?.map(target => <div className="automation-target-receipt" key={target.messageId}><strong>{displayStage(target.stage)}</strong><span>{target.detail}</span><code>消息 {target.messageId}</code><code>任务 {target.taskId ?? "无项目任务"} · Run {target.runId ?? "未取得"}</code>{sessions.some(s => s.id === target.sessionId) && <button className="btn" onClick={() => openSession(target.sessionId)}>打开会话</button>}</div>)}</details>;
                })}
              </section>)}
            </section>}
          </div>

          {formOpen && (
            <div className={`automation-form-actions${error ? " has-error" : ""}`}>{error && <p className="automation-error" role="alert">{error}</p>}<span className="automation-execution-summary">{AUTOMATION_LABELS[form.runtime as AgentRuntimeKind] ?? "未选择 Agent"} · {form.mode === "new" ? "本地配置" : "沿用目标配置"} · {({ new: "新建会话", session: "指定会话", member: "项目成员", broadcast: "此 Agent 全部运行会话" })[form.mode]} · {triggerKind === "daily" ? `每天 ${dailyTime}` : triggerKind === "interval" ? `每 ${intervalMinutes} 分钟` : triggerKind === "weekly" ? `每周 ${weeklyTime}` : triggerKind === "cron" ? "cron 触发" : triggerKind === "once" ? "一次性触发" : "指定会话事件"} · {overlap === "skip" ? "未结束则跳过" : "允许继续派发"}</span>
              <button className="btn" disabled={busy} onClick={() => setFormOpen(false)}>取消</button>
              <button className="btn automation-primary" disabled={busy} onClick={() => void create()}>
                {editing ? "保存修改" : "保存自动化"}
              </button>
            </div>
          )}
        </div>
      </section>
  );

  return embedded ? panel : (
    <div className="dialog-overlay" onClick={() => { if (!mutationInFlight.current) onClose?.(); }}>
      {panel}
    </div>
  );
}
