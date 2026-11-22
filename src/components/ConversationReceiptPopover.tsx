import { CaretDown, CheckCircle, Clock, ArrowSquareOut } from "@phosphor-icons/react";
import { useEffect, useId, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "../styles/session-evidence.css";

interface Receipt {
  id: string;
  sender: string;
  target: string;
  body: string;
  queuedAt: number;
  submittedAt: number | null;
  receivedAt: number | null;
  replyAt: number | null;
  reply: string | null;
  stage: string;
}

const labels: Record<string, string> = {
  queued: "已排队", attempting: "投递待核实", publishing: "投递待核实",
  submitted: "输入已提交", received: "原会话已接收", rejected: "未投递",
  replied: "已收到回复", reply_recorded: "对方回复已登记",
};

function ReceiptPopover({ receipt, onOpen }: { receipt: Receipt; onOpen?: () => void }) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!open) return;
    const pointer = (event: PointerEvent) => {
      if (event.target instanceof Node && !anchor.current?.contains(event.target)) setOpen(false);
    };
    const keyboard = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        setOpen(false);
        trigger.current?.focus();
      }
    };
    document.addEventListener("pointerdown", pointer);
    document.addEventListener("keydown", keyboard, true);
    return () => {
      document.removeEventListener("pointerdown", pointer);
      document.removeEventListener("keydown", keyboard, true);
    };
  }, [open]);
  const verified = receipt.stage === "received" || receipt.stage === "replied";
  const stages: [string, number | null][] = [
    [receipt.stage === "publishing" ? "请求已登记" : "已排队", receipt.queuedAt],
    [labels.submitted, receipt.submittedAt],
    [labels.received, receipt.receivedAt],
    [receipt.stage === "replied" ? labels.replied : labels.reply_recorded, receipt.replyAt],
  ];
  return <div className="session-receipt-anchor" ref={anchor}>
    <button type="button" ref={trigger} className={`session-receipt-trigger${verified ? " session-evidence-ok" : ""}`}
      aria-expanded={open} aria-controls={id} onClick={() => setOpen(!open)}>
      {verified ? <CheckCircle size={15} /> : <Clock size={15} />}{labels[receipt.stage] ?? "投递待核实"}<CaretDown size={12} />
    </button>
    {open && <section className="session-receipt-popover" id={id} aria-label="投递回执">
      <strong>投递回执</strong>
      <ol>{stages.filter(([, at]) => at != null).map(([label, at]) => <li key={label}>
        <span>{label}</span><time>{new Date(at!).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" })}</time>
      </li>)}</ol>
      {receipt.reply && <p>{receipt.reply}</p>}
      {onOpen && <button type="button" className="communication-text-button" onClick={onOpen}>
        <ArrowSquareOut size={15} />查看原会话
      </button>}
    </section>}
  </div>;
}

export function SessionMessageReceipts({ sessionId, sessionName, onOpenSession }: {
  sessionId: string;
  sessionName: (id: string) => string | undefined;
  onOpenSession?: (id: string) => void;
}) {
  const [result, setResult] = useState<{ id: string; rows: Receipt[]; error?: string }>();
  useEffect(() => {
    let active = true;
    let pending = false;
    async function refresh() {
      if (pending) return;
      pending = true;
      try {
        const rows = await invoke<Receipt[]>("list_session_message_receipts", { sessionId });
        if (active) setResult({ id: sessionId, rows });
      } catch (error) {
        if (active) setResult({ id: sessionId, rows: [], error: String(error) });
      } finally { pending = false; }
    }
    void refresh();
    const timer = setInterval(() => void refresh(), 2500);
    return () => { active = false; clearInterval(timer); };
  }, [sessionId]);
  if (result?.id !== sessionId || (!result.rows.length && !result.error)) return null;
  return <section className="session-receipt-section" aria-label="最近通信">
    <h3>最近通信</h3>
    {result.error && <p role="alert">无法读取投递回执：{result.error}</p>}
    {result.rows.map(receipt => {
      const peer = receipt.sender === sessionId ? receipt.target : receipt.sender;
      const title = sessionName(peer);
      return <div className="session-receipt-message" key={receipt.id}>
        <div><small>{receipt.sender === sessionId ? "发给" : "来自"} {title ?? "原会话"}</small><p>{receipt.body}</p></div>
        <ReceiptPopover receipt={receipt} onOpen={onOpenSession && title ? () => onOpenSession(peer) : undefined} />
      </div>;
    })}
  </section>;
}
