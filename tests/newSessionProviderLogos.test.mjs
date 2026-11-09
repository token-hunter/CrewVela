import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const dialogSource = readFileSync(
  new URL("../src/components/NewSessionDialog.tsx", import.meta.url),
  "utf8",
);
const settingsSource = readFileSync(
  new URL("../src/components/SettingsDialog.tsx", import.meta.url),
  "utf8",
);
const logosSource = readFileSync(
  new URL("../src/providerLogos.ts", import.meta.url),
  "utf8",
);

test("新建会话和模型设置复用同一套 Provider Logo", () => {
  assert.match(dialogSource, /PROVIDER_LOGOS, providerTone/);
  assert.match(settingsSource, /PROVIDER_LOGOS, providerTone/);
  assert.match(dialogSource, /<img src=\{logo\} alt="" aria-hidden="true" \/>/);
  assert.match(logosSource, /NEUTRAL_AGENT_ICONS\.kimi/);
  assert.match(logosSource, /NEUTRAL_AGENT_ICONS\.zcode/);
  assert.match(logosSource, /NEUTRAL_AGENT_ICONS\.deepseek/);
  assert.match(logosSource, /NEUTRAL_AGENT_ICONS\.minimax/);
});
