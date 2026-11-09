import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const panel = readFileSync(new URL("../src/components/NotificationSettingsPanel.tsx", import.meta.url), "utf8");

test("历史消息仅在有来源时提供入口，并打开该条消息记录的地址", () => {
  assert.match(panel, /record\.url && <button[^>]*onClick=\{\(\) => onOpenUrl\(record\.url!\)\}/);
  assert.doesNotMatch(panel, /TIBO_CODEX_UPDATES_URL|diagnostic\.latestUrl/);
});
