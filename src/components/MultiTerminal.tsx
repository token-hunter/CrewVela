import { sessionDisplayTitle, sessionDisplayProvider } from "../sessionCapabilities";
import { useEffect, useState } from "react";
import { usePtyPolling } from "../ptyPolling";
import { TerminalPane } from "./TerminalPane";
import type { AppTheme, SessionRecord } from "../types";
import { retainedSwitchSource, sessionSwitchReason, switchSlotId, switchedHistory, type SessionSwitchRecord } from "../sessionSwitch";
import { HandoffSourceView } from "./HandoffSourceView";
import { isNativeChatSession, isPtySession } from "../sessionCapabilities";

interface Props {
  sessions: SessionRecord[];
  selectedSessionId: string | null;
  theme: AppTheme;
  terminalTransparent?: boolean;
  preview?: boolean;
  /** 非当前页签的会话：保持挂载（保住 xterm 缓冲与轮询），仅视觉上隐藏 */
  hiddenSessionIds?: ReadonlySet<string>;
  onClose: (id: string) => void;
  onToggleMaximize: (id: string) => void;
  onSessionsChange: (sessions: SessionRecord[]) => void;
  switchRecords?: SessionSwitchRecord[];
  onSwitchAgent?: (session: SessionRecord) => void;
  onViewSourceHistory?: (session: SessionRecord) => void;
  onRelayDetails?: (session: SessionRecord) => void;
}

/** N → 网格列数：cols = ceil(sqrt(N))，rows = ceil(N/cols) 由 CSS 自动换行 */
function gridCols(n: number): number {
  if (n <= 0) return 1;
  return Math.ceil(Math.sqrt(n));
}

export function MultiTerminal({
  sessions,
  selectedSessionId,
  theme,
  terminalTransparent = false,
  preview = false,
  hiddenSessionIds,
  onClose,
  onToggleMaximize,
  onSessionsChange,
  switchRecords = [],
  onSwitchAgent,
  onViewSourceHistory,
  onRelayDetails,
}: Props) {
  const [slotIds, setSlotIds] = useState<string[]>([]);
  const [draggedId, setDraggedId] = useState<string | null>(null);
  const [dropTargetId, setDropTargetId] = useState<string | null>(null);

  // 拖拽 pane-head 在网格内重新排列会话格子（会话内有效，重启恢复成 sessions 顺序）。
  const resetDragState = () => {
    setDraggedId(null);
    setDropTargetId(null);
  };

  const moveSlot = (targetId: string) => {
    if (!draggedId || draggedId === targetId) {
      resetDragState();
      return;
    }
    setSlotIds((current) => {
      const sourceIndex = current.indexOf(draggedId);
      const targetIndex = current.indexOf(targetId);
      if (sourceIndex < 0 || targetIndex < 0) return current;
      const next = current.slice();
      next.splice(sourceIndex, 1);
      next.splice(targetIndex, 0, draggedId);
      return next;
    });
    resetDragState();
  };

  usePtyPolling(onSessionsChange, !preview);

  useEffect(() => {
    setSlotIds((current) => {
      const liveIds = new Set(
        sessions.filter(session => isPtySession(session) || (!isNativeChatSession(session) && retainedSwitchSource(session.id, switchRecords))).map((session) => switchSlotId(session.id, switchRecords)),
      );
      const next = current.filter((id) => liveIds.has(id));
      const nextIds = new Set(next);
      for (const session of sessions) {
        const slotId = switchSlotId(session.id, switchRecords);
        if ((isPtySession(session) || (!isNativeChatSession(session) && retainedSwitchSource(session.id, switchRecords))) && !nextIds.has(slotId)) {
          next.push(slotId);
          nextIds.add(slotId);
        }
      }
      return next.length === current.length && next.every((id, index) => id === current[index])
        ? current
        : next;
    });
  }, [sessions, switchRecords]);

  const visibleCount = hiddenSessionIds
    ? slotIds.filter((id) => !hiddenSessionIds.has(sessions.find((session) => switchSlotId(session.id, switchRecords) === id)?.id ?? id)).length
    : slotIds.length;
  const cols = selectedSessionId === null ? gridCols(visibleCount) : 1;

  return (
    <div className="grid" style={{ gridTemplateColumns: `repeat(${cols}, 1fr)` }}>
      {slotIds.map((id) => {
        const session = sessions.find((item) => switchSlotId(item.id, switchRecords) === id);
        const hidden =
          (selectedSessionId !== null && selectedSessionId !== session?.id) ||
          (hiddenSessionIds?.has(session?.id ?? id) ?? false);
        const relay = session && retainedSwitchSource(session.id, switchRecords);
        if (session && relay) return <div key={id} className={`pane${hidden ? " pane-hidden" : ""}`}><HandoffSourceView
          session={session} record={relay} target={sessions.find(item => item.id === relay.targetId)} preview={preview}
          onSelectTarget={() => { if (relay.targetId) onToggleMaximize(relay.targetId); }} onDetails={() => onRelayDetails?.(session)}
        /></div>;
        if (!session || !isPtySession(session) || session.ptyId === null) {
          return <div key={id} className={`pane pane-vacant${hidden ? " pane-hidden" : ""}`} />;
        }
        return (
          <TerminalPane
            key={`${session.id}:${session.ptyId}`}
            sessionId={session.ptyId}
            title={sessionDisplayTitle(session)}
            providerName={sessionDisplayProvider(session)}
            kind={session.kind}
            shell={session.shell}
            agentRuntimeKind={session.agentRuntimeKind ?? null}
            agentAvatarId={session.agentAvatarId}
            cliLaunchMode={session.cliLaunchMode}
            multiAgentProjectId={session.multiAgentProjectId ?? null}
            multiAgentAgentId={session.multiAgentAgentId ?? null}
            cwd={session.cwd}
            status={session.status}
            theme={theme}
            terminalTransparent={terminalTransparent}
            preview={preview}
            hidden={hidden}
            maximized={selectedSessionId === session.id}
            onClose={() => onClose(session.id)}
            onToggleMaximize={() => onToggleMaximize(session.id)}
            onSwitchAgent={onSwitchAgent && sessionSwitchReason(session) === null ? () => onSwitchAgent(session) : undefined}
            onViewSourceHistory={onViewSourceHistory && switchedHistory(session.id, switchRecords) ? () => onViewSourceHistory(switchedHistory(session.id, switchRecords)!) : undefined}
            relaySession={Boolean(switchedHistory(session.id, switchRecords))}
            onRelayDetails={onRelayDetails && switchedHistory(session.id, switchRecords) ? () => onRelayDetails(session) : undefined}
            dragged={draggedId === id}
            dropTarget={dropTargetId === id}
            onPaneDragStart={() => {
              setDraggedId(id);
              setDropTargetId(null);
            }}
            onPaneDragEnd={resetDragState}
            onPaneDragOver={() => {
              if (!draggedId || draggedId === id) return;
              setDropTargetId(id);
            }}
            onPaneDragLeave={() => {
              setDropTargetId((current) =>
                current === id ? null : current,
              );
            }}
            onPaneDrop={() => moveSlot(id)}
          />
        );
      })}
    </div>
  );
}
