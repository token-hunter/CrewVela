import { UnverifiedRunNotice } from "./UnverifiedRunNotice";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { COLLABORATION_UI_PREVIEW_SNAPSHOT } from "./CollaborationSettingsPanel";
import { workspaceParticipation } from "../multiAgentWorkspacePresentation";
import { MultiTerminal } from "./MultiTerminal";
import type {
  AppTheme,
  AgentControlReceipt,
  AgentSessionRecord,
  DeliveryRecord,
  MultiAgentSnapshot,
  MultiAgentActivityEvent,
  RuntimeEventRecord,
  SessionRecord,
} from "../types";
import { AgentAvatar } from "./AgentAvatar";
import { RUNTIME_LABELS } from "../runtimeLogos";
import { CommunicationGuideButton } from "./CommunicationGuide";

const UI_PREVIEW = new URLSearchParams(window.location.search).get("ui-preview") === "1";

interface Props {
  projectId: string;
  sessions: SessionRecord[];
  theme: AppTheme;
  terminalTransparent?: boolean;
  onSessionsChange: (sessions: SessionRecord[]) => void;
  onCloseSession: (id: string) => void;
  onClose: () => void;
}

const STATUS_LABELS: Record<string, string> = {
  idle: "待命",
  starting: "启动中",
  running: "执行中",
  waiting: "等待输入",
  completed: "已完成",
  failed: "失败",
  stopped: "已停止",
  draft: "草稿",
  ready: "可执行",
  blocked: "等待依赖",
  claimed: "已领取",
  waiting_approval: "待批准",
  in_review: "审查中",
  changes_requested: "需修改",
  done: "已完成",
  cancelled: "已取消",
};

const EVENT_LABELS: Record<string, string> = {
  "run.claimed": "任务已派发",
  "run.status_changed": "执行状态更新",
  "task.execution_completed": "Agent 已交付",
  "task.approved": "审查已通过",
  "task.changes_requested": "审查要求修改",
  "review.submitted": "Reviewer 已提交结论",
  "review.approved": "自动审核已通过",
  "review.changes_requested": "自动审核退回修改",
  "review.loop_exhausted": "审核修复次数已用尽",
  "task.retry_requested": "任务重新执行",
  "task.reassigned": "任务重新派发",
  "task.dependency_added": "新增协作依赖",
  "agent.control_changed": "控制权已切换",
  "project.status_changed": "项目状态更新",
  "task.capability_routed": "按能力重新派发",
  "run.retry_scheduled": "执行失败，已安排重试",
  "run.failed": "执行已失败",
  "message.sent": "Agent 发送消息",
  "artifact.published": "Agent 发布产物",
  "approval.requested": "Agent 请求审批",
  "memory.recorded": "Agent 记录经验",
};

