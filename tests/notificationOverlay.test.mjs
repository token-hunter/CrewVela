import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { readStyles } from "./helpers/loadStyles.mjs";

const main = readFileSync(new URL("../src/main.tsx", import.meta.url), "utf8");
const overlay = readFileSync(
  new URL("../src/components/NotificationOverlay.tsx", import.meta.url),
  "utf8",
);
const center = readFileSync(
  new URL("../src/notificationCenter.ts", import.meta.url),
  "utf8",
);
const backend = readFileSync(
  new URL("../src-tauri/src/lib.rs", import.meta.url),
  "utf8",
);
const styles = readStyles();

test("Windows 通知通过单实例 Tauri 悬浮窗显示", () => {
  assert.match(center, /show_notification_overlay/);
  assert.doesNotMatch(center, /send_windows_unpacked_notification/);
  assert.match(backend, /NOTIFICATION_WINDOW_LABEL/);
  assert.match(backend, /WebviewUrl::App\("index\.html#notification"/);
  assert.match(backend, /always_on_top\(true\)/);
  assert.match(backend, /decorations\(false\)/);
  assert.match(backend, /transparent\(true\)/);
  assert.match(backend, /skip_taskbar\(true\)/);
  assert.match(backend, /notification_overlay_ready/);
  assert.match(backend, /notification_initialization_script/);
  assert.match(backend, /\.initialization_script\(initialization_script\)/);
  assert.match(
    backend,
    /position_notification_window\(&window\)\?[\s\S]*window\.show\(\)/,
  );
});

test("方案 3 保留橙子、查看操作和自动关闭进度", () => {
  assert.match(main, /view === "notification"/);
  assert.match(main, /window\.location\.hash/);
  assert.match(overlay, /notification-orange-mascot\.png/);
  assert.match(overlay, /任务完成/);
  assert.match(overlay, /查看/);
  assert.match(overlay, /close_notification_overlay/);
  assert.match(styles, /notification-overlay-progress/);
  assert.match(overlay, /AUTO_DISMISS_MS/);
});

test("通知窗支持更新、聚焦主应用和打开原链接", () => {
  assert.match(overlay, /notification-overlay-updated/);
  assert.match(overlay, /focus_main_from_notification/);
  assert.match(overlay, /openUrl/);
  assert.match(backend, /notification_overlay_current/);
});

test("首次渲染不在 WebView 创建阶段重入 Tauri IPC", () => {
  assert.match(overlay, /readInitialNotice/);
  assert.match(overlay, /__CREWVELA_NOTIFICATION__/);
  assert.match(backend, /async fn show_notification_overlay/);
  assert.match(backend, /\.visible\(false\)/);
  assert.match(backend, /monitor\.work_area\(\)/);
  assert.doesNotMatch(overlay, /window\.requestAnimationFrame/);
  assert.match(overlay, /notification_overlay_ready/);
  assert.match(overlay, /setTimeout\(subscribe/);
});
