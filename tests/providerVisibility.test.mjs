import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const settings = readFileSync(
  new URL("../src/components/SettingsDialog.tsx", import.meta.url),
  "utf8",
);
const types = readFileSync(new URL("../src/types.ts", import.meta.url), "utf8");
const backend = readFileSync(
  new URL("../src-tauri/src/providers.rs", import.meta.url),
  "utf8",
);
const commands = readFileSync(
  new URL("../src-tauri/src/lib.rs", import.meta.url),
  "utf8",
);

test("ProviderConfig 持久化隐藏状态", () => {
  assert.match(types, /hidden\??:\s*boolean/);
  assert.match(backend, /pub hidden: bool/);
  assert.match(backend, /set_hidden/);
  assert.match(commands, /set_provider_hidden/);
  assert.match(commands, /restore_builtin_provider/);
});

test("内置模型显示隐藏与恢复默认操作，自定义模型显示删除", () => {
  assert.match(settings, /onSetHidden/);
  assert.match(settings, /onRestoreDefault/);
  assert.match(settings, /隐藏|重新显示/);
  assert.match(settings, /恢复默认/);
  assert.match(settings, /provider\.builtin \?/);
  assert.match(settings, /settings-provider-delete/);
  assert.match(backend, /内置模型不能删除/);
});

test("运行时选择器排除隐藏模型，设置页仍接收完整列表", () => {
  assert.match(app, /selectableProviders\s*=\s*providers\.filter/);
  assert.match(app, /<SettingsDialog[\s\S]*providers=\{providers\}/);
  assert.match(
    app,
    /<NewSessionDialog[\s\S]*providers=\{selectableProviders\}/,
  );
  // 设置页显示完整列表供重新显示模型；自动化只接收可选模型。
  assert.match(
    settings,
    /<AutomationPanel[\s\S]*providers=\{selectableProviders\}/,
  );
  assert.match(
    settings,
    /const selectableProviders = providers\.filter\(\(provider\) => !provider\.hidden\)/,
  );
});
