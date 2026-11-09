import test from "node:test";
import assert from "node:assert/strict";

import {
  appendNotificationHistory,
  clearNotificationHistory,
  loadNotificationHistory,
  markAllNotificationsRead,
  NOTIFICATION_HISTORY_STORAGE_KEY,
} from "../src/notificationHistory.ts";

function createHarness(t, { failInvoke = false } = {}) {
  const values = new Map();
  const records = new Map();
  const calls = [];
  const localStorage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    removeItem: (key) => values.delete(key),
  };
  const window = {
    dispatchEvent: () => true,
    __TAURI_INTERNALS__: {
      invoke: async (command, args = {}) => {
        calls.push(command);
        if (failInvoke) throw new Error("Tauri is unavailable");

        if (command === "notification_history_list") return [...records.values()].map((item) => ({ ...item }));
        if (command === "notification_history_import") {
          for (const record of args.records) {
            if (!records.has(record.id)) records.set(record.id, record);
          }
          return [...records.values()];
        }
        if (command === "notification_history_append") {
          if (!records.has(args.record.id)) records.set(args.record.id, args.record);
          return [...records.values()];
        }
        if (command === "notification_history_mark_all_read") {
          for (const [id, record] of records) records.set(id, { ...record, read: true });
          return [...records.values()];
        }
        if (command === "notification_history_clear") {
          records.clear();
          return undefined;
        }
        throw new Error(`Unexpected Tauri command: ${command}`);
      },
    },
  };
  const previous = {
    window: globalThis.window,
    localStorage: globalThis.localStorage,
    CustomEvent: globalThis.CustomEvent,
  };
  globalThis.window = window;
  globalThis.localStorage = localStorage;
  globalThis.CustomEvent = class {
    constructor(type, options) {
      this.type = type;
      this.detail = options?.detail;
    }
  };
  t.after(() => {
    for (const key of Object.keys(previous)) {
      if (previous[key] === undefined) delete globalThis[key];
      else globalThis[key] = previous[key];
    }
  });

  return { calls, localStorage };
}

test("persists the complete notification body through the async history API", async (t) => {
  createHarness(t);
  const notice = {
    id: "quota:1",
    title: "Codex 全局额度动态",
    body: "额度已经重置，可以继续使用。",
    category: "quota",
    createdAt: 100,
  };

  await appendNotificationHistory(notice);

  assert.deepEqual(await loadNotificationHistory(), [{ ...notice, read: false }]);
});

test("migrates legacy records before appending or marking them read", async (t) => {
  const { calls, localStorage } = createHarness(t);
  const notice = {
    id: "quota:repeat",
    title: "Codex 账号额度已恢复",
    body: "已用额度从 95% 降至 0%。",
    category: "quota",
    createdAt: 200,
    read: false,
  };
  localStorage.setItem(NOTIFICATION_HISTORY_STORAGE_KEY, JSON.stringify([notice]));

  await appendNotificationHistory(notice);
  await appendNotificationHistory(notice);
  await markAllNotificationsRead();

  assert.deepEqual(await loadNotificationHistory(), [{ ...notice, read: true }]);
  assert.equal(localStorage.getItem(NOTIFICATION_HISTORY_STORAGE_KEY), null);
  assert.deepEqual(calls.slice(0, 4), [
    "notification_history_import",
    "notification_history_append",
    "notification_history_append",
    "notification_history_mark_all_read",
  ]);
});

test("successful clear removes legacy data so the next load cannot restore it", async (t) => {
  const { calls, localStorage } = createHarness(t);
  localStorage.setItem(NOTIFICATION_HISTORY_STORAGE_KEY, JSON.stringify([{
    id: "legacy:1",
    title: "旧通知",
    body: "已经清空",
    category: "general",
    createdAt: 300,
    read: false,
  }]));

  assert.deepEqual(await clearNotificationHistory(), []);

  assert.equal(localStorage.getItem(NOTIFICATION_HISTORY_STORAGE_KEY), null);
  assert.deepEqual(await loadNotificationHistory(), []);
  assert.deepEqual(calls, ["notification_history_clear", "notification_history_list"]);
});

test("falls back to localStorage when Tauri is unavailable", async (t) => {
  createHarness(t, { failInvoke: true });
  delete globalThis.window.__TAURI_INTERNALS__;
  const notice = {
    id: "quota:fallback",
    title: "额度通知",
    body: "仍可在浏览器预览中查看。",
    category: "quota",
    createdAt: 400,
  };

  await appendNotificationHistory(notice);
  await markAllNotificationsRead();

  assert.deepEqual(await loadNotificationHistory(), [{ ...notice, read: true }]);
  assert.deepEqual(await clearNotificationHistory(), []);
});

test("native mutation failures preserve local history and never publish false success", async (t) => {
  const { localStorage } = createHarness(t, { failInvoke: true });
  const notice = { id: "pending", title: "保留通知", body: "未读", category: "general", createdAt: 400, read: false };
  const stored = JSON.stringify([notice]);
  localStorage.setItem(NOTIFICATION_HISTORY_STORAGE_KEY, stored);
  const events = []; globalThis.window.dispatchEvent = event => events.push(event);
  await assert.rejects(clearNotificationHistory(), /Tauri is unavailable/);
  await assert.rejects(markAllNotificationsRead(), /Tauri is unavailable/);
  assert.equal(localStorage.getItem(NOTIFICATION_HISTORY_STORAGE_KEY), stored);
  assert.deepEqual(events, []);
});
