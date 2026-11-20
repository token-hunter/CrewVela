import { UnverifiedRunNotice } from "./UnverifiedRunNotice";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { COLLABORATION_UI_PREVIEW_SNAPSHOT } from "./CollaborationSettingsPanel";
import { workspaceParticipation } from "../multiAgentWorkspacePresentation";
import { isBackgroundChatHost } from "../sessionCapabilities";
import { MultiTerminal } from "./MultiTerminal";
import type {
  AppTheme,
  AgentControlReceipt,
  AgentSessionRecord,
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

  const appSessionByAgent = new Map(
    snapshot.agentSessions
      .filter((session) => session.appSessionId)
      .map((session) => [session.agentId, session.appSessionId as string]),
  );
  const linkedSessionIds = new Set(appSessionByAgent.values());
  const hiddenSessionIds = new Set(
    sessions.filter((session) => !linkedSessionIds.has(session.id)).map((session) => session.id),
  );
  const focusedAppSessionId = focusedAgentId ? appSessionByAgent.get(focusedAgentId) ?? null : null;
  const hasProjectSession = sessions.some(session => linkedSessionIds.has(session.id)
    && (session.id === focusedAppSessionId || !isBackgroundChatHost(session)));

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
          {hasProjectSession ? (
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
      </div>
    </section>
  );
}
