import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  AgentActivity,
  AgentActivityState,
  DeliveryRecord,
  MultiAgentSnapshot,
  SessionRecord,
} from "../types";
import { AgentAvatar } from "./AgentAvatar";
import { buildTeamMessages, deliveryStageLabel, summarizeTeamSend } from "../teamMessages";

interface ChatRecipient { agentId: string; sessionId: string; name: string; requestId: string }
interface ChatBatch { projectId: string; draft: string; recipients: ChatRecipient[] }

interface Props {
  projectId: string | null;
  active: boolean;
  sessions: SessionRecord[];
  previewSnapshot?: MultiAgentSnapshot;
  previewEvidence?: DeliveryRecord[];
}

function formatClock(value: number): string {
  return new Date(value).toLocaleTimeString("zh-CN", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/** 群聊活跃度三档（出站投递语义见 control.rs AgentActivity）。 */
const ACTIVITY_LABELS: Record<AgentActivity, string> = {
  active: "活跃",
  lurk: "低频",
  muted: "静音",
};

const ACTIVITY_CYCLE: AgentActivity[] = ["active", "lurk", "muted"];

/** 协作面板内的项目团队消息：历史持久化，运输状态仅以证据为准。 */
export function GroupChatPanel({ projectId, active, sessions, previewSnapshot, previewEvidence }: Props) {
  const [snapshot, setSnapshot] = useState<MultiAgentSnapshot | null>(null);
  const [deliveryRecords, setDeliveryRecords] = useState<DeliveryRecord[]>([]);
  const [activityBySession, setActivityBySession] = useState<
    Record<string, AgentActivity>
  >({});
  const [chatDraft, setChatDraft] = useState("");
  const [chatTarget, setChatTarget] = useState<string>("all");
  const [sendingChat, setSendingChat] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sendError, setSendError] = useState<string | null>(null);
  const [retryBatch, setRetryBatch] = useState<ChatBatch | null>(null);
  const chatListRef = useRef<HTMLDivElement | null>(null);
  const refreshSequence = useRef(0);
  const projectGeneration = useRef(0);
  const sendInFlight = useRef(false);

  const refresh = useCallback(async () => {
    if (!projectId) return;
    const sequence = ++refreshSequence.current;
    if (previewSnapshot) {
      setSnapshot(current => current ?? previewSnapshot);
      setDeliveryRecords(current => current.length ? current : previewEvidence ?? []);
      return;
    }
    try {
      const next = await invoke<MultiAgentSnapshot>("ma_snapshot", { projectId });
      if (sequence !== refreshSequence.current) return;
      setSnapshot(next);
      setError(null);
      const sessionIds = next.agentSessions
        .map((session) => session.appSessionId)
        .filter((id): id is string => Boolean(id));
      if (sessionIds.length > 0) {
        try {
          const states = await invoke<AgentActivityState[]>(
            "list_agent_activity",
            { sessionIds },
          );
          if (sequence === refreshSequence.current) {
            setActivityBySession(
              Object.fromEntries(
                states.map((state) => [state.sessionId, state.activity]),
              ),
            );
          }
        } catch {
          // 活跃度读取失败只影响开关显示，下一轮轮询重试。
        }
      }
    } catch (reason) {
      if (sequence === refreshSequence.current) setError(String(reason));
    }
    try {
      const records = await invoke<DeliveryRecord[]>("list_communication_evidence");
      if (sequence === refreshSequence.current) setDeliveryRecords(records);
    } catch {
      if (sequence === refreshSequence.current) {
        setDeliveryRecords([]);
        setError("通信状态暂时无法读取；历史消息仍可查看，当前投递状态未知。");
      }
    }
  }, [projectId, previewSnapshot, previewEvidence]);

  useEffect(() => {
    projectGeneration.current += 1;
    sendInFlight.current = false;
    setSendingChat(false);
    refreshSequence.current += 1;
    setSnapshot(null);
    setDeliveryRecords([]);
    setActivityBySession({});
    setRetryBatch(null);
    setChatTarget("all");
    setChatDraft("");
    setError(null);
    setSendError(null);
  }, [projectId]);

  useEffect(() => {
    if (!active || !projectId) return;
    let disposed = false;
    let timer: number | null = null;
    const tick = async () => {
      await refresh();
      if (!disposed) timer = window.setTimeout(() => void tick(), 2_000);
    };
    void tick();
    return () => {
      disposed = true;
      refreshSequence.current += 1;
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [active, projectId, refresh]);

  const currentSnapshot = snapshot?.project.id === projectId ? snapshot : null;
  const appSessionByAgent = new Map(
    (currentSnapshot?.agentSessions ?? [])
      .filter((session) => session.appSessionId)
      .map((session) => [session.agentId, session.appSessionId as string]),
  );
  const liveSessionIds = new Set(
    previewSnapshot
      ? previewSnapshot.agentSessions.map(session => session.appSessionId ?? "")
      : sessions.filter((session) => !session.archivedAt && !session.retiredAgentRuntime
        && (session.ptyId !== null || session.runtimeCommandHost)).map((session) => session.id),
  );
  // 后台协作成员通过调度器接收消息，不一定拥有 PTY。
  // 普通会话仍要求在线终端；归档或已退役的绑定不能成为发送对象。
  const linkedAgents = (currentSnapshot?.agents ?? []).filter(
    (agent) =>
      agent.enabled && appSessionByAgent.has(agent.id) &&
      liveSessionIds.has(appSessionByAgent.get(agent.id) as string),
  );
  const chatItems = currentSnapshot ? buildTeamMessages(currentSnapshot, deliveryRecords) : [];

  useEffect(() => {
    const list = chatListRef.current;
    if (list) list.scrollTop = list.scrollHeight;
  }, [chatItems.length, active]);

  const cycleActivity = async (sessionId: string, current: AgentActivity) => {
    const next =
      ACTIVITY_CYCLE[
        (ACTIVITY_CYCLE.indexOf(current) + 1) % ACTIVITY_CYCLE.length
      ];
    // 乐观更新，失败时由下一轮轮询纠正。
    setActivityBySession((currentMap) => ({ ...currentMap, [sessionId]: next }));
    if (previewSnapshot) return;
    try {
      await invoke("set_agent_activity", { sessionId, activity: next });
    } catch (reason) {
      setError(String(reason));
      await refresh();
    }
  };

  const sendChat = async (retry = false) => {
    if (sendInFlight.current || !projectId || (!retry && retryBatch)) return;
    const batch: ChatBatch | null = retry ? retryBatch : {
      projectId,
      draft: chatDraft,
      recipients: linkedAgents
        .filter(agent => chatTarget === "all" || agent.id === chatTarget)
        .map(agent => ({ agentId: agent.id, sessionId: appSessionByAgent.get(agent.id)!, name: agent.name, requestId: crypto.randomUUID() })),
    };
    if (!batch || batch.projectId !== projectId || !batch.draft.trim()) return;
    if (!batch.recipients.length) {
      setSendError("所选会话不可用，请重新选择发送对象。");
      return;
    }
    setSendError(null);
    sendInFlight.current = true;
    setSendingChat(true);
    const generation = projectGeneration.current;
    const body = batch.draft.trim();
    try {
      const outcomes = await Promise.allSettled(batch.recipients.map(async target => {
        if (!linkedAgents.some(agent => agent.id === target.agentId && appSessionByAgent.get(agent.id) === target.sessionId)) {
          throw new Error("原收件会话已停止或已重新绑定，请重新选择目标");
        }
        if (previewSnapshot) {
          const record: DeliveryRecord = { id: target.requestId, at: Date.now(), fromSessionId: null,
            fromLabel: null, toSessionId: target.sessionId, body, kind: "chat", stage: "queued" };
          setDeliveryRecords(current => [...current, record]);
          return;
        }
        await invoke("send_session_message", { sessionId: target.sessionId, projectId, body, requestId: target.requestId });
      }));
      if (generation !== projectGeneration.current) return;
      const { failures, clearDraft } = summarizeTeamSend(outcomes, batch.recipients.map(member => member.name));
      if (clearDraft) setChatDraft(current => current === batch.draft ? "" : current);
      const failed = batch.recipients.filter((_, i) => outcomes[i].status === "rejected");
      setRetryBatch(failed.length ? { ...batch, recipients: failed } : null);
      if (failed.length) setSendError(`待补发：${failures.join("；")}`);
      await refresh();
    } catch (reason) {
      if (generation === projectGeneration.current) setSendError(String(reason));
    } finally {
      if (generation === projectGeneration.current) {
        sendInFlight.current = false;
        setSendingChat(false);
      }
    }
  };

  return (
    <div className="group-chat-panel">
      <header className="group-chat-panel-header">
        <strong>团队消息</strong>
        <small title={snapshot?.project.name ?? ""}>
          {snapshot?.project.name ?? "未选择项目"}
        </small>
      </header>
      {error && <div className="group-chat-panel-error">{error}</div>}
      {sendError && <div className="group-chat-panel-error" role="alert">{sendError}</div>}
      {retryBatch?.projectId === projectId && retryBatch && <div className="group-chat-retry">
        <p>待补发原文：{retryBatch.draft}</p>
        <button type="button" className="btn secondary" disabled={sendingChat} onClick={() => void sendChat(true)}>仅补发失败成员（{retryBatch.recipients.length}）</button>
        <button type="button" className="btn ghost" disabled={sendingChat} onClick={() => { setRetryBatch(null); setSendError(null); }}>放弃补发</button>
      </div>}
      {previewSnapshot && <p className="group-chat-hint">UI 预览：发送与频率切换仅模拟，不连接真实 Agent。</p>}
      {!projectId ? (
        <p className="group-chat-empty">
          请先在任务指令中选择或创建协作项目。
        </p>
      ) : !currentSnapshot ? (
        <p className="group-chat-empty">正在加载团队消息…</p>
      ) : (
        <div className="ma-group-chat">
          {linkedAgents.length === 0 ? (
            <p className="group-chat-hint">
              当前没有运行中的成员。历史通信和任务结果仍可查看；启动成员后可人工插话。
            </p>
          ) : (
            <div className="ma-activity-row" aria-label="Agent 活跃度开关">
            {linkedAgents.map((agent) => {
              const sessionId = appSessionByAgent.get(agent.id) as string;
              const activity = activityBySession[sessionId] ?? "active";
              return (
                <button
                  key={agent.id}
                  type="button"
                  className={`ma-activity-chip activity-${activity}`}
                  title={`点击切换活跃度（当前：${ACTIVITY_LABELS[activity]}）。静音后 Agent 互发暂存不投递，人工消息照常。`}
                  onClick={() => void cycleActivity(sessionId, activity)}
                >
                  <AgentAvatar className="ma-agent-logo" avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} />
                  <strong>{agent.name}</strong>
                  <em>{ACTIVITY_LABELS[activity]}</em>
                </button>
              );
            })}
            </div>
          )}
          <div className="ma-chat-list" ref={chatListRef}>
            {chatItems.length === 0 ? (
              <p className="empty">
                暂无团队消息。这里保留项目指令、Agent 通信与任务执行摘要。
              </p>
            ) : (
              chatItems.map((record) => {
                const fromHuman = record.from === "你";
                return (
                  <article
                    key={record.id}
                    className={`ma-chat-bubble${fromHuman ? " from-human" : ""}`}
                  >
                    <div>
                      <span>
                        {record.from} → {record.to}
                      </span>
                      <time>{formatClock(record.at)}</time>
                    </div>
                    <pre>{record.body}</pre>
                    <small className="team-message-evidence">{record.source}
                      {record.stage && ` · ${deliveryStageLabel(record.stage)}`}
                    </small>
                    {record.error && <p className="team-message-error">{record.error}</p>}
                  </article>
                );
              })
            )}
          </div>
          <footer className="ma-chat-composer">
            <select
              value={chatTarget}
              aria-label="发送对象"
              onChange={(event) => setChatTarget(event.target.value)}
            >
              <option value="all">全员</option>
              {linkedAgents.map((agent) => (
                <option key={agent.id} value={agent.id}>
                  {agent.name}
                </option>
              ))}
            </select>
            <textarea
              rows={2}
              value={chatDraft}
              placeholder="人工插话（全员逐个入队；Ctrl+Enter 发送）"
              onChange={(event) => setChatDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
                  event.preventDefault();
                  void sendChat();
                }
              }}
            />
            <button
              type="button"
              disabled={!chatDraft.trim() || sendingChat || !!retryBatch || linkedAgents.length === 0}
              onClick={() => void sendChat()}
            >
              {sendingChat ? "发送中…" : "发送"}
            </button>
          </footer>
        </div>
      )}
    </div>
  );
}