function formatClock(value: number): string {
  return new Date(value).toLocaleTimeString("zh-CN", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/** 协作栏页签：群聊已移至右侧活动栏独立面板（GroupChatPanel），这里只留记录。 */
const RAIL_TABS = [{ key: "log", label: "记录" }] as const;

export function MultiAgentWorkspace({
  projectId,
  sessions,
  theme,
  terminalTransparent = false,
  onSessionsChange,
  onCloseSession,
  onClose,
}: Props) {
  const [snapshot, setSnapshot] = useState<MultiAgentSnapshot | null>(null);
  const [membersExpanded, setMembersExpanded] = useState(false);
  const [focusedAgentId, setFocusedAgentId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // 即时通道的投递记录（只读投影，合并进「协作记录」时间线）
  const [deliveryRecords, setDeliveryRecords] = useState<DeliveryRecord[]>([]);
  const refreshSequence = useRef(0);
  const activityCursor = useRef(0);
  const runtimeCursor = useRef(0);
  const controlRequest = useRef(0);
  const controlInFlight = useRef(false);
  const [controlBusy, setControlBusy] = useState(false);
  const [controlError, setControlError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const sequence = ++refreshSequence.current;
    if (UI_PREVIEW) {
      setSnapshot({ ...COLLABORATION_UI_PREVIEW_SNAPSHOT, project: { ...COLLABORATION_UI_PREVIEW_SNAPSHOT.project, id: projectId } });
      return;
    }
    try {
      const next = await invoke<MultiAgentSnapshot>("ma_snapshot", { projectId });
      if (sequence === refreshSequence.current) {
        activityCursor.current = Math.max(
          activityCursor.current,
          0,
          ...next.events.map((event) => event.seq),
        );
        runtimeCursor.current = Math.max(
          runtimeCursor.current,
          0,
          ...next.runtimeEvents.map((event) => event.seq),
        );
        setSnapshot(next);
        setError(null);
      }
    } catch (reason) {
      if (sequence === refreshSequence.current) setError(String(reason));
    }
    // 投递记录失败不影响快照主流程（投影是附加视图）
    try {
      const records = await invoke<DeliveryRecord[]>("list_delivery_records");
      if (sequence === refreshSequence.current) setDeliveryRecords(records);
    } catch {
      // 忽略：下一轮 2s 轮询会重试
    }
  }, [projectId]);

  useEffect(() => {
    let disposed = false;
    let timer: number | null = null;
    setSnapshot(null);
    setFocusedAgentId(null);
    setMembersExpanded(false);
    setError(null);
    setControlBusy(false);
    setControlError(null);
    controlInFlight.current = false;
    activityCursor.current = 0;
    runtimeCursor.current = 0;
    const tick = async () => {
      await refresh();
      if (!disposed) timer = window.setTimeout(() => void tick(), 2_000);
    };
    void tick();
    return () => {
      disposed = true;
      refreshSequence.current += 1;
      controlRequest.current += 1;
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [refresh]);

  useEffect(() => {
    if (UI_PREVIEW) return;
    let disposed = false;
    let timer: number | null = null;
    const tick = async () => {
      try {
        const [events, runtimeEvents] = await Promise.all([
          invoke<MultiAgentActivityEvent[]>("ma_activity_events", {
            projectId,
            afterSeq: activityCursor.current,
            limit: 500,
          }),
          invoke<RuntimeEventRecord[]>("ma_runtime_events", {
            projectId,
            afterSeq: runtimeCursor.current,
            limit: 500,
          }),
        ]);
        if (!disposed && (events.length > 0 || runtimeEvents.length > 0)) {
          activityCursor.current = Math.max(activityCursor.current, ...events.map((event) => event.seq));
          runtimeCursor.current = Math.max(runtimeCursor.current, ...runtimeEvents.map((event) => event.seq));
          setSnapshot((current) => current && current.project.id === projectId ? {
            ...current,
            events: [...current.events, ...events]
              .filter((event, index, items) => items.findIndex((item) => item.seq === event.seq) === index)
              .slice(-500),
            runtimeEvents: [...current.runtimeEvents, ...runtimeEvents]
              .filter((event, index, items) => items.findIndex((item) => item.seq === event.seq) === index)
              .slice(-1_000),
          } : current);
        }
      } catch {
        // 全量快照轮询会负责恢复；增量事件失败不覆盖主错误状态。
      }
      if (!disposed) timer = window.setTimeout(() => void tick(), 400);
    };
    void tick();
    return () => {
      disposed = true;
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [projectId]);

  const sessionByAgent = useMemo(
    () => new Map(snapshot?.agentSessions.map((session) => [session.agentId, session]) ?? []),
    [snapshot?.agentSessions],
  );
  const taskById = useMemo(
    () => new Map(snapshot?.tasks.map((task) => [task.id, task]) ?? []),
    [snapshot?.tasks],
  );
  const runById = useMemo(
    () => new Map(snapshot?.runs.map((run) => [run.id, run]) ?? []),
    [snapshot?.runs],
  );
  const changeControl = async (agentId: string, manual: boolean) => {
    if (UI_PREVIEW) return;
    if (controlInFlight.current) return;
    controlInFlight.current = true;
    const request = ++controlRequest.current;
    setControlBusy(true); setControlError(null);
    try {
      await invoke<AgentControlReceipt>(manual ? "ma_takeover_agent" : "ma_return_agent_control", {
        input: { projectId, agentId, ...(manual ? { instruction: null } : {}) },
      });
      if (request === controlRequest.current) await refresh();
    } catch (reason) {
      if (request === controlRequest.current) setControlError(String(reason));
    } finally {
      if (request === controlRequest.current) { controlInFlight.current = false; setControlBusy(false); }
    }
  };
  if (!snapshot) {
    return <section className="multi-agent-workspace loading">{error ?? "正在加载协作工作台…"}</section>;
  }

  const participation = workspaceParticipation(snapshot, sessions);
  const participatingAgents = snapshot.agents.filter((agent) => participation.participating.has(agent.id));

  // appSessionId → agentId 反向映射：把即时通道投递记录关联到本项目 agent
  const appSessionToAgent = new Map(
    snapshot.agentSessions
      .filter((session) => session.appSessionId)
      .map((session) => [session.appSessionId as string, session.agentId]),
  );
  const agentNameBySession = (sessionId: string | null): string | null => {
    if (!sessionId) return null;
    const agentId = appSessionToAgent.get(sessionId);
    return agentId
      ? snapshot.agents.find((agent) => agent.id === agentId)?.name ?? null
      : null;
  };

  const handoffItems = [
    ...snapshot.messages.map((message) => ({
      id: `message-${message.id}`,
      at: message.createdAt,
      kind: "消息",
      title: `${message.senderId ? snapshot.agents.find((agent) => agent.id === message.senderId)?.name ?? "Agent" : "总控"} → ${message.targetAgentId ? snapshot.agents.find((agent) => agent.id === message.targetAgentId)?.name ?? "Agent" : "项目"}`,
      body: message.body,
      agentId: message.targetAgentId ?? message.senderId,
    })),
    // 即时通道投影：只显示收发双方至少一方属于本项目的记录
    ...deliveryRecords
      .filter(
        (record) =>
          (record.fromSessionId && appSessionToAgent.has(record.fromSessionId)) ||
          appSessionToAgent.has(record.toSessionId),
      )
      .map((record) => {
        const fromName =
          agentNameBySession(record.fromSessionId) ?? record.fromLabel ?? "总控";
        const toName =
          agentNameBySession(record.toSessionId) ?? record.toSessionId.slice(0, 8);
        return {
          id: `delivery-${record.id}`,
          at: record.at,
          kind:
            record.kind === "system"
              ? "系统"
              : record.kind === "broadcast"
                ? "直通·广播"
                : "直通",
          title: `${fromName} → ${toName}`,
          body: record.body,
          agentId:
            appSessionToAgent.get(record.toSessionId) ??
            (record.fromSessionId
              ? appSessionToAgent.get(record.fromSessionId) ?? null
              : null),
        };
      }),
    ...snapshot.artifacts.map((artifact) => ({
      id: `artifact-${artifact.id}`,
      at: artifact.createdAt,
      kind: "交付",
      title: taskById.get(artifact.taskId)?.title ?? "任务交付物",
      body: `${artifact.summary}${artifact.contentHash ? `\n哈希 ${artifact.contentHash}` : ""}`,
      agentId: taskById.get(artifact.taskId)?.assigneeAgentId ?? null,
    })),
    ...snapshot.events.slice(-300).map((event) => {
      const payloadTaskId = typeof event.payload.taskId === "string" ? event.payload.taskId : null;
      const run = event.entityType === "run" ? runById.get(event.entityId) : null;
      const taskId = event.entityType === "task" ? event.entityId : payloadTaskId ?? run?.taskId ?? null;
      const task = taskId ? taskById.get(taskId) : null;
      const actorIsAgent = event.actorId
        ? snapshot.agents.some((agent) => agent.id === event.actorId)
        : false;
      const agentId = actorIsAgent ? event.actorId : run?.agentId ?? task?.assigneeAgentId ?? null;
      const baseTitle = EVENT_LABELS[event.eventType] ?? "协作事件";
      const eventAgent = agentId ? snapshot.agents.find((agent) => agent.id === agentId) : null;
      const title = event.eventType === "task.execution_completed" && eventAgent?.role === "integrator"
        ? "集成任务已完成"
        : event.eventType === "task.execution_completed" && eventAgent?.role === "reviewer"
          ? "审查任务已完成"
          : baseTitle;
      const state = typeof event.payload.to === "string" ? ` · ${STATUS_LABELS[event.payload.to] ?? event.payload.to}` : "";
      return {
        id: `event-${event.seq}`,
        at: event.createdAt,
        kind: "事件",
        title: `${title}${state}`,
        body: task?.title ?? "项目级事件",
        agentId,
      };
    }),
    ...snapshot.runtimeEvents
      .filter((event) => !["output", "heartbeat"].includes(event.eventType))
      .slice(-300)
      .map((event) => ({
        id: `runtime-event-${event.seq}`,
        at: event.createdAt,
        kind: "运行",
        title: `Agent ${event.eventType}`,
        body: taskById.get(event.taskId)?.title ?? "运行事件",
        agentId: event.agentId,
      })),
    ...snapshot.memories.slice(0, 100).map((memory) => ({
      id: `memory-${memory.id}`,
      at: memory.updatedAt,
      kind: "经验",
      title: `[${memory.namespace}] ${memory.key}`,
      body: memory.value,
      agentId: memory.sourceTaskId
        ? taskById.get(memory.sourceTaskId)?.assigneeAgentId ?? null
        : null,
    })),
  ].sort((left, right) => right.at - left.at);

  const appSessionByAgent = new Map(
    snapshot.agentSessions
      .filter((session) => session.appSessionId)
      .map((session) => [session.agentId, session.appSessionId as string]),
  );
  const linkedSessionIds = new Set(appSessionByAgent.values());
  const hiddenSessionIds = new Set(
    sessions.filter((session) => !linkedSessionIds.has(session.id)).map((session) => session.id),
  );
  const visualSessionCount = sessions.filter((session) => linkedSessionIds.has(session.id)).length;
  const focusedAppSessionId = focusedAgentId ? appSessionByAgent.get(focusedAgentId) ?? null : null;

  return (
    <section className="multi-agent-workspace">
      <header className="ma-workspace-header">
        <button type="button" onClick={onClose}>返回会话总览</button>
        <CommunicationGuideButton projectId={projectId} className="btn secondary communication-workspace-entry" />
      </header>

      {error && <div className="ma-workspace-error">{error}</div>}
      {controlError && <div className="ma-workspace-error" role="alert">{controlError}</div>}
      {snapshot.runs.filter(run => run.sessionLocator?.stopUnverified === true).map(run => <UnverifiedRunNotice key={run.id} run={run} taskTitle={snapshot.tasks.find(task => task.id === run.taskId)?.title} onConfirmed={refresh} />)}
      <div className="ma-workspace-body">
        <div className="ma-visible-terminal-stage">
          <div className="ma-participation-summary"><span>参与 <b>{participation.participating.size}</b> · 执行中 <b>{participation.executing.size}</b> · 待处理 <b>{participation.pending.size}</b></span><button type="button" aria-expanded={membersExpanded} onClick={() => setMembersExpanded((value) => !value)}>成员{membersExpanded ? " ▴" : " ▾"}</button>{focusedAgentId && <button type="button" onClick={() => setFocusedAgentId(null)}>显示全部终端</button>}</div>
          {membersExpanded && <div className="ma-agent-strip">
            {participatingAgents.map((agent) => {
              const session = sessionByAgent.get(agent.id) ?? null;
              const linked = appSessionByAgent.has(agent.id);
              return (
                <div className={focusedAgentId === agent.id ? "active" : ""} key={agent.id}>
                  <button
                    type="button"
                    disabled={!linked}
                    title={`${RUNTIME_LABELS[agent.runtimeKind]} · 能力：${agent.capabilities.join("、") || "未声明"}`}
                    onClick={() => setFocusedAgentId((current) => current === agent.id ? null : agent.id)}
                  >
                    <AgentAvatar className="ma-agent-logo" avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} />
                    <strong>{agent.name}</strong>
                    <em>{STATUS_LABELS[session?.status ?? "idle"]}</em>
                  </button>
                  {session?.controlMode === "manual" ? (
                    <button type="button" disabled={controlBusy} onClick={() => void changeControl(agent.id, false)}>交还总控</button>
                  ) : (
                    <button type="button" disabled={controlBusy} onClick={() => void changeControl(agent.id, true)}>人工接管</button>
                  )}
                </div>
              );
            })}
            {!participatingAgents.length && <span className="ma-no-participants">当前没有参与任务的成员；可在项目协作设置中编辑团队。</span>}
          </div>}
          {visualSessionCount > 0 ? (
            <MultiTerminal
              sessions={sessions}
              selectedSessionId={focusedAppSessionId}
              theme={theme}
              terminalTransparent={terminalTransparent}
              hiddenSessionIds={hiddenSessionIds}
              onClose={onCloseSession}
              onToggleMaximize={(appSessionId) => {
                const agentId = snapshot.agentSessions.find((item) => item.appSessionId === appSessionId)?.agentId ?? null;
                setFocusedAgentId((current) => current === agentId ? null : agentId);
              }}
              onSessionsChange={onSessionsChange}
            />
          ) : (
            <div className="ma-terminal-awaiting">暂无参与中的终端，可从右侧下达任务或选择成员。</div>
          )}
        </div>

        <aside className="ma-collaboration-rail">
          <header className="ma-collaboration-rail-header ma-rail-tabs">
            {RAIL_TABS.map((tab) => (
              <button key={tab.key} type="button" className="active">
                {tab.label}
              </button>
            ))}
          </header>
          <div className="ma-handoff-list">
            {handoffItems.length === 0 ? <p className="empty">暂无 Agent 交付记录</p> : handoffItems.map((item) => (
              <article
                key={item.id}
                role={item.agentId ? "button" : undefined}
                tabIndex={item.agentId ? 0 : undefined}
                onClick={() => item.agentId && setFocusedAgentId(item.agentId)}
              >
                <div><span>{item.kind}</span><time>{formatClock(item.at)}</time></div>
                <strong>{item.title}</strong>
                <pre>{item.body}</pre>
              </article>
            ))}
          </div>
        </aside>
      </div>
    </section>
  );
}
