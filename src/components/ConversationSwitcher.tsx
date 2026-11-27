import { useEffect, useId, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CaretDown, ChatCircleText, Check, DotsThree, PencilSimple, Plus } from "@phosphor-icons/react";
import type { ConversationHistoryEntry } from "../types";
import { conversationTitle } from "./ConversationHistoryDialog";
import { useTranslation } from "../useInterfaceLanguage";

export function ConversationTopicStrip({ projectId, currentId, currentTitle, busy, fullWidth, revision, onSwitch, onCreate }: {
  projectId: string; currentId: string; currentTitle: string; busy: boolean; fullWidth: boolean;
  revision: number;
  onSwitch: (id: string) => Promise<boolean>; onCreate: () => Promise<void>;
}) {
  const t = useTranslation();
  const [entries, setEntries] = useState<ConversationHistoryEntry[]>([]);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let current = true;
    invoke<ConversationHistoryEntry[]>("ma_conversation_history", { projectId }).then(rows => {
      if (current) { setEntries(rows); setError(""); }
    }, reason => { if (current) setError(String(reason)); });
    return () => { current = false; };
  }, [projectId, currentId, currentTitle, revision, attempt]);
  const recent = entries.filter(entry => !entry.legacy && entry.id !== currentId).slice(0, 4);
  if (!fullWidth && !recent.length && !error) return null;
  return <nav className="conversation-topic-strip" aria-label={t("最近话题")}>
    <div>{[{ id: currentId, title: currentTitle }, ...recent.map(entry => ({ id: entry.id, title: conversationTitle(entry.title, entry.preview) }))].map(entry =>
      <button type="button" key={entry.id} aria-current={currentId === entry.id ? "page" : undefined}
        disabled={busy && entry.id !== currentId} title={entry.title} onClick={() => { if (entry.id !== currentId) void onSwitch(entry.id); }}>
        <span>{entry.title === "空白会话" ? t("空白会话") : entry.title}</span>
      </button>)}</div>
    <button type="button" title={t("新建话题")} aria-label={t("新建话题")} disabled={busy} onClick={() => void onCreate()}><Plus size={15} /></button>
    {error && <button type="button" title={error} onClick={() => setAttempt(value => value + 1)}>{t("重新读取话题")}</button>}
  </nav>;
}

interface Props {
  projectId: string;
  currentId: string;
  currentTitle: string;
  blocked: boolean;
  busy: boolean;
  memberCount: (id: string) => number;
  onSwitch: (id: string) => Promise<boolean>;
  onRename: (id: string, label: string) => void;
}

export function ConversationSwitcher({ projectId, currentId, currentTitle, blocked, busy, memberCount, onSwitch, onRename }: Props) {
  const [open, setOpen] = useState(false);
  const [rowMenu, setRowMenu] = useState<{ id: string; label: string; top: number } | null>(null);
  const [entries, setEntries] = useState<ConversationHistoryEntry[]>([]);
  const [readError, setReadError] = useState("");
  const [loading, setLoading] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const rowTrigger = useRef<HTMLButtonElement | null>(null);
  const renameButton = useRef<HTMLButtonElement>(null);
  const listId = useId();
  const rowMenuId = useId();

  useEffect(() => {
    if (!open) return;
    let ignore = false;
    setEntries([]);
    setReadError("");
    setLoading(true);
    void invoke<ConversationHistoryEntry[]>("ma_conversation_history", { projectId }).then(
      value => { if (!ignore) { setEntries(value); setLoading(false); } },
      reason => { if (!ignore) { setReadError(String(reason)); setLoading(false); } },
    );
    return () => { ignore = true; };
  }, [open, projectId, currentId, attempt]);

  useEffect(() => {
    if (!open) return;
    const outside = (event: Event) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) {
        setOpen(false);
        setRowMenu(null);
      }
    };
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("focusin", outside);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("focusin", outside);
    };
  }, [open]);

  useEffect(() => { if (rowMenu) renameButton.current?.focus(); }, [rowMenu]);

  const rows = [
    { id: currentId, label: currentTitle },
    ...entries.filter(entry => !entry.legacy && entry.id !== currentId).map(entry => ({
      id: entry.id, label: conversationTitle(entry.title, entry.preview),
    })),
  ];

  return <div ref={root} className="conversation-switcher" onKeyDown={event => {
    if (event.key !== "Escape" || event.nativeEvent.isComposing || !open) return;
    event.preventDefault();
    event.stopPropagation();
    if (rowMenu) { setRowMenu(null); rowTrigger.current?.focus(); }
    else { setOpen(false); trigger.current?.focus(); }
  }}>
    <button ref={trigger} type="button" className="conversation-switch-trigger" aria-label={`切换会话：${currentTitle}`} aria-expanded={open} aria-controls={open ? listId : undefined} title={currentTitle} onClick={() => { setOpen(!open); setRowMenu(null); }}>
      <ChatCircleText size={17} /><span>{currentTitle}</span><CaretDown size={13} />
    </button>
    {open && <div ref={popup} id={listId} className="conversation-switch-popover" aria-label="切换会话">
      <ul onScroll={() => setRowMenu(null)}>
        {rows.map(({ id, label }) => {
          const current = id === currentId;
          return <li key={id} data-current={current}>
            <button type="button" className="conversation-switch-row" aria-current={current ? "true" : undefined} title={label}
              disabled={!current && blocked} onClick={async () => {
                if (current) { setOpen(false); setRowMenu(null); trigger.current?.focus(); }
                else if (await onSwitch(id)) { setOpen(false); setRowMenu(null); trigger.current?.focus(); }
              }}>
              <ChatCircleText size={16} /><span className="conversation-switch-name">{label}</span>
              <small>已选 {memberCount(id)} 位</small><span className="conversation-switch-check">{current && <Check size={16} weight="bold" />}</span>
            </button>
            <button type="button" className="conversation-icon conversation-switch-more" aria-label={`${label}的会话操作`} aria-haspopup="menu" aria-expanded={rowMenu?.id === id}
              aria-controls={rowMenu?.id === id ? rowMenuId : undefined} disabled={busy}
              onClick={event => {
                if (rowMenu?.id === id) { setRowMenu(null); return; }
                if (!popup.current) return;
                rowTrigger.current = event.currentTarget;
                const button = event.currentTarget.getBoundingClientRect();
                const menu = popup.current.getBoundingClientRect();
                setRowMenu({ id, label, top: button.top - menu.top - 3 });
              }}><DotsThree size={19} /></button>
          </li>;
        })}
      </ul>
      {rowMenu && <div id={rowMenuId} className="conversation-history-menu conversation-switch-row-menu" role="menu" aria-label={`${rowMenu.label}的会话操作`}
        style={{ top: rowMenu.top }}>
        <button ref={renameButton} type="button" role="menuitem" onClick={() => {
          setOpen(false); setRowMenu(null); trigger.current?.focus(); onRename(rowMenu.id, rowMenu.label);
        }}><PencilSimple size={16} />重命名会话</button>
      </div>}
      {loading && <p role="status">正在读取历史会话…</p>}
      {readError && <p role="alert">读取历史会话失败。<button type="button" className="conversation-text-button" onClick={() => setAttempt(value => value + 1)}>重试</button></p>}
    </div>}
  </div>;
}
