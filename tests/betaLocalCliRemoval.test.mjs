import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import test from "node:test";
const read = path => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");

test("Beta 删除三项设置入口、管理 IPC 和专用实现，保留协作、Skills、MCP、浏览器", () => {
  const settings = read("src/components/SettingsDialog.tsx");
  assert.doesNotMatch(settings, /ModelAssignmentsPanel|ComputerControlSettingsPanel|PluginMarketPanel|id: "(?:providers|computerControl|pluginMarket)"/);
  for (const retained of ["CollaborationSettingsPanel", "SkillCenterPanel", "BrowserControlSettingsPanel", "SharedMemorySettingsPanel", "AutomationPanel"]) assert.ok(settings.includes(retained));
  const backend = read("src-tauri/src/lib.rs");
  assert.doesNotMatch(backend, /fn (?:model_assignments_save|model_assignments_snapshot|plugin_market_install|computer_control_save|save_provider|fetch_provider_models)\b/);
  for (const file of ["src-tauri/src/model_assignments.rs", "src-tauri/src/computer_control.rs", "src-tauri/src/plugin_market.rs", "src/components/ProviderEditor.tsx"]) assert.equal(existsSync(new URL(`../${file}`, import.meta.url)), false, file);
});

test("CLI 创建和唤醒不再注入应用模型覆盖或读取应用服务密钥", () => {
  const backend = read("src-tauri/src/lib.rs");
  assert.doesNotMatch(backend, /model_assignments::|set_launch_model\(|record_model_assignment\(|secrets::get\(/);
  const executor = read("src-tauri/src/multi_agent/executor.rs").split("#[cfg(test)]\nmod tests")[0];
  assert.doesNotMatch(executor, /configure_claude_provider|model_assignments::|\.arg\("--model"\)/);
  assert.doesNotMatch(read("src-tauri/src/main.rs"), /Some\("--computer-control-mcp"\)/);
  assert.doesNotMatch(read("src/hooks/useSessionData.ts"), /invoke.*(?:list_providers|get_key_state)/);
  assert.doesNotMatch(read("src/components/ControlWindow.tsx"), /invoke.*list_providers/);
});

test("自动化和成员编辑不提供应用模型选择，原生 CLI 入口直接保留", () => {
  assert.doesNotMatch(read("src/components/CollaborationAgentEditor.tsx"), /<label>模型<input/);
  assert.doesNotMatch(read("src/components/AutomationPanel.tsx"), /<summary>模型与 Provider|aria-label="模型标识"|指定 CrewVela Provider/);
  assert.doesNotMatch(read("src/components/NewSessionDialog.tsx"), /选择 Provider|providers\.filter/);
  assert.match(read("src/components/NewSessionDialog.tsx"), /onLocalClaudeSelect\(\)/);
});
