import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ConversationHistoryEntry } from "../types";
import { Modal } from "./ui/Modal";

const date = new Intl.DateTimeFormat("zh-CN", {
  year: "numeric", month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit", hour12: false,
});

interface Props {
  projectId: string;
  conversationId: string;
  blocked: boolean;
  busy: boolean;
  switchError: string;
  onSwitch: (id: string) => void;
  onClose: () => void;
}

export function ConversationHistoryDialog({ projectId, conversationId, blocked, busy, switchError, onSwitch, onClose }: Props) {
  const [entries, setEntries] = useState<ConversationHistoryEntry[] | null>(null);
  const [readError, setReadError] = useState("");
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let ignore = false;
    setEntries(null);
    setReadError("");
    void invoke<ConversationHistoryEntry[]>("ma_conversation_history", { projectId }).then(
      value => { if (!ignore) setEntries(value); },
      reason => { if (!ignore) setReadError(String(reason)); },
    );
    return () => { ignore = true; };
  }, [projectId, conversationId, attempt]);

  const renderEntries = (items: ConversationHistoryEntry[]) => <ul className="conversation-history-list">
    {items.map(item => {
      const current = item.id === conversationId;
      const time = item.lastMessageAt ?? item.createdAt;
      return <li key={item.id}>
        <button type="button" disabled={blocked || current} aria-current={current ? "true" : undefined} onClick={() => onSwitch(item.id)}>
          <strong>{item.preview?.replace(/\s+/g, " ").trim() || "空白会话"}</strong>
          <span><time dateTime={new Date(time).toISOString()}>{date.format(time)}</time>{current && " · 当前会话"}</span>
        </button>
      </li>;
    })}
  </ul>;
  const legacy = entries?.filter(item => item.legacy) ?? [];
  return <Modal className="conversation-clear-dialog conversation-history-dialog" ariaLabel="历史会话" onClose={onClose}>
    <h2>历史会话</h2>
    {readError ? <p role="alert">读取历史会话失败。<button type="button" onClick={() => setAttempt(value => value + 1)}>重试</button></p>
      : !entries ? <p role="status">正在读取历史会话…</p> : <>
        {renderEntries(entries.filter(item => !item.legacy))}
        {legacy.length > 0 && <details className="conversation-legacy-history">
          <summary>查看旧版保留记录（{legacy.length}）</summary>
          <p>旧版的新建和清空使用同一记录格式，列表可能包含曾清空的会话。</p>
          {renderEntries(legacy)}
        </details>}
      </>}
    {blocked && !busy && !switchError && <p role="status">请等本轮讨论结束或发送结果确认后再切换。</p>}
    {switchError && <p className="conversation-clear-error" role="alert">{switchError}</p>}
    <div><button type="button" disabled={busy} onClick={onClose}>关闭</button></div>
  </Modal>;
}
