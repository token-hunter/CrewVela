import type { SessionRecord, SessionStatus } from "./types";
import { deliverDesktopNotice } from "./notificationCenter";
import { loadNotificationSettings } from "./notificationSettings";
import { PRODUCT_FULL_NAME } from "./brand";
import { getInterfaceLanguage, translateCurrent } from "./interfaceLanguage";
import { localizeError } from "./localizedError";

const NOTIFY_STATUSES: ReadonlySet<SessionStatus> = new Set(["waiting", "completed", "error"]);
const notified = new Map<string, SessionStatus>();

export function notifySessionTransitions(previous: SessionRecord[], next: SessionRecord[]): void {
  const settings = loadNotificationSettings();
  if (!settings.enabled) return;
  const previousById = new Map(previous.map((session) => [session.id, session]));
  const nextIds = new Set(next.map((session) => session.id));
  for (const sessionId of [...notified.keys()]) if (!nextIds.has(sessionId)) notified.delete(sessionId);

  // 首次加载只建立状态基线，避免恢复历史会话时集中弹窗。
  if (previous.length === 0) {
    for (const session of next) if (NOTIFY_STATUSES.has(session.status)) notified.set(session.id, session.status);
    return;
  }

  for (const session of next) {
    const before = previousById.get(session.id);
    if (before?.status === session.status || notified.get(session.id) === session.status) continue;
    if (!NOTIFY_STATUSES.has(session.status)) {
      notified.delete(session.id);
      continue;
    }
    notified.set(session.id, session.status);
    if (settings.suppressWhenFocused && document.hasFocus()) continue;
    if (session.status === "completed" && !settings.taskComplete) continue;
    if (session.status === "error" && !settings.taskFailed) continue;
    if (session.status === "waiting" && !settings.waitingForUser) continue;
    const status = session.status === "completed" ? "任务完成" : session.status === "error" ? "执行失败" : "等待处理";
    const title = `${PRODUCT_FULL_NAME} · ${translateCurrent(status)}`;
    const body = session.status === "error" && session.lastError
      ? `${session.title} · ${localizeError(getInterfaceLanguage(), session.lastError).message.slice(0, 120)}`
      : session.title;
    void deliverDesktopNotice({ id: `session:${session.id}:${session.status}`, title, body, category: "task" });
  }
}
