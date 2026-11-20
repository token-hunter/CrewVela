import { sessionDisplayTitle } from "../sessionCapabilities";
import { useEffect, useId, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ConversationOriginalSession, SessionRecord } from "../types";
import { Modal } from "./ui/Modal";

interface TranscriptMessage {
  role: string;
  text: string;
  at: string | null;
}

type Props = {
  onClose: () => void;
  preview?: boolean;
  relayHistory?: boolean;
  embedded?: boolean;
} & ({ session: SessionRecord; original?: never } | { original: ConversationOriginalSession; session?: never });

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

/** 历史会话内容回看（尾部 200 条）。 */
export function TranscriptViewer({ session, original, onClose, preview = false, relayHistory = false, embedded = false }: Props) {
  const titleId = useId();
  const [messages, setMessages] = useState<TranscriptMessage[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const sessionId = session?.id;
  const projectId = original?.projectId;
  const runId = original?.runId;

  useEffect(() => {
    let disposed = false;
    setMessages(null);
    setError(null);
    const load = runId ? invoke<TranscriptMessage[]>("ma_conversation_run_transcript", { projectId, runId })
      : preview ? Promise.resolve<TranscriptMessage[]>([{ role: "user", text: "界面演示：继续开发当前项目，保留已有进度。", at: null }, { role: "assistant", text: "界面演示：原 Agent 历史保留在这里，实际内容由原生应用读取。", at: null }])
      : invoke<TranscriptMessage[]>(relayHistory ? "session_switch_history" : "session_transcript", { sessionId });
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
  }, [sessionId, projectId, runId, preview, relayHistory]);

  const fallback = original && (error || messages?.length === 0) ? [
    { role: "user", text: original.request, at: null },
    { role: "assistant", text: original.reply, at: null },
  ].filter(item => item.text.trim()) : null;
  const runError = original?.error ?? (session?.status === "error" ? session.lastError || "本次执行失败，未记录具体错误。查看记录不会自动重试任务。" : null);
  const managedRecord = original?.source === "managed_task";

  const content = <>
      <h2 id={titleId}>
          {session ? sessionDisplayTitle(session) : `${original?.agentName} · 原会话`}
          <small className="transcript-subtitle">{managedRecord ? "只读记录" : "历史记录"}（每段最多 200 条）</small>
        </h2>
        {runError && <p className="session-error" role="alert">{runError}</p>}
        <div className="transcript-messages">
          {fallback && <div className="transcript-empty" role="status">
            原会话记录暂不可用。{fallback.length ? (managedRecord ? "以下为任务保存的本次记录。" : "以下为群聊保存的本次对话。") : "本次运行尚无可回看的对话。"}
          </div>}
          {error && !original && <div className="transcript-empty" role="alert">{error}</div>}
          {!error && messages === null && (
            <div className="transcript-empty">正在读取会话记录…</div>
          )}
          {!fallback && messages?.length === 0 && (
            <div className="transcript-empty">记录为空（会话可能还没产生对话）</div>
          )}
          {(fallback ?? messages)?.map((message, index) => (
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
    <Modal overlayClassName="editor-overlay" className="editor transcript-viewer" labelledBy={titleId} onClose={onClose} closeOnOverlayClick>{content}</Modal>
  );
}
