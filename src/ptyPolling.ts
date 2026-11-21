import { useEffect, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { PollResult, SessionRecord } from "./types";
import { sessionDisplayTitle } from "./sessionCapabilities";

// pty_poll_all 消费全局输出队列。同一窗口只有一个消费者，所有终端共用分发。
const subscribers = new Set<(sessions: SessionRecord[]) => void>();
let timer: ReturnType<typeof setInterval> | null = null;
let polling = false;
let signature = "";
let workingCliPtys = new Set<number>();
const cliWorkListeners = new Set<() => void>();

function setWorkingCliPtys(ids: number[]) {
  if (ids.length === workingCliPtys.size && ids.every(id => workingCliPtys.has(id))) return;
  workingCliPtys = new Set(ids);
  for (const listener of cliWorkListeners) listener();
}

function subscribeCliWork(listener: () => void) {
  cliWorkListeners.add(listener);
  return () => { cliWorkListeners.delete(listener); };
}

export function useCliPtyWorking(ptyId: number | null): boolean {
  return useSyncExternalStore(subscribeCliWork, () => ptyId !== null && workingCliPtys.has(ptyId));
}

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
        setWorkingCliPtys(result.workingCliPtyIds ?? []);
        window.dispatchEvent(new CustomEvent("pty:chunk", { detail: result.chunks }));
        const next = result.sessions.map(session => [session.id, session.status, session.ptyId,
          sessionDisplayTitle(session), session.updatedAt, session.backgroundChatHost]).flat().join("|");
        if (next !== signature) {
          signature = next;
          for (const notify of subscribers) notify(result.sessions);
        }
        if (result.communicationDeliveries?.length) {
          window.dispatchEvent(new CustomEvent("pty:communication-delivery", { detail: result.communicationDeliveries }));
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
