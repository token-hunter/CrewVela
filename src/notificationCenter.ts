import { invoke } from "@tauri-apps/api/core";
import {
  isPermissionGranted,
  onAction,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { openUrl } from "@tauri-apps/plugin-opener";
import { normalizeNotificationTitle } from "./brand";
export { normalizeNotificationTitle } from "./brand";
import {
  loadNotificationSettings,
  playNotificationSound,
} from "./notificationSettings";
import {
  appendNotificationHistory,
  type NotificationCategory,
} from "./notificationHistory";

let permission: boolean | null = null;
let actionListenerStarted = false;

async function ensurePermission(): Promise<boolean> {
  if (permission !== null) return permission;
  try {
    permission = await isPermissionGranted();
    if (!permission) permission = (await requestPermission()) === "granted";
  } catch {
    permission = false;
  }
  return permission;
}

export async function initializeNotificationActions(): Promise<void> {
  if (actionListenerStarted || !("__TAURI_INTERNALS__" in window)) return;
  actionListenerStarted = true;
  try {
    await onAction((notification) => {
      const url = notification.extra?.url;
      if (typeof url === "string" && /^https:\/\/x\.com\//i.test(url))
        void openUrl(url);
    });
  } catch {
    actionListenerStarted = false;
  }
}

export interface DesktopNotice {
  id?: string;
  title: string;
  body: string;
  url?: string;
  force?: boolean;
  category?: NotificationCategory;
}

export async function deliverDesktopNotice(
  notice: DesktopNotice,
): Promise<boolean> {
  const settings = loadNotificationSettings();
  if (!settings.enabled && !notice.force) return false;
  const displayTitle = normalizeNotificationTitle(notice.title);
  await appendNotificationHistory({
    id:
      notice.id ??
      `notice:${Date.now()}:${Math.random().toString(36).slice(2, 9)}`,
    title: displayTitle,
    body: notice.body,
    category: notice.category ?? "general",
    createdAt: Date.now(),
    url: notice.url,
  });
  try {
    const windows = /Windows/i.test(navigator.userAgent);
    if (windows) {
      await invoke<boolean>("show_notification_overlay", {
        title: displayTitle,
        body: notice.body,
        url: notice.url ?? null,
      });
      playNotificationSound(settings);
      return true;
    }
    if (!(await ensurePermission())) return false;
    sendNotification({
      title: displayTitle,
      body: notice.body,
      sound: settings.sound === "system" ? "Default" : undefined,
      extra: notice.url ? { url: notice.url } : undefined,
      autoCancel: true,
    });
    playNotificationSound(settings);
    return true;
  } catch {
    return false;
  }
}
