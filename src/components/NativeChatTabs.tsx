import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Plus, X, ArrowsLeftRight } from "@phosphor-icons/react";
import { RUNTIME_LABELS, RUNTIME_LOGOS } from "../runtimeLogos";
import { sessionDisplayTitle } from "../sessionCapabilities";
import type { NativeAgentKind } from "../nativeAgentSwitch";
import type { SessionRecord } from "../types";

const agents: NativeAgentKind[] = ["claude_chat", "codex_chat", "kimi_chat", "zcode", "deepseek_harness", "opencode_chat", "pi_chat"];
const statusLabels: Record<string, string> = { running: "运行中", waiting: "等待确认", stopped: "已停止", completed: "已完成", error: "出错" };
interface TabTooltip {
  sessionId: string;
  title: string;
  cwd: string;
  anchor: HTMLButtonElement;
  source: "pointer" | "focus";
}
interface Props {
  sessions: SessionRecord[];
  selectedId: string | null;
  creating: boolean;
  enabledAgents: Record<NativeAgentKind, boolean>;
  canCreate: boolean;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onCreate: (kind: NativeAgentKind) => void;
  onReorder: (source: string, target: string, after: boolean) => void;
  orderError?: string | null;
  onHandoff?: () => void;
}

export function NativeChatTabs({ sessions, selectedId, creating, enabledAgents, canCreate, onSelect, onClose, onCreate, onHandoff, onReorder, orderError }: Props) {
  const [menuOpen, setMenuOpen] = useState(false);
  const menu = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const tabs = useRef<HTMLDivElement>(null);
  const drag = useRef<{ id: string; x: number; active: boolean; pointerId: number; button: HTMLButtonElement } | null>(null);
  const suppressClickUntil = useRef(0);
  const [dragId, setDragId] = useState<string | null>(null);
  const [drop, setDrop] = useState<{ id: string; after: boolean } | null>(null);
  const dropRef = useRef<{ id: string; after: boolean } | null>(null);
  const [tabTooltip, setTabTooltip] = useState<TabTooltip | null>(null);
  const [tabTooltipPosition, setTabTooltipPosition] = useState<{ left: number; top: number } | null>(null);
  const tabTooltipRef = useRef<HTMLDivElement>(null);
  const clearTabTooltip = () => {
    setTabTooltip(null);
    setTabTooltipPosition(null);
  };
  const showTabTooltip = (session: SessionRecord, anchor: HTMLButtonElement, source: TabTooltip["source"]) => {
    if (session.id === selectedId) { clearTabTooltip(); return; }
    setTabTooltip({ sessionId: session.id, title: sessionDisplayTitle(session), cwd: session.cwd, anchor, source });
    setTabTooltipPosition(null);
  };
  const finishDrag = () => {
    drag.current = null;
    dropRef.current = null;
    setDragId(null);
    setDrop(null);
    clearTabTooltip();
  };
  const current = sessions.find(session => session.id === selectedId);
  useEffect(() => {
    if (!menuOpen) return;
    const dismiss = (event: PointerEvent) => { if (!menu.current?.contains(event.target as Node)) setMenuOpen(false); };
    document.addEventListener("pointerdown", dismiss);
    return () => document.removeEventListener("pointerdown", dismiss);
  }, [menuOpen]);
  useEffect(() => { setMenuOpen(false); clearTabTooltip(); }, [selectedId]);
  useEffect(() => {
    clearTabTooltip();
    tabs.current?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.closest('.native-chat-tab')?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [selectedId, sessions.length]);
  useLayoutEffect(() => {
    if (!tabTooltip) return;
    const updatePosition = () => {
      if (!tabTooltip.anchor.isConnected) { clearTabTooltip(); return; }
      const anchorBounds = tabTooltip.anchor.getBoundingClientRect();
      const tooltipBounds = tabTooltipRef.current?.getBoundingClientRect();
      if (!tooltipBounds) return;
      const gutter = 12;
      const left = Math.max(gutter, Math.min(
        anchorBounds.left + (anchorBounds.width - tooltipBounds.width) / 2,
        window.innerWidth - tooltipBounds.width - gutter,
      ));
      const below = anchorBounds.bottom + 8;
      const top = below + tooltipBounds.height <= window.innerHeight - gutter
        ? below
        : Math.max(gutter, anchorBounds.top - tooltipBounds.height - 8);
      setTabTooltipPosition({ left, top });
    };
    const dismiss = () => clearTabTooltip();
    updatePosition();
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", dismiss, true);
    return () => {
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", dismiss, true);
    };
  }, [tabTooltip]);
  return <><div className="native-chat-navigation">
    <div ref={tabs} className="native-chat-tabs" role="tablist" aria-label="已打开的聊天" onScroll={clearTabTooltip}>
      {sessions.map(session => {
        const displayTitle = sessionDisplayTitle(session);
        const statusLabel = statusLabels[session.status] ?? session.status;
        return <div key={session.id} data-native-tab-id={session.id} onDragStart={event => event.preventDefault()}
        className={`native-chat-tab${session.id === selectedId ? " selected" : ""}${dragId === session.id ? " sorting" : ""}${drop?.id === session.id ? (drop.after ? " drop-after" : " drop-before") : ""}`}
        onPointerEnter={event => {
          const anchor = event.currentTarget.querySelector<HTMLButtonElement>('[role="tab"]');
          if (anchor) showTabTooltip(session, anchor, "pointer");
        }}
        onPointerLeave={event => {
          if (event.currentTarget.contains(document.activeElement)) {
            const anchor = event.currentTarget.querySelector<HTMLButtonElement>('[role="tab"]');
            if (anchor) showTabTooltip(session, anchor, "focus");
          } else if (tabTooltip?.source === "pointer" && tabTooltip.sessionId === session.id) clearTabTooltip();
        }}
        onFocusCapture={event => {
          const anchor = event.currentTarget.querySelector<HTMLButtonElement>('[role="tab"]');
          if (anchor) showTabTooltip(session, anchor, "focus");
        }}
        onBlurCapture={event => {
          if (event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) return;
          if (tabTooltip?.source === "pointer" && event.currentTarget.matches(":hover")) return;
          if (tabTooltip?.sessionId === session.id) clearTabTooltip();
        }}
        onClickCapture={event => { if (Date.now() < suppressClickUntil.current) { event.preventDefault(); event.stopPropagation(); } }}
        onPointerDown={event => {
          if (event.button !== 0 || (event.target as HTMLElement).closest('.native-chat-tab-close')) return;
          const button = event.currentTarget.querySelector<HTMLButtonElement>('[role="tab"]');
          if (!button) return;
          drag.current = { id: session.id, x: event.clientX, active: false, pointerId: event.pointerId, button };
          button.setPointerCapture(event.pointerId);
        }}
        onPointerMove={event => {
          const state = drag.current;
          if (!state || state.id !== session.id || state.pointerId !== event.pointerId) return;
          if (!state.active && Math.abs(event.clientX - state.x) < 6) return;
          if (!state.active) { state.active = true; clearTabTooltip(); setDragId(state.id); }
          event.preventDefault();
          const target = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>('[data-native-tab-id]');
          const id = target?.dataset.nativeTabId;
          const next = id && id !== state.id && sessions.some(item => item.id === id) ? { id, after: event.clientX > target!.getBoundingClientRect().left + target!.getBoundingClientRect().width / 2 } : null;
          dropRef.current = next;
          setDrop(next);
          const bounds = tabs.current?.getBoundingClientRect();
          if (bounds && tabs.current) { if (event.clientX > bounds.right - 30) tabs.current.scrollLeft += 16; else if (event.clientX < bounds.left + 30) tabs.current.scrollLeft -= 16; }
        }}
        onPointerUp={event => {
          const state = drag.current;
          if (!state || state.id !== session.id || state.pointerId !== event.pointerId) return;
          if (state.active) {
            suppressClickUntil.current = Date.now() + 400;
            const target = dropRef.current;
            if (target) onReorder(state.id, target.id, target.after);
          }
          if (state.button.hasPointerCapture(event.pointerId)) state.button.releasePointerCapture(event.pointerId);
          finishDrag();
        }}
        onPointerCancel={finishDrag} onLostPointerCapture={finishDrag}>
        {drop?.id === session.id && <span className={`native-chat-drop-marker ${drop.after ? "after" : "before"}`} />}
        <button type="button" role="tab" aria-selected={session.id === selectedId} aria-label={`${displayTitle}，${statusLabel}`} aria-describedby={tabTooltip?.sessionId === session.id ? `native-chat-tooltip-${session.id}` : undefined} title={session.id === selectedId ? `${displayTitle} · ${session.cwd}` : undefined} onClick={() => onSelect(session.id)}
          onKeyDown={event => {
            if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
            event.preventDefault();
            const index = sessions.findIndex(item => item.id === session.id);
            const next = event.key === "Home" ? 0 : event.key === "End" ? sessions.length - 1 : (index + (event.key === "ArrowRight" ? 1 : -1) + sessions.length) % sessions.length;
            onSelect(sessions[next].id);
            event.currentTarget.closest('[role="tablist"]')?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
          }}>
          <img src={RUNTIME_LOGOS[session.agentRuntimeKind as NativeAgentKind]} data-runtime={session.agentRuntimeKind as NativeAgentKind} alt="" draggable={false} />
          <span className="native-chat-tab-title">{displayTitle}</span>
          <i className={`native-chat-status ${session.status}`} aria-label={`状态：${statusLabel}`} />
        </button>
        <button type="button" className="native-chat-tab-close" aria-label={`关闭「${displayTitle}」标签`} title="关闭标签，保留会话和运行任务；可从左侧重新打开" onClick={() => onClose(session.id)}><X size={14} /></button>
      </div>;
      })}
    </div>
    <div className="native-chat-create" ref={menu} onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget as Node)) setMenuOpen(false); }} onKeyDown={event => {
      if (event.key === "Escape" && menuOpen) { event.preventDefault(); event.stopPropagation(); setMenuOpen(false); trigger.current?.focus(); }
      if (menuOpen && ["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
        event.preventDefault();
        const buttons = Array.from(menu.current!.querySelectorAll<HTMLButtonElement>('.native-chat-create-menu button:not(:disabled)'));
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
        const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : index < 0 ? (event.key === "ArrowDown" ? 0 : buttons.length - 1) : (index + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
        buttons[next]?.focus();
      }
    }}>
      <button ref={trigger} type="button" className="btn secondary" aria-label={creating ? "正在新建聊天" : "新建聊天"} title="新建聊天" aria-expanded={menuOpen} aria-controls="native-chat-create-menu" disabled={creating || !canCreate} onClick={() => setMenuOpen(value => !value)}><Plus size={17} /></button>
      {menuOpen && <div className="native-chat-create-menu" id="native-chat-create-menu" aria-label="选择 Agent，新建独立聊天">
        <p>选择 Agent，新建独立聊天</p>
        {agents.map(kind => <button type="button" key={kind} disabled={!enabledAgents[kind] || creating} onClick={() => { setMenuOpen(false); trigger.current?.focus(); onCreate(kind); }}>
          <img src={RUNTIME_LOGOS[kind]} data-runtime={kind} alt="" /><span>{RUNTIME_LABELS[kind]}</span>{!enabledAgents[kind] && <small>未启用</small>}
        </button>)}
      </div>}
    </div>
    {current && onHandoff && <button type="button" className="btn secondary native-chat-handoff" title="把当前工作交接给其它Agent" aria-label="把当前工作交接给其它Agent" disabled={creating} onClick={onHandoff}><ArrowsLeftRight size={17} aria-hidden="true" /></button>}
  </div>
    {orderError && <p className="native-chat-tab-order-error" role="alert">{orderError}</p>}
    {tabTooltip && createPortal(
      <div
        ref={tabTooltipRef}
        id={`native-chat-tooltip-${tabTooltip.sessionId}`}
        className="native-chat-tab-hovercard"
        role="tooltip"
        style={{
          left: tabTooltipPosition?.left ?? 0,
          top: tabTooltipPosition?.top ?? 0,
          visibility: tabTooltipPosition ? "visible" : "hidden",
        }}
      >
        <strong>{tabTooltip.title}</strong>
        <span>{tabTooltip.cwd}</span>
      </div>,
      document.body,
    )}
  </>;
}
