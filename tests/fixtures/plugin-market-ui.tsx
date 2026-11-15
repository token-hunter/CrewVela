import React from "react";
import { createRoot } from "react-dom/client";
import { PluginMarketPanel } from "../../src/components/PluginMarketPanel";
import { CURATED_PLUGIN_PREVIEW, type PluginMarketSnapshot } from "../../src/pluginMarket";
import "../../src/styles.css";

const snapshot: PluginMarketSnapshot = structuredClone(CURATED_PLUGIN_PREVIEW);
snapshot.installed = [];
if (new URLSearchParams(window.location.search).has("external")) {
  snapshot.marketplaces[0].id = "fixture-market";
  snapshot.marketplaces[0].sourceKind = "local";
  snapshot.marketplaces[0].plugins = snapshot.marketplaces[0].plugins.map(plugin => ({ ...plugin, compatibility: { format: "claude", status: "native", nativeCodex: false, nativeClaude: true, skillCompatible: true, components: ["1 个 Skill"], note: "fixture only" } }));
}
const candidates = structuredClone(snapshot.marketplaces);
if (new URLSearchParams(window.location.search).has("official")) {
  snapshot.marketplaces = []; snapshot.selectedPluginIds = []; snapshot.selectionConfigured = true;
  candidates.splice(0);
}
const storageKey = "crewvela-curation-fixture";
if (new URLSearchParams(window.location.search).has("curation")) {
  const saved = localStorage.getItem(storageKey);
  snapshot.selectedPluginIds = saved ? JSON.parse(saved) : [];
  snapshot.selectionConfigured = saved !== null;
  snapshot.installed = structuredClone(CURATED_PLUGIN_PREVIEW.installed);
  snapshot.installedCatalog = candidates.flatMap(market => market.plugins.map(plugin => ({ market, plugin }))).filter(entry => snapshot.installed.some(item => item.id === `${entry.plugin.name}@${entry.market.id}`));
  snapshot.marketplaces = candidates.map(market => ({ ...market, plugins: market.plugins.filter(plugin => snapshot.selectedPluginIds!.includes(`${plugin.name}@${market.id}`)) }));
}
const calls: { cmd: string; args: any }[] = [];
let failLoad = new URLSearchParams(location.search).has("load-error");
const pending: Array<{ cmd: string; resolve: () => void; reject: (error: Error) => void }> = [];
Object.assign(window, { __pluginPending: pending, __pluginHolds: new URLSearchParams(location.search).has("delay-load") ? ["plugin_market_snapshot"] : [] });
Object.assign(window, { __pluginCalls: calls, __pluginSnapshot: snapshot, __TAURI_INTERNALS__: { invoke: async (cmd: string, args: any) => {
  calls.push({ cmd, args });
  if (cmd === "plugin_market_snapshot" && failLoad) { failLoad = false; throw new Error("测试目录加载失败"); }
  const captured = cmd === "plugin_market_snapshot" ? structuredClone(snapshot) : null;
  if ((window as any).__pluginHolds.includes(cmd)) await new Promise<void>((resolve, reject) => pending.push({ cmd, resolve, reject }));
  if (cmd === "plugin_market_add_official") {
    const market = { ...CURATED_PLUGIN_PREVIEW.marketplaces[0], id: "openai-curated", sourceKind: "git", ownerName: "OpenAI", source: "openai/plugins @ 0722921d5542fc593105c27bd52630babd8b8c2a", url: "https://github.com/openai/plugins", plugins: [
      { ...CURATED_PLUGIN_PREVIEW.marketplaces[0].plugins[0], name: "remotion", displayName: "Remotion", description: "通过代码创建视频", compatibility: { format: "codex", status: "native" as const, nativeCodex: true, nativeClaude: false, skillCompatible: false, components: ["Skill ×12"], note: "原生整包保留相对引用；其它 Agent 尚未适配", dependencies: ["部分流程需要 Codex in-app browser 与配套宿主工具"], license: "MIT" } },
      { ...CURATED_PLUGIN_PREVIEW.marketplaces[0].plugins[0], name: "product-design", displayName: "Product Design", description: "产品设计工作流", compatibility: { format: "codex", status: "native" as const, nativeCodex: true, nativeClaude: false, skillCompatible: false, components: ["Skill ×10"], note: "原生文件安装不代表宿主功能可用", dependencies: ["需要 Codex 宿主的 Browser、ImageGen、Sites 工具", "需要正确的用户上下文与账号授权"], license: "专有许可" } },
      { ...CURATED_PLUGIN_PREVIEW.marketplaces[0].plugins[0], name: "github", displayName: "GitHub", installable: false, compatibility: { format: "codex", status: "needs_auth" as const, nativeCodex: false, nativeClaude: false, skillCompatible: false, components: ["Apps / 连接器（待授权）"], note: "仅展示，不自动授权或启用 MCP" } },
    ] };
    market.plugins = market.plugins.map(plugin => ({ ...plugin, sourceDisplay: `openai/plugins / plugins/${plugin.name} @ 0722921d5542fc593105c27bd52630babd8b8c2a`, version: plugin.name === "remotion" ? "1.0.7" : plugin.name === "product-design" ? "0.1.52" : null }));
    candidates.push(market);
    snapshot.marketplaces.push({ ...market, plugins: [] });
    return market;
  }
  if (cmd === "plugin_market_snapshot") return captured;
  if (cmd === "plugin_market_candidates") return { marketplaces: (window as any).__candidateFailure ? candidates.map(market => ({ ...market, plugins: [], lastError: "测试来源读取失败" })) : structuredClone(candidates), selectedPluginIds: snapshot.selectedPluginIds ?? [], selectionConfigured: snapshot.selectionConfigured ?? false, componentHints: {} };
  if (cmd === "plugin_market_save_selection") {
    if ((window as any).__curationFailure) throw new Error("测试保存失败");
    snapshot.selectedPluginIds = args.pluginIds;
    snapshot.selectionConfigured = true;
    localStorage.setItem(storageKey, JSON.stringify(args.pluginIds));
    snapshot.marketplaces = candidates.map(market => ({ ...market, plugins: market.plugins.filter(plugin => args.pluginIds.includes(`${plugin.name}@${market.id}`)) }));
    return structuredClone(snapshot);
  }
  if (cmd === "skill_center_snapshot") return { runtimes: [{ runtime: "claude_code", installed: true }, { runtime: "codex", installed: true }] };
  if (cmd === "plugin_market_install") {
    if ((window as any).__pluginFailure) throw new Error("测试安装失败");
    const input = args.input;
    const id = `${input.pluginName}@${input.marketplaceId}`;
    let plugin = snapshot.installed.find((item) => item.id === id);
    if (!plugin) { plugin = { ...CURATED_PLUGIN_PREVIEW.installed[0], id, marketplaceId: input.marketplaceId, name: input.pluginName, hasAgents: false, skillNames: [input.pluginName], mcpServerNames: [], targets: [] }; snapshot.installed.push(plugin); }
    for (const runtime of input.runtimes) plugin.targets!.push({ id: `${runtime}|${input.scope}|${input.projectPath ?? ""}`, runtime, scope: input.scope, projectPath: input.projectPath, deploymentIds: [runtime], status: "installed", lastError: null, native: input.mode.startsWith("native_") ? { selector: id, scope: "user", installPath: "fixture-only", verified: true, runtime: input.mode === "native_codex" ? "codex" : "claude_code" } : null });
    return structuredClone(plugin);
  }
  if (cmd === "plugin_market_remove_target") { const plugin = snapshot.installed.find((item) => item.id === args.installedId)!; plugin.targets = plugin.targets!.filter((target) => target.id !== args.targetId); return; }
  if (cmd === "plugin_market_add" || cmd === "plugin_market_refresh") return {};
  throw new Error(`Unexpected command ${cmd}`);
} } });
document.documentElement.dataset.theme = "dark";
createRoot(document.getElementById("root")!).render(<div className="settings-overlay"><div className="settings-dialog" style={{ position: "fixed", inset: 0, width: "100%", height: "100%", maxWidth: "none", maxHeight: "none" }}>
  <aside className="settings-nav"><div className="settings-brand">设置</div><div style={{ margin: "12px 0 24px", padding: 10, border: "1px solid var(--border)", borderRadius: 7, color: "var(--text-dim)", fontSize: 13 }}>搜索设置</div><nav className="settings-nav-list">{["常规", "外观", "模型设置", "协作中心", "共享记忆", "自动化", "浏览器控制", "电脑控制", "插件市场", "Skills", "MCP"].map(label => <span className={"settings-nav-item" + (label === "插件市场" ? " active" : "")} key={label}>{label}</span>)}</nav><small style={{ display: "block", marginTop: 24, color: "var(--text-faint)" }}>隔离 Mock · 不会实际安装</small></aside>
  <div className="settings-content"><div className="settings-content-inner"><div className="settings-heading"><h2>插件市场</h2></div><PluginMarketPanel projects={[{ id: "demo", name: "测试项目", path: "F:/project/demo", badgeColor: "", createdAt: 0, updatedAt: 0 }]} /></div></div>
</div></div>);
