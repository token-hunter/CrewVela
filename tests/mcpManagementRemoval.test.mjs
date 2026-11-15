import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import test from "node:test";
import { sanitizeSidebarShortcuts } from "../src/sidebarShortcuts.ts";

const read = path => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");

test("removed MCP management cannot be reached through settings, shortcuts or renderer IPC", () => {
  const settings = read("src/components/SettingsDialog.tsx");
  assert.doesNotMatch(settings, /McpSettingsPanel|id: "mcp"|title: "MCP 服务器"/);
  assert.deepEqual(sanitizeSidebarShortcuts(["mcp", "git", "browserControl", "sharedMemory", "skills"]), ["git", "sharedMemory", "browserControl", "skills"]);
  const native = read("src-tauri/src/lib.rs");
  for (const command of ["mcp_list_servers", "mcp_save_server", "mcp_delete_server", "mcp_set_server_enabled", "mcp_probe_servers"]) assert.ok(!native.includes(command), command);
  for (const file of ["src/components/McpSettingsPanel.tsx", "src/mcpServers.ts"]) assert.equal(existsSync(new URL(`../${file}`, import.meta.url)), false);
  assert.doesNotMatch(read("src/styles/integrations.css"), /\.mcp-(?:settings|toolbar|server|form|card|tools|tool-list)/);
});

test("removal retains internal MCP control planes and native CLI injection", () => {
  const native = read("src-tauri/src/lib.rs");
  for (const module of ["mcp", "session_mcp", "browser_control", "ai_memory"]) assert.match(native, new RegExp(`mod ${module};`));
  assert.match(read("src-tauri/src/mcp.rs"), /pub fn enabled_claude_map\(/);
  assert.match(read("src-tauri/src/mcp.rs"), /pub fn enabled_native_chat_servers\(/);
  assert.match(read("src-tauri/src/session_mcp.rs"), /crewvela_sessions/);
  assert.match(read("src-tauri/src/browser_control.rs"), /mcp::upsert_builtin_server_at/);
  assert.match(read("src-tauri/src/ai_memory_verify.rs"), /mcp::probe_remote/);
});
