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

test("新建会话保留 Provider Logo，极简模型设置使用可访问的服务名称", () => {
  assert.match(dialogSource, /PROVIDER_LOGOS, providerTone/);
  assert.match(settingsSource, /className="model-provider-name"/);
  assert.match(settingsSource, /<strong>\{provider.name \|\| provider.id\}<\/strong>/);
  assert.match(settingsSource, /aria-expanded=\{expanded\}/);
  assert.match(dialogSource, /<img src=\{logo\} alt="" aria-hidden="true" \/>/);
  assert.match(logosSource, /agent-kimi-code\.svg/);
  assert.match(logosSource, /model-zhipu\.svg/);
  assert.match(logosSource, /model-deepseek\.svg/);
  assert.match(logosSource, /model-minimax\.svg/);
});
