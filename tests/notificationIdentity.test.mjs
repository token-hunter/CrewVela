import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { normalizeNotificationTitle } from "../src/brand.ts";

const center = readFileSync(new URL("../src/notificationCenter.ts", import.meta.url), "utf8");
const historyPanel = readFileSync(new URL("../src/components/NotificationSettingsPanel.tsx", import.meta.url), "utf8");
const main = readFileSync(new URL("../src/main.tsx", import.meta.url), "utf8");
const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");

test("Windows 使用自定义通知窗，不再显示系统应用标题栏", () => {
  assert.match(center, /show_notification_overlay/);
  assert.doesNotMatch(center, /send_windows_unpacked_notification/);
  assert.match(main, /view === "notification"/);
  assert.match(lib, /NOTIFICATION_WINDOW_LABEL/);
  assert.match(lib, /decorations\(false\)/);
});

test("通知标题清除重复的 CrewVela 品牌前缀并保留用户标题", () => {
  assert.match(center, /normalizeNotificationTitle/);
  assert.equal(normalizeNotificationTitle("CrewVela · 协帆 · CrewVela 任务完成"), "任务完成");
  assert.equal(normalizeNotificationTitle("CrewVela · 协帆"), "通知");
  assert.equal(normalizeNotificationTitle("通知来自 CrewVela 社区"), "通知来自 CrewVela 社区");
  assert.equal(normalizeNotificationTitle("CrewVelaX 任务"), "CrewVelaX 任务");
  assert.match(center, /title: displayTitle/);
  assert.match(historyPanel, /normalizeNotificationTitle\(record\.title\)/);
});

test("非 Windows 平台继续使用 Tauri 通知插件", () => {
  assert.match(center, /ensurePermission/);
  assert.match(center, /sendNotification/);
  assert.match(center, /if \(windows\)[\s\S]*show_notification_overlay/);
});
