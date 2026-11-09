import { ArrowSquareOutIcon } from "@phosphor-icons/react/dist/csr/ArrowSquareOut";
import { CaretUpIcon } from "@phosphor-icons/react/dist/csr/CaretUp";
import { CommandIcon } from "@phosphor-icons/react/dist/csr/Command";
import type { PermissionMode, SessionRecord } from "../types";

interface Props {
  sessions: SessionRecord[];
  expanded: boolean;
  detached: boolean;
  onToggle: () => void;
  onDetach: () => void | Promise<void>;
  onShowDetached: () => void | Promise<void>;
}

const MODE_LABELS: Partial<Record<PermissionMode, string>> = {
  default: "Default",
  acceptEdits: "Accept Edits",
  plan: "Plan",
  auto: "Auto",
};

function modeSummary(sessions: SessionRecord[]): string {
  const liveSessions = sessions.filter(
    (session) => session.kind === "claude" && session.ptyId !== null,
  );
  const firstMode = liveSessions[0]?.permissionMode;
  if (!firstMode || liveSessions.some((session) => session.permissionMode !== firstMode)) {
    return "Mixed";
  }
  return MODE_LABELS[firstMode] ?? "Mixed";
}

export function ControlDock({
  sessions,
  expanded,
  detached,
  onToggle,
  onDetach,
  onShowDetached,
}: Props) {
  const targetCount = sessions.filter(
    (session) => session.kind === "claude" && session.ptyId !== null,
  ).length;

  return (
    <footer className={`control-dock${expanded ? " expanded" : ""}`}>
      <button
        className="control-dock-title"
        type="button"
        aria-expanded={expanded}
        onClick={detached ? onShowDetached : onToggle}
      >
        <CommandIcon size={19} weight="duotone" />
        <span>总控</span>
        {detached && <small>独立窗口</small>}
      </button>

      <div className="control-dock-summary">
        <span className="control-dock-label">目标</span>
        <strong>{targetCount}</strong>
        <span className="control-dock-divider" />
        <span className="control-dock-label">统一模式</span>
        <strong>{modeSummary(sessions)}</strong>
      </div>

      <div className="control-dock-actions">
        {!detached && (
          <button
            className="control-dock-expand"
            type="button"
            aria-label={expanded ? "收起总控" : "展开总控"}
            title={expanded ? "收起总控" : "展开总控：统一模式与发送至全部"}
            aria-expanded={expanded}
            onClick={onToggle}
          >
            <CaretUpIcon size={17} weight="bold" />
          </button>
        )}
        <button
          className="control-dock-detach"
          type="button"
          onClick={detached ? onShowDetached : onDetach}
        >
          <ArrowSquareOutIcon size={17} />
          <span>{detached ? "显示窗口" : "分离窗口"}</span>
        </button>
      </div>
    </footer>
  );
}
