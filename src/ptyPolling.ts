import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { PollResult, SessionRecord } from "./types";
import { sessionDisplayTitle } from "./sessionCapabilities";

// pty_poll_all 消费全局输出队列。同一窗口只有一个消费者，所有终端共用分发。
const subscribers = new Set<(sessions: SessionRecord[]) => void>();
let timer: ReturnType<typeof setInterval> | null = null;
let polling = false;
let signature = "";

export function subscribePtyPolling(onSessions: (sessions: SessionRecord[]) => void): () => void {
  // 每个订阅实例独立，两个视图传入同一 callback 也不能互相取消。
  const subscriber = (sessions: SessionRecord[]) => onSessions(sessions);
  subscribers.add(subscriber);
  if (timer === null) {
    signature = "";
    timer = setInterval(async () => {
      if (polling) return;
      polling = true;
      try {
        const result = await invoke<PollResult>("pty_poll_all");
        window.dispatchEvent(new CustomEvent("pty:chunk", { detail: result.chunks }));
        const next = result.sessions.map(session => [session.id, session.status, session.ptyId,
          sessionDisplayTitle(session), session.updatedAt]).flat().join("|");
        if (next !== signature) {
          signature = next;
          for (const notify of subscribers) notify(result.sessions);
        }
      } catch (error) {
        console.error("poll_all 失败:", error);
      } finally {
        polling = false;
      }
    }, 30);
  }
  return () => {
    subscribers.delete(subscriber);
    if (subscribers.size === 0 && timer !== null) {
      clearInterval(timer);
      timer = null;
    }
  };
}

export function usePtyPolling(onSessions: (sessions: SessionRecord[]) => void, enabled: boolean): void {
  useEffect(() => enabled ? subscribePtyPolling(onSessions) : undefined, [onSessions, enabled]);
}
