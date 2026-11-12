import { sessionDisplayTitle } from "../sessionCapabilities";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SessionRecord } from "../types";
import { Modal } from "./ui/Modal";

interface TranscriptMessage {
  role: string;
  text: string;
  at: string | null;
}

interface Props {
  session: SessionRecord;
  onClose: () => void;
  preview?: boolean;
  relayHistory?: boolean;
  embedded?: boolean;
}

const ROLE_LABELS: Record<string, string> = {
  user: "用户",
  assistant: "Agent",
  system: "交接线索",
  tool: "工具",
};

function formatTime(iso: string | null): string {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleTimeString("zh-CN", { hour12: false });
}

/** 历史会话内容回看：渲染 claude transcript 的消息流（尾部 200 条）。 */
export function TranscriptViewer({ session, onClose, preview = false, relayHistory = false, embedded = false }: Props) {
  const [messages, setMessages] = useState<TranscriptMessage[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    setMessages(null);
    setError(null);
    const load = preview ? Promise.resolve<TranscriptMessage[]>([{ role: "user", text: "界面演示：继续开发当前项目，保留已有进度。", at: null }, { role: "assistant", text: "界面演示：原 Agent 历史保留在这里，实际内容由原生应用读取。", at: null }])
      : invoke<TranscriptMessage[]>(relayHistory ? "session_switch_history" : "session_transcript", { sessionId: session.id });
    load
      .then((result) => {
        if (!disposed) setMessages(result);
      })
      .catch((reason) => {
        if (!disposed) setError(String(reason));
      });
    return () => {
      disposed = true;
    };
  }, [session.id, preview, relayHistory]);

  const content = <>
      <h2>
          {sessionDisplayTitle(session)}
          <small className="transcript-subtitle">历史记录（每段最多 200 条）</small>
        </h2>
        {session.status === "error" && <p className="session-error" role="alert">{session.lastError || "本次执行失败，未记录具体错误。查看记录不会自动重试任务。"}</p>}
        <div className="transcript-messages">
          {error && <div className="transcript-empty">{error}</div>}
          {!error && messages === null && (
            <div className="transcript-empty">正在读取会话记录…</div>
          )}
          {!error && messages !== null && messages.length === 0 && (
            <div className="transcript-empty">记录为空（会话可能还没产生对话）</div>
          )}
          {messages?.map((message, index) => (
            <div
              key={index}
              className={`transcript-message ${message.role}`}
            >
              <div className="transcript-message-head">
                <span>{ROLE_LABELS[message.role] ?? message.role}</span>
                {message.at && <time>{formatTime(message.at)}</time>}
              </div>
              <pre>{message.text}</pre>
            </div>
          ))}
        </div>
        {!embedded && <div className="actions">
          <button className="btn" onClick={onClose}>
            关闭
          </button>
        </div>}
  </>;
  return embedded ? <section className="handoff-transcript" aria-label="来源会话历史">{content}</section> : (
    <Modal overlayClassName="editor-overlay" className="editor transcript-viewer" onClose={onClose} closeOnOverlayClick>{content}</Modal>
  );
}
