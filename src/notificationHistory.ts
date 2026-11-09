import { invoke } from "@tauri-apps/api/core";

export const NOTIFICATION_HISTORY_STORAGE_KEY = "crewvela-notification-history-v1";
export const NOTIFICATION_HISTORY_EVENT = "crewvela:notification-history";
export const NOTIFICATION_HISTORY_RETENTION_DAYS = 30;

export type NotificationCategory = "task" | "intervention" | "terminal" | "quota" | "test" | "general";

export interface NotificationHistoryRecord {
  id: string;
  title: string;
  body: string;
  category: NotificationCategory;
  createdAt: number;
  read: boolean;
  url?: string;
}

export interface NotificationHistoryInput extends Omit<NotificationHistoryRecord, "read"> {
  read?: boolean;
}

function emitHistoryChanged(records: NotificationHistoryRecord[]): void {
  window.dispatchEvent(new CustomEvent(NOTIFICATION_HISTORY_EVENT, { detail: records }));
}

function legacyHistory(): NotificationHistoryRecord[] {
  try {
    const value = JSON.parse(localStorage.getItem(NOTIFICATION_HISTORY_STORAGE_KEY) ?? "[]");
    return Array.isArray(value) ? value : [];
  } catch {
    return [];
  }
}

async function migrateLegacyHistory(): Promise<NotificationHistoryRecord[] | undefined> {
  const legacy = legacyHistory();
  if (legacy.length === 0) return undefined;

  const migrated = await invoke<NotificationHistoryRecord[]>("notification_history_import", { records: legacy });
  localStorage.removeItem(NOTIFICATION_HISTORY_STORAGE_KEY);
  return migrated;
}

export async function loadNotificationHistory(): Promise<NotificationHistoryRecord[]> {
  try {
    const migrated = await migrateLegacyHistory();
    if (migrated !== undefined) {
      emitHistoryChanged(migrated);
      return migrated;
    }
    return await invoke<NotificationHistoryRecord[]>("notification_history_list");
  } catch {
    return legacyHistory();
  }
}

export async function appendNotificationHistory(input: NotificationHistoryInput): Promise<NotificationHistoryRecord[]> {
  const record: NotificationHistoryRecord = { ...input, read: input.read ?? false };
  try {
    await migrateLegacyHistory();
    const records = await invoke<NotificationHistoryRecord[]>("notification_history_append", { record });
    emitHistoryChanged(records);
    return records;
  } catch {
    const records = legacyHistory();
    const next = records.some((item) => item.id === record.id) ? records : [record, ...records];
    localStorage.setItem(NOTIFICATION_HISTORY_STORAGE_KEY, JSON.stringify(next));
    emitHistoryChanged(next);
    return next;
  }
}

export async function markAllNotificationsRead(): Promise<NotificationHistoryRecord[]> {
  try {
    await migrateLegacyHistory();
    const records = await invoke<NotificationHistoryRecord[]>("notification_history_mark_all_read");
    emitHistoryChanged(records);
    return records;
  } catch (error) {
    if ("__TAURI_INTERNALS__" in window) throw error;
    const records = legacyHistory().map((item) => ({ ...item, read: true }));
    localStorage.setItem(NOTIFICATION_HISTORY_STORAGE_KEY, JSON.stringify(records));
    emitHistoryChanged(records);
    return records;
  }
}

export async function clearNotificationHistory(): Promise<NotificationHistoryRecord[]> {
  try {
    await invoke("notification_history_clear");
  } catch (error) {
    if ("__TAURI_INTERNALS__" in window) throw error;
    // Browser preview still clears its local copy.
  }
  localStorage.removeItem(NOTIFICATION_HISTORY_STORAGE_KEY);
  emitHistoryChanged([]);
  return [];
}
