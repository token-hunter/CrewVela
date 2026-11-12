import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { BellIcon } from "@phosphor-icons/react/dist/csr/Bell";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import { Modal } from "./ui/Modal";
import type { SessionRecord } from "../types";
import { sessionDisplayTitle, sessionDisplayProvider } from "../sessionCapabilities";
import { ATTENTION_DISMISSED_KEY, attentionSummary, readDismissedAttention, sessionAttentionItems, type AttentionKind } from "../sidebarAttention";
import "../styles/sidebar-attention.css";

export function SidebarAttention({ sessions, onOpenSession }: {
  sessions: readonly SessionRecord[];
  onOpenSession: (session: SessionRecord, kind: AttentionKind) => void;
}) {
  const [open, setOpen] = useState(false);
  const [dismissed, setDismissed] = useState<string[]>(() => {
    try { return readDismissedAttention(window.localStorage.getItem(ATTENTION_DISMISSED_KEY)); }
    catch { return []; }
  });
  const [notice, setNotice] = useState("");
  const items = sessionAttentionItems(sessions, dismissed);
  const summary = attentionSummary(items);
  useEffect(() => { if (!items.length) setOpen(false); }, [items.length]);
  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key !== ATTENTION_DISMISSED_KEY && event.key !== null) return;
      try { setDismissed(readDismissedAttention(window.localStorage.getItem(ATTENTION_DISMISSED_KEY))); }
      catch { setNotice("无法读取忽略记录，已保留待处理提醒。"); setDismissed([]); }
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);
  const dismiss = (key: string) => {
    // Re-read to retain another window's dismissals. Store only event keys,
    // never account data, paths, task text, or raw error contents.
    try {
      const next = [...new Set([...readDismissedAttention(window.localStorage.getItem(ATTENTION_DISMISSED_KEY)), key])].slice(-500);
      window.localStorage.setItem(ATTENTION_DISMISSED_KEY, JSON.stringify(next));
      setDismissed(next); setNotice("");
    } catch { setNotice("未能保存忽略记录，提醒仍保留。请检查本地存储后重试。"); }
  };
  if (!items.length) return null;
  return <>
    <button type="button" className="sidebar-attention-trigger" aria-label={`当前工作区待处理：${summary}`} title={`当前工作区：${summary}`} aria-haspopup="dialog" aria-expanded={open} onClick={() => { setNotice(""); setOpen(true); }}>
      <BellIcon size={15} aria-hidden="true" /><span>{summary}</span>
    </button>
    {open && createPortal(<Modal className="sidebar-attention-dialog" overlayClassName="dialog-overlay sidebar-attention-overlay" ariaLabel="当前工作区待处理" onClose={() => setOpen(false)} closeOnOverlayClick>
      <header><div><h2>待处理</h2><p>当前工作区 · {summary}</p></div><button type="button" className="sidebar-attention-close" aria-label="关闭待处理" onClick={() => setOpen(false)}><XIcon size={18} /></button></header>
      <div className="sidebar-attention-list">
        {items.map(({ session, kind, key }) => <article key={key}>
          <span className={`sidebar-attention-kind ${kind}`}>{kind === "approval" ? "待授权" : "执行失败"}</span>
          <strong>{sessionDisplayTitle(session)}</strong><small>{sessionDisplayProvider(session)}</small>
          <p>{kind === "approval" ? "请进入会话核对并处理授权请求。" : "查看错误与历史记录，不会自动重试任务。"}</p>
          <div className="sidebar-attention-actions"><button type="button" className="btn secondary" onClick={() => { onOpenSession(session, kind); setOpen(false); }}>{kind === "approval" ? "前往会话" : "查看失败记录"}</button>
          {kind === "error" && <button type="button" className="btn ghost" aria-label={`忽略 ${sessionDisplayTitle(session)} 本次失败`} onClick={() => dismiss(key)}>忽略本次</button>}</div>
        </article>)}
      </div>
      {notice && <p className="sidebar-attention-notice" role="alert">{notice}</p>}
      <footer>状态恢复后自动移除；忽略本次失败不会删除会话，新一轮失败仍会提醒。</footer>
    </Modal>, document.body)}
  </>;
}
