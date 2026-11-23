import { useEffect, useRef, useState } from "react";
import { ArrowSquareInIcon } from "@phosphor-icons/react/dist/csr/ArrowSquareIn";
import { ArrowSquareOutIcon } from "@phosphor-icons/react/dist/csr/ArrowSquareOut";
import { WarningCircleIcon } from "@phosphor-icons/react/dist/csr/WarningCircle";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import { InterventionCenter } from "./InterventionCenter";
import { MultiAgentControl } from "./MultiAgentControl";
import { hasOpenModal } from "./ui/Modal";
import { saveControlSurface, type ControlSurface } from "../controlSurface";
import type { QuickContext } from "../quickCommands";
import type { ConversationOriginalSession, ProviderConfig, SessionRecord } from "../types";

interface Props {
  quickContext?: QuickContext;
  onOpenQuickSettings?: () => void;
  open: boolean;
  active?: boolean;
  projectOverview?: boolean;
  variant?: "docked" | "detached";
  providers: ProviderConfig[];
  sessions: SessionRecord[];
  defaultCwd: string | null;
  onClose: () => void;
  onDetach?: () => void | Promise<void>;
  onDock?: () => void | Promise<void>;
  onSessionsChange: (sessions: SessionRecord[]) => void;
  initialSurface?: ControlSurface;
  isolatedSurface?: boolean;
  onSurfaceChange?: (surface: ControlSurface) => void;
  onOpenSession?: (sessionId: string | null, original?: ConversationOriginalSession) => void | Promise<void>;
  onOpenProjectSettings?: (projectId?: string, target?: "tasks") => void;
}

/** Shared dock/detached shell. General commands remain available from their own application entry. */
export function ControlSidebar({ open, active, projectOverview, variant = "docked", sessions, defaultCwd, onClose, onDetach, onDock, onSurfaceChange, isolatedSurface = false, onOpenSession, onOpenProjectSettings }: Props) {
  const [interventionsOpen, setInterventionsOpen] = useState(false);
  const [interventionCount, setInterventionCount] = useState(0);
  const [focusedProject, setFocusedProject] = useState<{ id: string; request: number } | null>(null);
  const panel = useRef<HTMLElement>(null);
  const isActive = active ?? open;
  useEffect(() => {
    if (!isolatedSurface) saveControlSurface("multi_agent");
    onSurfaceChange?.("multi_agent");
  }, [isolatedSurface, onSurfaceChange]);
  useEffect(() => {
    if (!open || !isActive) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const frame = requestAnimationFrame(() => panel.current?.querySelector<HTMLElement>('textarea:not([disabled]), button:not([disabled])')?.focus());
    const key = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented || event.isComposing || hasOpenModal()) return;
      if (interventionsOpen) setInterventionsOpen(false); else onClose();
    };
    document.addEventListener("keydown", key);
    return () => { cancelAnimationFrame(frame); document.removeEventListener("keydown", key); previous?.focus(); };
  }, [open, isActive, onClose, interventionsOpen]);
  if (!open) return null;
  return <div className={`control-overlay control-${variant} collaboration-control-overlay`} data-multi-agent-style="grok_noir">
    <aside ref={panel} className="control-sidebar collaboration-control-sidebar" data-multi-agent-style="grok_noir" role="complementary" aria-label="多智能体协作" tabIndex={-1}>
      <header className="conversation-shell-header">
        <div className="conversation-shell-actions">
          <button type="button" className="conversation-icon" aria-label={`待处理中心，${interventionCount} 项`} title="待处理中心" aria-expanded={interventionsOpen} onClick={() => setInterventionsOpen(value => !value)}><WarningCircleIcon size={17} />{interventionCount > 0 && <span>{Math.min(interventionCount, 99)}</span>}</button>
          {variant === "docked" && onDetach && <button type="button" className="conversation-icon" aria-label="分离多智能体窗口" title="分离为独立窗口" onClick={() => void onDetach()}><ArrowSquareOutIcon size={16} /></button>}
          {variant === "detached" && onDock && <button type="button" className="conversation-icon" aria-label="重新停靠多智能体窗口" onClick={() => void onDock()}><ArrowSquareInIcon size={16} /></button>}
          <button type="button" className="conversation-icon control-close" aria-label="关闭多智能体面板" onClick={onClose}><XIcon size={18} /></button>
        </div>
      </header>
      <InterventionCenter open={interventionsOpen} onClose={() => setInterventionsOpen(false)} onCountChange={setInterventionCount} onOpenProject={id => { setInterventionsOpen(false); setFocusedProject(value => ({ id, request: (value?.request ?? 0) + 1 })); }} />
      <div className="control-multi-agent-surface"><MultiAgentControl sessions={sessions} active={isActive} projectOverview={projectOverview} defaultCwd={defaultCwd} focusedProjectId={focusedProject?.id} focusedProjectRequest={focusedProject?.request} onOpenSession={onOpenSession} onOpenProjectSettings={onOpenProjectSettings} /></div>
    </aside>
  </div>;
}
