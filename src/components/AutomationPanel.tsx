import { useRuntimeAvailability } from "../runtimeAvailability";
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Switch } from "./ui/Switch";
import { ClockIcon } from "@phosphor-icons/react/dist/csr/Clock";
import { FileTextIcon } from "@phosphor-icons/react/dist/csr/FileText";
import { GearSixIcon } from "@phosphor-icons/react/dist/csr/GearSix";
import { LightningIcon } from "@phosphor-icons/react/dist/csr/Lightning";
import { ListBulletsIcon } from "@phosphor-icons/react/dist/csr/ListBullets";
import { PlusIcon } from "@phosphor-icons/react/dist/csr/Plus";
import { TrashIcon } from "@phosphor-icons/react/dist/csr/Trash";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import type {
  Automation,
  AutomationAction,
  AutomationTrigger,
  NewAutomation,
  ProviderConfig,
  SessionRecord,
  AgentProfile,
  MultiAgentProject,
} from "../types";

interface Props {
  sessions: SessionRecord[];
  providers: ProviderConfig[];
  onClose?: () => void;
  embedded?: boolean;
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

export function AutomationPanel({ sessions, providers, onClose, embedded = false }: Props) {
  const { ready: availabilityReady, error: availabilityError, prefs: runtimePreferences, isRuntimeEnabled } = useRuntimeAvailability();
  const [automations, setAutomations] = useState<Automation[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const mutationInFlight = useRef(false);
  const listSequence = useRef(0);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);

  const [name, setName] = useState("");
  // "on_completed"/"on_failed" 是表单层合成值，buildTrigger 转成 on_session_event。
  const [triggerKind, setTriggerKind] = useState<
    AutomationTrigger["kind"] | "on_completed" | "on_failed"
  >("interval");
  const [onceAt, setOnceAt] = useState("");
  const [intervalMinutes, setIntervalMinutes] = useState("30");
  const [dailyTime, setDailyTime] = useState("09:00");
  const [weekdays, setWeekdays] = useState<number[]>([1, 2, 3, 4, 5]);
  const [weeklyTime, setWeeklyTime] = useState("09:00");
  const [cronExpr, setCronExpr] = useState("");
  const [cronPreview, setCronPreview] = useState<
    { state: "idle" } | { state: "ok"; runs: number[] } | { state: "error"; message: string }
  >({ state: "idle" });
  const [actionKind, setActionKind] = useState<AutomationAction["kind"]>("message");
  const [launchProviderId, setLaunchProviderId] = useState("");
  const [launchCwd, setLaunchCwd] = useState("");
  const [launchPrompt, setLaunchPrompt] = useState("");
  const [launchTargetMode, setLaunchTargetMode] = useState<"provider" | "agent">("provider");
  const [messageSessionId, setMessageSessionId] = useState("");
  const [messageTargetMode, setMessageTargetMode] = useState<"session" | "agent">("session");
  const [messageBody, setMessageBody] = useState("");
  const [broadcastBody, setBroadcastBody] = useState("");
  const [maxRuns, setMaxRuns] = useState("");
  const [targetProjects, setTargetProjects] = useState<MultiAgentProject[]>([]);
  const [targetProjectId, setTargetProjectId] = useState("");
  const [targetAgents, setTargetAgents] = useState<AgentProfile[]>([]);
  const [targetAgentId, setTargetAgentId] = useState("");

  const claudeSessions = sessions.filter((session) => session.kind === "claude" && isRuntimeEnabled(session.agentRuntimeKind ?? "claude_code"));

  useEffect(() => {
    if (!formOpen) return;
    let disposed = false;
    void invoke<MultiAgentProject[]>("ma_list_projects")
      .then((projects) => {
        if (disposed) return;
        setTargetProjects(projects);
        setTargetProjectId((current) => current && projects.some((project) => project.id === current) ? current : projects[0]?.id ?? "");
      })
      .catch((reason) => { if (!disposed) setError(String(reason)); });
    return () => { disposed = true; };
  }, [formOpen]);

  useEffect(() => {
    let disposed = false;
    setTargetAgents([]);
    if (!targetProjectId) {
      setTargetAgents([]);
      setTargetAgentId("");
      return;
    }
    void invoke<{ agents: AgentProfile[] }>("ma_snapshot", { projectId: targetProjectId })
      .then((snapshot) => {
        if (disposed) return;
        const enabled = snapshot.agents.filter((agent) => agent.enabled && isRuntimeEnabled(agent.runtimeKind));
        setTargetAgents(enabled);
        setTargetAgentId((current) => current && enabled.some((agent) => agent.id === current) ? current : enabled[0]?.id ?? "");
      })
      .catch((reason) => { if (!disposed) setError(String(reason)); });
    return () => { disposed = true; };
  }, [targetProjectId, availabilityReady, runtimePreferences, isRuntimeEnabled]);

  const refresh = useCallback(async () => {
    const sequence = ++listSequence.current;
    try {
      const latest = await invoke<Automation[]>("list_automations");
      if (sequence === listSequence.current) setAutomations(latest);
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
      return {
        kind: "on_session_event",
        event: triggerKind === "on_completed" ? "completed" : "failed",
      };
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
    const usesAgent = actionKind === "launch" ? launchTargetMode === "agent" : actionKind === "message" && messageTargetMode === "agent";
    if (usesAgent && !targetAgents.some((agent) => agent.id === targetAgentId && isRuntimeEnabled(agent.runtimeKind))) return "请选择已启用的 Agent";
    if (actionKind === "launch" && !usesAgent && !isRuntimeEnabled("claude_code")) return "Claude Code 未启用";
    if (actionKind === "message" && !usesAgent && messageSessionId && !claudeSessions.some((session) => session.id === messageSessionId)) return "目标会话 Runtime 未启用";
    if (actionKind === "launch") {
      if (launchTargetMode === "agent") {
        if (!targetProjectId || !targetAgentId) return "请选择目标项目和 Agent";
        if (!launchPrompt.trim()) return "请填写启动 Prompt";
        return {
          kind: "launch",
          providerId: "",
          cwd: null,
          prompt: launchPrompt,
          projectId: targetProjectId,
          agentId: targetAgentId,
        };
      }
      const providerId = launchProviderId || providers[0]?.id || "";
      if (!providerId) return "没有可用的 Provider";
      if (!launchPrompt.trim()) return "请填写启动 Prompt";
      return {
        kind: "launch",
        providerId,
        cwd: launchCwd.trim() || null,
        prompt: launchPrompt,
        projectId: null,
        agentId: null,
      };
    }
    if (actionKind === "broadcast") {
      if (!broadcastBody.trim()) return "请填写广播内容";
      return { kind: "broadcast", body: broadcastBody };
    }
    if (messageTargetMode === "agent") {
      if (!targetProjectId || !targetAgentId) return "请选择目标项目和 Agent";
      if (!messageBody.trim()) return "请填写消息内容";
      return { kind: "message", sessionId: "", body: messageBody, projectId: targetProjectId, agentId: targetAgentId };
    }
    const sessionId = messageSessionId || claudeSessions[0]?.id || "";
    if (!sessionId) return "没有可发送的会话";
    if (!messageBody.trim()) return "请填写消息内容";
    return { kind: "message", sessionId, body: messageBody, projectId: null, agentId: null };
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
    const input: NewAutomation = {
      name: name.trim(),
      trigger,
      action,
      maxRuns: runs,
      maxConsecutiveFailures: null,
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
    if (trigger.kind === "on_session_event") {
      return trigger.event === "completed" ? "会话完成时" : "会话失败时";
    }
    return `每 ${trigger.minutes} 分钟`;
  };

  const actionText = (action: AutomationAction): string => {
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
    if (automation.trigger.kind === "on_session_event") return "等待事件";
    return "已完成";
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
                <h2 id="automation-title">自动化</h2>
              </div>
              <p>定时给会话派任务：起新会话、发给现有会话或广播。全部执行记录可审计。</p>
            </div>
            <div className="settings-heading-actions">
              <button className="btn-icon" aria-label="关闭" title="关闭" disabled={busy} onClick={onClose}>
                <XIcon size={15} />
              </button>
              <button className="btn automation-primary" disabled={busy} onClick={() => setFormOpen((open) => !open)}>
                <PlusIcon size={13} weight="bold" /> 新建自动化
              </button>
            </div>
          </div>

          <div className="automation-scroll-area">
            {error && <p className="automation-error" role="alert">{error}</p>}

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

                <section className="automation-section">
                  <h3><ClockIcon size={18} aria-hidden="true" />触发条件</h3>
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
                    任一会话{triggerKind === "on_completed" ? "执行完成" : "失败"}时触发；一批事件合并为一次
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
                </section>

                <section className="automation-section">
                  <h3><LightningIcon size={18} aria-hidden="true" />执行动作</h3>
              <div className="automation-form-row">
                <label>
                  动作
                  <select
                    value={actionKind}
                    onChange={(event) => setActionKind(event.target.value as AutomationAction["kind"])}
                  >
                    <option value="message">发给现有会话</option>
                    <option value="launch">启动新会话</option>
                    <option value="broadcast">广播全部会话</option>
                  </select>
                </label>
                {actionKind === "launch" && (
                  <>
                    <label>
                      运行目标
                      <select value={launchTargetMode} onChange={(event) => setLaunchTargetMode(event.target.value as typeof launchTargetMode)}>
                        <option value="provider">新建独立会话</option>
                        <option value="agent">项目 Agent</option>
                      </select>
                    </label>
                    {launchTargetMode === "provider" ? <>
                      <label>
                        Provider
                        <select
                          value={launchProviderId || providers[0]?.id || ""}
                          onChange={(event) => setLaunchProviderId(event.target.value)}
                        >
                          {providers.map((provider) => (
                            <option key={provider.id} value={provider.id}>{provider.name}</option>
                          ))}
                        </select>
                      </label>
                      <label>
                        工作目录（留空用主目录）
                        <input
                          value={launchCwd}
                          placeholder="F:/project/xxx"
                          onChange={(event) => setLaunchCwd(event.target.value)}
                        />
                      </label>
                    </> : <>
                      <label>
                        目标项目
                        <select value={targetProjectId} onChange={(event) => setTargetProjectId(event.target.value)}>
                          {targetProjects.map((project) => <option key={project.id} value={project.id}>{project.name}</option>)}
                        </select>
                      </label>
                      <label>
                        目标 Agent
                        <select value={targetAgentId} disabled={!targetAgents.length} onChange={(event) => setTargetAgentId(event.target.value)}>
                          {targetAgents.filter((agent) => isRuntimeEnabled(agent.runtimeKind)).map((agent) => <option key={agent.id} value={agent.id}>{agent.name}（{agent.runtimeKind}）</option>)}
                        </select>
                      </label>
                    </>}
                  </>
                )}
                {actionKind === "message" && (
                  <>
                    <label>
                      目标类型
                      <select value={messageTargetMode} onChange={(event) => setMessageTargetMode(event.target.value as typeof messageTargetMode)}>
                        <option value="session">现有会话</option>
                        <option value="agent">项目 Agent</option>
                      </select>
                    </label>
                    {messageTargetMode === "session" ? <label>
                      目标会话
                      <select
                        value={messageSessionId || claudeSessions[0]?.id || ""}
                        onChange={(event) => setMessageSessionId(event.target.value)}
                      >
                        {claudeSessions.map((session) => <option key={session.id} value={session.id}>{session.title}（{session.providerName}）</option>)}
                      </select>
                    </label> : <>
                      <label>
                        目标项目
                        <select value={targetProjectId} onChange={(event) => setTargetProjectId(event.target.value)}>
                          {targetProjects.map((project) => <option key={project.id} value={project.id}>{project.name}</option>)}
                        </select>
                      </label>
                      <label>
                        目标 Agent
                        <select value={targetAgentId} disabled={!targetAgents.length} onChange={(event) => setTargetAgentId(event.target.value)}>
                          {targetAgents.filter((agent) => isRuntimeEnabled(agent.runtimeKind)).map((agent) => <option key={agent.id} value={agent.id}>{agent.name}（{agent.runtimeKind}）</option>)}
                        </select>
                      </label>
                    </>}
                  </>
                )}
              </div>
              {actionKind !== "message" && actionKind !== "broadcast" ? (
                <label>
                  启动 Prompt
                  <textarea
                    rows={3}
                    value={launchPrompt}
                    placeholder="新会话启动后要执行的任务……"
                    onChange={(event) => setLaunchPrompt(event.target.value)}
                  />
                </label>
              ) : (
                <label>
                  消息内容
                  <textarea
                    rows={3}
                    value={actionKind === "broadcast" ? broadcastBody : messageBody}
                    placeholder="到点注入会话终端的消息（自动提交）……"
                    onChange={(event) =>
                      actionKind === "broadcast"
                        ? setBroadcastBody(event.target.value)
                        : setMessageBody(event.target.value)
                    }
                  />
                </label>
              )}
              <div className="automation-form-row">
                <label>
                  最大触发次数（留空不限）
                  <input
                    type="number"
                    min={1}
                    value={maxRuns}
                    placeholder="不限"
                    onChange={(event) => setMaxRuns(event.target.value)}
                  />
                </label>
              </div>
                </section>
              </fieldset>
            )}

            <section className="automation-list-section">
              <h3><ListBulletsIcon size={18} aria-hidden="true" />自动化任务</h3>
              <div className="automation-list">
                {automations.length === 0 && (
                  <p className="automation-empty">还没有自动化任务。</p>
                )}
                {automations.map((automation) => (
              <article
                key={automation.id}
                className={`automation-card${automation.enabled ? "" : " disabled"}`}
              >
                <header>
                  <Switch
                    className="automation-toggle"
                    checked={automation.enabled}
                    disabled={busy}
                    onChange={() => void toggle(automation)}
                    withKnob={false}
                  >
                    <strong>{automation.name}</strong>
                  </Switch>
                  {confirmDeleteId === automation.id ? (
                    <button
                      className="btn danger-solid automation-delete"
                      disabled={busy}
                      onClick={() => void remove(automation.id)}
                    >
                      确认删除
                    </button>
                  ) : (
                    <button
                      className="btn-icon automation-delete"
                      disabled={busy}
                      aria-label={`删除 ${automation.name}`}
                      title="删除"
                      onClick={() => setConfirmDeleteId(automation.id)}
                    >
                      <TrashIcon size={15} />
                    </button>
                  )}
                </header>
                <p className="automation-card-line">{triggerText(automation.trigger)}</p>
                <p className="automation-card-line">{actionText(automation.action)}</p>
                <p className="automation-card-meta">
                  下次执行：{nextRunText(automation)} · 已运行 {automation.runCount}
                  {automation.maxRuns ? `/${automation.maxRuns}` : ""} 次
                  {automation.consecutiveFailures > 0 &&
                    ` · 连续失败 ${automation.consecutiveFailures}`}
                </p>
                {automation.recentRuns.length > 0 && (
                  <details className="automation-runs">
                    <summary>执行记录（{automation.recentRuns.length}）</summary>
                    <ul>
                      {[...automation.recentRuns].reverse().slice(0, 5).map((run, index) => (
                        <li key={index} className={run.ok ? "ok" : "fail"}>
                          <span>{run.ok ? "✓" : "✗"}</span>
                          <time>{formatDateTime(run.at)}</time>
                          <span>{run.detail}</span>
                        </li>
                      ))}
                    </ul>
                  </details>
                )}
              </article>
                ))}
              </div>
            </section>
          </div>

          {formOpen && (
            <div className="automation-form-actions">
              <button className="btn" disabled={busy} onClick={() => setFormOpen(false)}>取消</button>
              <button className="btn automation-primary" disabled={busy} onClick={() => void create()}>
                创建
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
