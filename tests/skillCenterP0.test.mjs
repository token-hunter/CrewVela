import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const panel = readFileSync(new URL("../src/components/SkillCenterPanel.tsx", import.meta.url), "utf8");
const settings = readFileSync(new URL("../src/components/SettingsDialog.tsx", import.meta.url), "utf8");
const collaboration = readFileSync(new URL("../src/components/CollaborationSettingsPanel.tsx", import.meta.url), "utf8");
const backend = readFileSync(new URL("../src-tauri/src/skill_center.rs", import.meta.url), "utf8");
const market = readFileSync(new URL("../src/components/SkillMarketPanel.tsx", import.meta.url), "utf8");
const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");

test("设置的集成分组提供插件市场、Skill 中心和 MCP 独立入口", () => {
  const group = settings.split('label: "集成"')[1].split('label: "数据"')[0];
  for (const [id, label, panelName] of [["pluginMarket", "插件市场", "PluginMarketPanel"], ["skills", "Skill 中心", "SkillCenterPanel"], ["mcp", "MCP", "McpSettingsPanel"]]) {
    assert.ok(group.includes(`id: "${id}"`) && group.includes(`label: "${label}"`));
    assert.match(settings, new RegExp(`activeSection === "${id}" \\? \\(\\s*<${panelName}\\b`));
  }
  assert.doesNotMatch(settings, /扩展中心|extensionTab/);
  assert.match(backend, /struct SkillAsset/);
  assert.match(backend, /struct SkillDeployment/);
  assert.match(backend, /struct SkillSet/);
});

test("多 Runtime 分发矩阵覆盖当前支持的 Runtime", () => {
  for (const runtime of [
    "claude_code", "codex", "kimi_code",
    "pi", "opencode",
  ]) {
    assert.match(backend, new RegExp(`\"${runtime}\"`));
  }
  assert.match(panel, /skill-center-matrix/);
  assert.match(panel, /skill_center_deploy_batch/);
  assert.match(panel, /skill_center_undeploy_batch/);
});

test("P0 支持全局、项目、任务作用域和多个配置集", () => {
  assert.match(panel, /option value="global">全局/);
  assert.match(panel, /option value="project">项目/);
  assert.match(panel, /option value="task">任务 \/ Agent/);
  assert.match(panel, /skill_center_save_set/);
  assert.match(panel, /skill_center_apply_set/);
  assert.match(collaboration, /Skill 配置建议/);
  assert.match(collaboration, /skill_center_apply_set/);
  assert.doesNotMatch(backend, /active_skill_set|唯一活动/);
});

test("部署具备无覆盖预检、Windows Junction 降级和 Copy 所有权保护", () => {
  assert.match(backend, /for item in &prepared \{\s*preflight_target/);
  assert.match(backend, /symlink_dir\(source, target\)/);
  assert.match(backend, /junction::create\(source, target\)/);
  assert.match(backend, /COPY_MARKER/);
  assert.match(backend, /is_owned_copy/);
  assert.match(backend, /Runtime 副本已被修改，拒绝删除/);
});

test("来源、Hash、信任审阅与文件预览均有真实命令链路", () => {
  for (const command of [
    "skill_center_import", "skill_center_discover", "skill_center_set_trust",
    "skill_center_preview", "skill_center_read_file",
  ]) {
    assert.match(panel, new RegExp(`\"${command}\"`));
  }
  assert.match(backend, /Sha256/);
  assert.match(backend, /trust_state/);


  assert.match(backend, /risk_summary/);
});

test("Skill 市场同时支持仓库浏览和 skills.sh 搜索安装", () => {
  assert.match(panel, /Skill 市场/);
  assert.match(market, /source === "repos"/);
  assert.match(market, /source === "skills_sh"/);
  assert.match(market, /skills.sh/);
  assert.match(market, /skill_center_market_install/);
  assert.match(market, /仓库管理/);
  for (const command of [
    "skill_center_market_repositories",
    "skill_center_market_repository_save",
    "skill_center_market_repository_delete",
    "skill_center_market_browse",
    "skill_center_skills_sh_search",
    "skill_center_market_install",
  ]) {
    assert.match(lib, new RegExp(`\\b${command}\\b`));
  }
  assert.match(backend, /https:\/\/skills\.sh\/api\/search/);
  assert.match(backend, /find_skill_roots_recursive/);
  assert.match(backend, /skills\.sh 安装缺少唯一 key/);
});


test("Skill 中心后台 Git 命令在 Windows 不创建可见终端", () => {
  const runtimeBackend = backend.split("#[cfg(test)]")[0];
  assert.match(runtimeBackend, /fn background_command\(program: &str\) -> Command/);
  assert.match(runtimeBackend, /CREATE_NO_WINDOW/);
  assert.match(runtimeBackend, /creation_flags\(CREATE_NO_WINDOW\)/);
  assert.doesNotMatch(runtimeBackend, /Command::new\("git"\)/);
});
