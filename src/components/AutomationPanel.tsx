import { activeRun, currentRun, matchesRunFilter, planState, runStage, RUN_LABELS, type RunFilter } from "../automationExecution";
import "../styles/automation-execution.css";
import { buildAutomationAction, eventAutomationTrigger, automationSessions, automationProjectRuntime, AUTOMATION_LABELS, type AutomationFormAction } from "../automationForm";
import { TERMINAL_RUNTIMES } from "../runtimeTaxonomy";
import { open as openDirectory } from "@tauri-apps/plugin-dialog";
import { FolderOpenIcon } from "@phosphor-icons/react/dist/csr/FolderOpen";
import { StackIcon } from "@phosphor-icons/react/dist/csr/Stack";
import { useRuntimeAvailability } from "../runtimeAvailability";
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Switch } from "./ui/Switch";
import { ClockIcon } from "@phosphor-icons/react/dist/csr/Clock";
import { FileTextIcon } from "@phosphor-icons/react/dist/csr/FileText";
import { GearSixIcon } from "@phosphor-icons/react/dist/csr/GearSix";
import { LightningIcon } from "@phosphor-icons/react/dist/csr/Lightning";
import { PlusIcon } from "@phosphor-icons/react/dist/csr/Plus";
import { TrashIcon } from "@phosphor-icons/react/dist/csr/Trash";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import type {
  Automation,
  AutomationAction,
  AutomationRun,
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
  onFormOpenChange?: (open: boolean) => void;
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

export function AutomationPanel({ sessions, providers, onClose, embedded = false, onFormOpenChange }: Props) {
  const { ready: availabilityReady, error: availabilityError, prefs: runtimePreferences, isRuntimeEnabled } = useRuntimeAvailability();
  const [automations, setAutomations] = useState<Automation[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  useEffect(() => {
    onFormOpenChange?.(formOpen);
    return () => onFormOpenChange?.(false);
  }, [formOpen, onFormOpenChange]);
  const [busy, setBusy] = useState(false);
  const mutationInFlight = useRef(false);
  const listSequence = useRef(0);
  const [filter, setFilter] = useState<RunFilter>("all");
  const [selectedHistory, setSelectedHistory] = useState<string | null>(null);
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
  const [eventSource, setEventSource] = useState<"session" | "runtime" | "project">("session");
  const [eventSourceId, setEventSourceId] = useState("");
  const [eventRuntime, setEventRuntime] = useState<AgentRuntimeKind | "">("");
  const [targetProjects, setTargetProjects] = useState<MultiAgentProject[]>([]);
  const [targetAgents, setTargetAgents] = useState<AgentProfile[]>([]);
  const [probes, setProbes] = useState<RuntimeProbe[]>([]);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [probeLoading, setProbeLoading] = useState(false);
  const [probeRevision, setProbeRevision] = useState(0);
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
    try { return buildAutomationAction(form, isRuntimeEnabled, providers, sessions, targetAgents); }
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
      await invoke<Automation>("create_automation", { input });
      setFormOpen(false);
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

  const panel = (
      <section
        className={`automation-dialog${embedded ? " embedded" : ""}`}
        role={embedded ? undefined : "dialog"}
        aria-modal={embedded ? undefined : "true"}
        aria-labelledby="automation-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="automation-content">
          <div className="automation-heading">
            <div className="automation-heading-copy">
              <div className="automation-title-row">
                <GearSixIcon size={24} weight="regular" aria-hidden="true" />
                <h2 id="automation-title">{formOpen ? "创建自动化" : "自动化"}</h2>
              </div>
              <p>选择 Agent 与执行任务，按时间或指定事件派发；记录保存在本机。</p>
            </div>
            <div className="settings-heading-actions">
              <button className="btn-icon" aria-label="关闭" title="关闭" disabled={busy} onClick={onClose}>
                <XIcon size={15} />
              </button>
              <button className="btn automation-primary" disabled={busy} onClick={() => setFormOpen((open) => !open)}>
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
                    <label>事件来源<select aria-label="事件来源" value={eventSource} onChange={e => { setEventSource(e.target.value as typeof eventSource); setEventSourceId(""); }}><option value="session">指定会话</option><option value="runtime">指定 Agent 类型</option><option value="project">指定项目</option></select></label>
                    {eventSource === "runtime" && <label>来源 Agent<select aria-label="来源 Agent" value={eventRuntime} onChange={e => setEventRuntime(e.target.value as AgentRuntimeKind)}><option value="">请选择来源 Agent</option>{runtimeOptions.map(([kind, label]) => <option key={kind} value={kind}>{label}</option>)}</select></label>}
                    {eventSource !== "runtime" && <label>来源目标<select aria-label="来源目标" value={eventSourceId} onChange={e => setEventSourceId(e.target.value)}><option value="">请选择具体来源</option>{eventSource === "session" ? sessions.filter(s => !s.retiredAgentRuntime && !s.archivedAt && (s.agentRuntimeKind || s.kind === "claude")).map(s => <option key={s.id} value={s.id}>{s.title}</option>) : targetProjects.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</select></label>}
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
                <button className="automation-refresh" disabled={busy} onClick={() => void refresh()}>刷新</button>
              </nav>
              <div className="automation-list">
                {!automations.some(a => matchesRunFilter(a, filter)) && <p className="automation-empty">{automations.length ? "此筛选下没有任务。" : "还没有自动化任务。"}</p>}
                {automations.filter(a => matchesRunFilter(a, filter)).map(item => {
                  const run = currentRun(item); const stage = runStage(run); const active = item.recentRuns.some(activeRun);
                  return <article key={item.id} className={`automation-card execution-${stage}${item.enabled ? "" : " plan-paused"}`}>
                    <header><div className="automation-card-title"><ClockIcon size={21} /><div><h3>{item.name}</h3><p title={actionText(item.action)}>{actionText(item.action)}</p></div></div>
                      <Switch className="automation-toggle" label={`${item.enabled ? "暂停后续计划" : "开启计划"} ${item.name}`} checked={item.enabled} disabled={busy} onChange={() => void toggle(item)}><span>{planState(item)}</span></Switch>
                    </header>
                    <p className="automation-card-schedule"><ClockIcon size={15} />{triggerText(item.trigger)} · 下次：{nextRunText(item)}</p>
                    <div className="automation-run-status"><span className={`automation-status status-${stage}`}>{RUN_LABELS[stage] ?? "结果未确认"}</span><span>{run ? run.detail : "到点后执行，也可手动运行一次"}</span></div>
                    <footer><div className="automation-card-buttons">
                      {run?.targets?.some(t => sessions.some(s => s.id === t.sessionId)) && <button className="btn" onClick={() => openSession(run.targets!.find(t => sessions.some(s => s.id === t.sessionId))!.sessionId)}>打开会话</button>}
                      {active && run?.id && <button className="btn" disabled={busy || stage === "dispatching"} onClick={() => void runAction(item, run, true)}>停止本轮</button>}
                      {stage === "unconfirmed" && run?.id && <button className="btn" disabled={busy} onClick={() => setConfirmUnknownId(run.id!)}>已核对原会话</button>}
                      {!active && <button className="btn automation-primary" disabled={busy} onClick={() => void runAction(item, stage === "failed" || stage === "cancelled" ? run : undefined)}>{stage === "failed" || stage === "cancelled" ? "失败重试" : "立即执行"}</button>}
                      <button className="btn" aria-expanded={selectedHistory === item.id} onClick={() => setSelectedHistory(selectedHistory === item.id ? null : item.id)}>执行记录 {item.recentRuns.length}</button>
                    </div><span>计划触发 {item.runCount}{item.maxRuns ? `/${item.maxRuns}` : ""} 次</span></footer>
                    {confirmUnknownId === run?.id && <div className="automation-state-note" role="group" aria-label="确认核对"><p>请先检查原会话，确认原任务不再执行。此操作仅解除待核对状态，不代表成功，不会重发任务。</p><button className="btn" disabled={busy} onClick={() => void runAction(item,run,false,true)}>确认原任务不再执行</button><button className="btn" onClick={() => setConfirmUnknownId(null)}>取消</button></div>}
                    <div className="automation-card-secondary">{item.consecutiveFailures > 0 && <span>连续失败 {item.consecutiveFailures} 次</span>}<button className="btn-icon" disabled={busy || active} title={active ? "先核对或停止本轮" : "删除计划"} aria-label={`删除 ${item.name}`} onClick={() => setConfirmDeleteId(item.id)}><TrashIcon size={14} /></button>{confirmDeleteId === item.id && <><button className="btn danger-solid" disabled={busy} onClick={() => void remove(item.id)}>确认删除</button><button className="btn" onClick={() => setConfirmDeleteId(null)}>取消</button></>}</div>
                  </article>;
                })}
              </div>
              <p className="automation-state-note">暂停计划只影响后续触发。已入队不代表已完成；停止本轮仅在能核对具体消息时执行，已进入共享会话的任务需在原会话停止。</p>
              {automations.filter(a => a.id === selectedHistory).map(item => <section className="automation-execution-history" key={item.id} aria-label={`执行记录 ${item.name}`}><h3>执行记录 · {item.name}</h3>{!item.recentRuns.length && <p>尚无执行记录。</p>}{[...item.recentRuns].reverse().map((run, index) => <details key={run.id ?? index}><summary><span className={`automation-status status-${runStage(run)}`}>{RUN_LABELS[runStage(run)] ?? "结果未确认"}</span><time>{formatDateTime(run.at)}</time><span>{({manual:"手动执行",retry:"重试",event:"事件触发",schedule:"计划触发"} as Record<string,string>)[run.origin ?? ""] ?? "历史记录"}</span><span>{run.detail}</span></summary><p>执行编号：{run.id ?? "旧记录未提供"}{run.retryOf && ` · 重试自 ${run.retryOf}`}</p>{run.targets?.map(target => <div className="automation-target-receipt" key={target.messageId}><strong>{RUN_LABELS[target.stage] ?? "结果未确认"}</strong><span>{target.detail}</span><code>消息 {target.messageId}</code><code>任务 {target.taskId ?? "无项目任务"} · Run {target.runId ?? "未取得"}</code>{sessions.some(s => s.id === target.sessionId) && <button className="btn" onClick={() => openSession(target.sessionId)}>打开会话</button>}</div>)}</details>)}</section>)}
            </section>}
          </div>

          {formOpen && (
            <div className={`automation-form-actions${error ? " has-error" : ""}`}>{error && <p className="automation-error" role="alert">{error}</p>}<span className="automation-execution-summary">{AUTOMATION_LABELS[form.runtime as AgentRuntimeKind] ?? "未选择 Agent"} · {form.mode === "new" ? "本地配置" : "沿用目标配置"} · {({ new: "新建会话", session: "指定会话", member: "项目成员", broadcast: "此 Agent 全部运行会话" })[form.mode]} · {triggerKind === "daily" ? `每天 ${dailyTime}` : triggerKind === "interval" ? `每 ${intervalMinutes} 分钟` : triggerKind === "weekly" ? `每周 ${weeklyTime}` : triggerKind === "cron" ? "cron 触发" : triggerKind === "once" ? "一次性触发" : "指定会话事件"} · {overlap === "skip" ? "未结束则跳过" : "允许继续派发"}</span>
              <button className="btn" disabled={busy} onClick={() => setFormOpen(false)}>取消</button>
              <button className="btn automation-primary" disabled={busy} onClick={() => void create()}>
                保存自动化
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
