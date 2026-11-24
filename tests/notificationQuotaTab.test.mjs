import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
const panelSource = readFileSync(
  new URL("../src/components/NotificationSettingsPanel.tsx", import.meta.url),
  "utf8",
);
const appSource = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
test("通知设置只提供提醒与历史，应用不再定时检查 Codex 额度", () => {
  assert.match(panelSource, />\{t\("通知设置"\)\}<\/button>/);
  assert.match(panelSource, />\{t\("通知历史"\)\}\{/);
  assert.doesNotMatch(panelSource, /CodexAccountsPanel|QuotaMonitorView|list_codex_quota_notifications/);
  assert.doesNotMatch(appSource, /check_codex_quota_updates|codexQuotaEnabled/);
});
