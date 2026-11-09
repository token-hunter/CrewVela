import assert from "node:assert/strict";
import test from "node:test";
import {
  componentBadges,
  filterMarketPlugins,
  installedKey,
  pluginTitle,
  validateMarketSource,
  PLUGIN_MARKET_PREVIEW,
  compatibilityLabel,
  pluginModes,
  pluginSupportsAgent,
} from "../src/pluginMarket.ts";

test("Codex 原生包与连接器明确区分安装方式和 Agent 边界", () => {
  const plugin = { ...PLUGIN_MARKET_PREVIEW.marketplaces[0].plugins[0], compatibility: { format: "codex", status: "native", nativeCodex: true, nativeClaude: false, skillCompatible: false, components: ["Skill ×12"], note: "保留根引用" } };
  assert.deepEqual(pluginModes(plugin, false), ["native_codex"]);
  assert.equal(pluginSupportsAgent(plugin, false, "codex"), true);
  assert.equal(pluginSupportsAgent(plugin, false, "claude_code"), false);
  assert.equal(pluginSupportsAgent(plugin, false, "pi"), false);
  const connector = { ...plugin, compatibility: { ...plugin.compatibility, status: "needs_auth", nativeCodex: false } };
  assert.deepEqual(pluginModes(connector, false), []);
  assert.deepEqual(pluginModes({ ...plugin, compatibility: null }, false), [], "未知包不预设兼容安装方式");
  assert.equal(compatibilityLabel(connector), "待授权 · 仅展示");
});

test("validateMarketSource 接受本地路径、git URL 与 owner/repo 简写", () => {
  assert.equal(validateMarketSource("F:\\plugins\\my-market"), null);
  assert.equal(validateMarketSource("/home/user/plugins/my-market"), null);
  assert.equal(validateMarketSource("/Users/person/Plugin Library"), null);
  assert.equal(validateMarketSource("\\\\server\\share\\market"), null);
  assert.equal(validateMarketSource("https://github.com/x/y.git"), null);
  assert.equal(validateMarketSource("git@gitlab.com:x/y.git"), null);
  assert.equal(validateMarketSource("anthropics/claude-plugins-community"), null);

  assert.match(validateMarketSource(""), /不能为空/);
  assert.match(validateMarketSource("not a source"), /无法识别/);
  assert.match(validateMarketSource("a/b/c"), /无法识别/);
  assert.match(validateMarketSource("带空格/x"), /无法识别/);
});

test("componentBadges 汇总组件并标注未分发", () => {
  const badges = componentBadges({
    skillNames: ["a", "b"],
    mcpServerNames: ["x"],
    hasHooks: true,
    hasAgents: true,
  });
  assert.deepEqual(
    badges.map((badge) => badge.label),
    ["Skills ×2", "MCP ×1", "Hooks/Agents（未分发）"],
  );
  assert.equal(badges[2].tone, "claude-only");

  assert.deepEqual(componentBadges({ skillNames: [], mcpServerNames: [], hasHooks: false, hasAgents: false }), []);
  assert.deepEqual(
    componentBadges({ skillNames: [], mcpServerNames: [], hasHooks: false, hasAgents: true }).map((b) => b.label),
    ["Agents（未分发）"],
  );
});

test("pluginTitle 与 installedKey 的组合约定", () => {
  assert.equal(pluginTitle({ displayName: "  ", name: "pdf" }), "pdf");
  assert.equal(
    pluginTitle(PLUGIN_MARKET_PREVIEW.marketplaces[0].plugins[0]),
    "PDF 工具箱",
  );
  assert.equal(installedKey("claude-community", "pdf"), "pdf@claude-community");
  assert.ok(PLUGIN_MARKET_PREVIEW.installed[0].id === installedKey("claude-community", "pdf"));
});

test("filterMarketPlugins 全部市场 + 保持原有顺序", () => {
  const entries = filterMarketPlugins(PLUGIN_MARKET_PREVIEW.marketplaces, null, "");
  assert.equal(
    entries.length,
    PLUGIN_MARKET_PREVIEW.marketplaces.reduce((sum, m) => sum + m.plugins.length, 0),
  );
  assert.deepEqual(
    entries.map(({ market, plugin }) => `${plugin.name}@${market.id}`),
    [
      "pdf@claude-community",
      "frontend-design@claude-community",
      "cloud-ops@claude-community",
      "browser-use@zcode-official",
      "postgres-helper@zcode-official",
      "deploy-notify@zcode-official",
    ],
  );
});

test("filterMarketPlugins 选中单个市场", () => {
  const entries = filterMarketPlugins(PLUGIN_MARKET_PREVIEW.marketplaces, "zcode-official", "");
  assert.ok(entries.every(({ market }) => market.id === "zcode-official"));
  assert.equal(entries.length, 3);

  const unknown = filterMarketPlugins(PLUGIN_MARKET_PREVIEW.marketplaces, "nope", "");
  assert.deepEqual(unknown, []);
});

test("filterMarketPlugins 关键词匹配标题/描述/分类且不区分大小写", () => {
  const byTitle = filterMarketPlugins(PLUGIN_MARKET_PREVIEW.marketplaces, null, "PDF 工具箱");
  assert.deepEqual(byTitle.map(({ plugin }) => plugin.name), ["pdf"]);

  const byDescription = filterMarketPlugins(PLUGIN_MARKET_PREVIEW.marketplaces, null, "浏览器");
  assert.deepEqual(byDescription.map(({ plugin }) => plugin.name), ["browser-use"]);

  const byCategory = filterMarketPlugins(PLUGIN_MARKET_PREVIEW.marketplaces, null, "database");
  assert.deepEqual(byCategory.map(({ plugin }) => plugin.name), ["postgres-helper"]);

  const upper = filterMarketPlugins(PLUGIN_MARKET_PREVIEW.marketplaces, null, "  POSTGRES ");
  assert.deepEqual(upper.map(({ plugin }) => plugin.name), ["postgres-helper"]);

  assert.deepEqual(filterMarketPlugins(PLUGIN_MARKET_PREVIEW.marketplaces, null, "不存在的关键词"), []);
});

test("filterMarketPlugins 市场筛选与关键词叠加", () => {
  const entries = filterMarketPlugins(
    PLUGIN_MARKET_PREVIEW.marketplaces,
    "claude-community",
    "前端",
  );
  assert.deepEqual(entries.map(({ plugin }) => plugin.name), ["frontend-design"]);
});
