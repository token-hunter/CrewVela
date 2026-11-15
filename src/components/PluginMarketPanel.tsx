import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowLeftIcon, ArrowRightIcon, CaretRightIcon, CodeIcon, FileTextIcon, MagnifyingGlassIcon, PackageIcon, SlidersHorizontalIcon, TableIcon, TestTubeIcon, XIcon } from "@phosphor-icons/react";
import { Modal } from "./ui/Modal";
import type { Project } from "../types";
import { agentLabel, compatibilityLabel, pluginModes, pluginSupportsAgent, filterMarketPlugins, installedKey, PLUGIN_AGENTS, PLUGIN_CATEGORIES, BUILTIN_CANDIDATE_COUNT, CURATED_PLUGIN_PREVIEW, pluginCategory, pluginTitle, pluginUpstreamUrl, validateMarketSource, type InstalledPlugin, type MarketPluginEntry, type PluginMarketSnapshot, type PluginTarget } from "../pluginMarket";
import "./plugin-market.css";
import "./plugin-market-browser.css";
import { PluginCurationDialog } from "./PluginCurationDialog";

const UI_PREVIEW = new URLSearchParams(window.location.search).get("ui-preview") === "1";
type Adapter = { runtime: string; installed: boolean };
function Icon({ name }: { name: string }) {
  const Component = name.includes("design") ? CodeIcon : name.includes("testing") ? TestTubeIcon : name.includes("xlsx") ? TableIcon : FileTextIcon;
  return <span className={"pm-icon pm-icon-" + (name.includes("design") ? "purple" : name.includes("testing") ? "green" : "blue")}><Component size={29} /></span>;
}
const statusLabel = (target: PluginTarget) => target.status === "installed" ? target.native ? "原生插件已安装" : "Skill 已部署" : target.status === "failed" ? "安装失败" : "需要检查";

export function PluginMarketPanel({ projects = [] }: { projects?: Project[] }) {
  const [snapshot, setSnapshot] = useState<PluginMarketSnapshot | null>(UI_PREVIEW ? CURATED_PLUGIN_PREVIEW : null);
  const [adapters, setAdapters] = useState<Adapter[]>([]);
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState("全部");
  const [agent, setAgent] = useState("");
  const [installableOnly, setInstallableOnly] = useState(false);
  const [catalogTab, setCatalogTab] = useState<"featured" | "installed" | "pending">("featured");
  const [advanced, setAdvanced] = useState(false);
  const [curationOpen, setCurationOpen] = useState(false);
  const [curationMarket, setCurationMarket] = useState<string | null>(null);
  const [source, setSource] = useState("");
  const [selectedMarket, setSelectedMarket] = useState<string | null>(null);
  const [detail, setDetail] = useState<MarketPluginEntry | null>(null);
  const [runtimes, setRuntimes] = useState<string[]>([]);
  const [scope, setScope] = useState("global");
  const [mode, setMode] = useState("skills");
  const [projectPath, setProjectPath] = useState("");
  const [trust, setTrust] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [view, setView] = useState<"catalog" | "installed">("catalog");
  const [busy, setBusy] = useState(false);
  const operationInFlight = useRef(false);
  const snapshotSequence = useRef(0);
  const inspectorScroll = useRef<HTMLDivElement>(null);
  const browserLayout = useRef<HTMLDivElement>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [removeSelection, setRemoveSelection] = useState<{ plugin: InstalledPlugin; target: PluginTarget } | null>(null);
  const refresh = async () => {
    const sequence = ++snapshotSequence.current;
    const fresh = await invoke<PluginMarketSnapshot>("plugin_market_snapshot");
    if (sequence === snapshotSequence.current) { setSnapshot(fresh); setLoadError(null); }
  };
  useEffect(() => {
    if (UI_PREVIEW) return;
    let cancelled = false;
    const sequence = ++snapshotSequence.current;
    void invoke<PluginMarketSnapshot>("plugin_market_snapshot").then((fresh) => { if (!cancelled && sequence === snapshotSequence.current) setSnapshot(fresh); }).catch((error) => { if (!cancelled && sequence === snapshotSequence.current) setLoadError(String(error)); });
    void invoke<{ runtimes: Adapter[] }>("skill_center_snapshot").then((data) => { if (!cancelled) setAdapters(data.runtimes); }).catch(() => { /* unavailable detection is shown in the UI */ });
    return () => { cancelled = true; snapshotSequence.current += 1; };
  }, []);
  const operation = async (action: () => Promise<void>) => {
    if (UI_PREVIEW || operationInFlight.current) return;
    operationInFlight.current = true;
    setBusy(true); setNotice(null);
    try { await action(); } catch (error) { setNotice(String(error)); } finally { operationInFlight.current = false; setBusy(false); }
  };
  const installed = snapshot?.installed ?? [];
  const markets = snapshot?.marketplaces ?? [];
  const catalogMarkets = useMemo(() => {
    if (catalogTab !== "installed") return markets;
    const byId = new Map(markets.map(market => [market.id, { ...market, plugins: [...market.plugins] }]));
    for (const entry of snapshot?.installedCatalog ?? []) {
      const market = byId.get(entry.market.id) ?? { ...entry.market, plugins: [] };
      if (!market.plugins.some(plugin => plugin.name === entry.plugin.name)) market.plugins.push(entry.plugin);
      byId.set(market.id, market);
    }
    return [...byId.values()];
  }, [markets, snapshot?.installedCatalog, catalogTab]);
  const entries = useMemo(() => filterMarketPlugins(catalogMarkets, selectedMarket, query).filter(({ plugin, market }) =>
    (category === "全部" || pluginCategory(plugin.category) === category) && (!installableOnly || plugin.installable) &&
    (catalogTab !== "installed" || installed.some(item => item.id === installedKey(market.id, plugin.name))) &&
    (catalogTab !== "pending" || !plugin.installable || !pluginModes(plugin, market.sourceKind === "builtin").length) &&
    (!agent || pluginSupportsAgent(plugin, market.sourceKind === "builtin", agent)))
    .sort((a, b) => PLUGIN_CATEGORIES.indexOf(pluginCategory(a.plugin.category)) - PLUGIN_CATEGORIES.indexOf(pluginCategory(b.plugin.category))), [catalogMarkets, selectedMarket, query, category, installableOnly, agent, catalogTab, installed]);
  const findEntry = (plugin: InstalledPlugin) => [...(snapshot?.installedCatalog ?? []), ...markets.flatMap((market) => market.plugins.map((entry) => ({ market, plugin: entry })))].find((entry) => installedKey(entry.market.id, entry.plugin.name) === plugin.id);
  const openDetail = (entry: MarketPluginEntry) => { setDetail(entry); const nextMode = pluginModes(entry.plugin, entry.market.sourceKind === "builtin")[0] ?? "unsupported"; setMode(nextMode); setRuntimes(nextMode === "native_codex" ? ["codex"] : nextMode === "native_claude" ? ["claude_code"] : agent && PLUGIN_AGENTS.find((item) => item.id === agent)?.support ? [agent] : []); if (nextMode.startsWith("native_")) setScope("global"); setTrust(false); setNotice(null); };
  useEffect(() => {
    if (view !== "catalog" || busy) return;
    const current = detail && entries.find(entry => installedKey(entry.market.id, entry.plugin.name) === installedKey(detail.market.id, detail.plugin.name));
    const next = current ?? entries[0] ?? null;
    if (!next) { if (detail) setDetail(null); return; }
    const available = pluginModes(next.plugin, next.market.sourceKind === "builtin");
    if (!current || (available.length ? !available.includes(mode) : mode !== "unsupported")) openDetail(next);
    else if (next !== detail) setDetail(next);
  }, [entries, view, busy, detail, mode]);
  useEffect(() => { inspectorScroll.current?.scrollTo({ top: 0 }); }, [detail?.market.id, detail?.plugin.name]);
  useEffect(() => {
    const layout = browserLayout.current;
    if (!layout) return;
    const host = layout.closest(".settings-content") ?? document.documentElement;
    const measure = () => {
      const bottom = Math.min(window.innerHeight, host.getBoundingClientRect().bottom);
      layout.style.setProperty("--pm-pane-height", `${Math.max(280, Math.floor(bottom - layout.getBoundingClientRect().top - 20))}px`);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(host); observer.observe(layout.parentElement!);
    window.addEventListener("resize", measure);
    return () => { observer.disconnect(); window.removeEventListener("resize", measure); };
  }, [view, loadError]);
  const showInstalledDetail = (entry: MarketPluginEntry) => {
    setView("catalog"); setCatalogTab("installed"); setQuery(""); setCategory("全部"); setAgent(""); setSelectedMarket(null); setInstallableOnly(false); openDetail(entry);
  };
  const titleOf = (plugin: InstalledPlugin) => { const entry = findEntry(plugin); return entry ? pluginTitle(entry.plugin) : plugin.name; };
  const detailInstalled = detail ? installed.find((plugin) => plugin.id === installedKey(detail.market.id, detail.plugin.name)) : null;
  const install = () => operation(async () => {
    if (!detail || !detail.plugin.installable || !pluginModes(detail.plugin, detail.market.sourceKind === "builtin").includes(mode)) return;
    const result = await invoke<InstalledPlugin>("plugin_market_install", { input: { marketplaceId: detail.market.id, pluginName: detail.plugin.name, runtimes, scope, projectPath: scope === "project" ? projectPath : null, trustConfirmed: trust, mode } });
    await refresh();
    const failures = (result.targets ?? []).filter((target) => runtimes.includes(target.runtime) && target.status !== "installed");
    setNotice(failures.length ? failures.map((target) => agentLabel(target.runtime) + "：" + target.lastError).join("；") : mode.startsWith("native_") ? agentLabel(mode === "native_codex" ? "codex" : "claude_code") + " 原生插件安装记录已核验。请重开会话；宿主工具、账号、密钥和依赖仍需按上游要求配置。" : "Skill 已分发到 " + runtimes.map(agentLabel).join("、") + "。重开会话后使用；原生插件、MCP、Hooks 与子 Agent 未安装。");
  });
  const groups = PLUGIN_CATEGORIES.filter(item => item !== "全部").map(label => ({ label, entries: entries.filter(entry => pluginCategory(entry.plugin.category) === label) })).filter(group => group.entries.length);
  const modes = detail ? pluginModes(detail.plugin, detail.market.sourceKind === "builtin") : [];
  const canInstall = !!detail?.plugin.installable && modes.length > 0;
  const sourceLabel = detail ? detail.plugin.sourceDisplay.split(/\s+\/\s+/)[0] || detail.market.ownerName : "";
  const upstreamUrl = detail ? pluginUpstreamUrl(detail) : null;
  const agentOption = (item: typeof PLUGIN_AGENTS[number]) => <label key={item.id} className={!item.support ? "unsupported" : ""}>
    <input type="checkbox" disabled={!canInstall || !item.support || busy || (mode.startsWith("native_") && item.id !== (mode === "native_codex" ? "codex" : "claude_code")) || mode === "unsupported"} checked={runtimes.includes(item.id)} onChange={event => setRuntimes(event.target.checked ? [...runtimes, item.id] : runtimes.filter(id => id !== item.id))} />
    <span><strong>{item.label}</strong><small>{mode.startsWith("native_") ? item.id === (mode === "native_codex" ? "codex" : "claude_code") ? "通过 CLI 安装原生整包" : "此安装模式不适用于该 Agent" : item.support ? adapters.find(adapter => adapter.runtime === item.id)?.installed ? "CLI 已检测 · 可分发 Skill" : "CLI 未检测 · 可预置 Skill" : item.hint}</small></span>
  </label>;
  const details = detail ? <section className="pm-inspector" role="region" aria-label="插件详情与安装" aria-busy={busy}>
    <header className="pm-inspector-heading"><Icon name={detail.plugin.name} /><div><h2>{pluginTitle(detail.plugin)}</h2><p>{detail.plugin.name} <span className="pm-badge">{detail.market.sourceKind === "builtin" ? "Skill" : compatibilityLabel(detail.plugin)}</span></p></div></header>
    <div className="pm-inspector-scroll" ref={inspectorScroll} tabIndex={0} aria-label="插件信息">
    <p className="pm-inspector-description">{detail.plugin.description || "请查看上游说明了解用途。"}</p>
    <dl className="pm-metadata pm-source">
      <div><dt>来源</dt><dd title={detail.plugin.sourceDisplay}>{sourceLabel}</dd></div>
      <div><dt>{detail.market.sourceKind === "builtin" ? "固定版本" : "版本"}</dt><dd>{detail.plugin.version ?? "上游未提供"}</dd></div>
      <div><dt>包含组件</dt><dd>{detail.plugin.compatibility?.components.length ? detail.plugin.compatibility.components.join(" · ") : detailInstalled ? detailInstalled.skillNames.length + " 个 Skill" : detail.market.sourceKind === "builtin" ? "1 个 Skill" : "尚未确认"}</dd></div>
      {upstreamUrl && <div><dt>来源说明</dt><dd><a href={upstreamUrl} target="_blank" rel="noreferrer">查看上游说明 <ArrowRightIcon size={14} /></a></dd></div>}
    </dl>
    {canInstall ? <section className="pm-install-column pm-install-box">
      <h3>安装到 Agent</h3><p className="pm-muted">{mode.startsWith("native_") ? "按原生格式安装，各 Agent 独立管理。" : "选择安装目标，Skill 将分别部署。"}</p>
      {detail.market.sourceKind !== "builtin" && <div className="pm-install-mode">{modes.map(candidate => <label key={candidate}><input type="radio" name="plugin-install-mode" checked={mode === candidate} disabled={busy} onChange={() => { setMode(candidate); if (candidate.startsWith("native_")) { setScope("global"); setRuntimes([candidate === "native_codex" ? "codex" : "claude_code"]); } else setRuntimes([]); setTrust(false); }} />{candidate === "native_codex" ? "Codex 原生插件 · 全局" : candidate === "native_claude" ? "Claude 原生插件 · 全局" : "Skill 兼容分发"}</label>)}</div>}
      <div className="pm-agent-options">{PLUGIN_AGENTS.slice(0, 2).map(agentOption)}<details className="pm-more-targets" open={runtimes.some(runtime => !["claude_code", "codex"].includes(runtime))}><summary>更多 Agent</summary>{PLUGIN_AGENTS.slice(2).map(agentOption)}</details></div>
      <div className="pm-scope-row"><label>作用范围<select aria-label="安装作用域" value={scope} disabled={busy || mode.startsWith("native_")} onChange={event => setScope(event.target.value)}><option value="global">全局 · 所有项目</option><option value="project">当前项目</option></select></label>{scope === "project" && <label>项目<select aria-label="安装项目" value={projectPath} disabled={busy} onChange={event => setProjectPath(event.target.value)}><option value="">选择项目</option>{projects.map(project => <option key={project.id} value={project.path}>{project.name}</option>)}{projectPath && !projects.some(project => project.path === projectPath) && <option value={projectPath}>{projectPath}</option>}</select></label>}</div>
    </section> : <div className="pm-warning" role="status">{detail.plugin.compatibility?.status === "needs_auth" ? "此连接器需要账号授权，当前仅展示，不自动安装或启用 MCP。" : "此插件尚未适配安装，可先查看来源与使用要求。"}</div>}
    <section className="pm-dependencies"><h3>依赖与安装范围</h3><dl className="pm-metadata"><div><dt>下载需要</dt><dd>网络、Git{mode.startsWith("native_") ? "、对应 Agent CLI" : ""}</dd></div><div><dt>运行依赖</dt><dd>按上游说明配置</dd></div></dl>
      <details><summary>查看依赖与包说明</summary>{detail.plugin.compatibility?.dependencies?.map(item => <p key={item}>{item}</p>)}<p>{detail.plugin.compatibility?.note ?? detail.plugin.note ?? "请先审阅上游使用说明。"}</p>{detail.plugin.compatibility?.license && <p>许可：{detail.plugin.compatibility.license}</p>}<p className="pm-source-full">{detail.plugin.sourceDisplay}</p></details>
    </section>
    </div><div className="pm-install-actions">
    {canInstall && <><label className="pm-trust"><input type="checkbox" checked={trust} disabled={busy} onChange={event => setTrust(event.target.checked)} />{mode.startsWith("native_") ? "我已审阅并信任此来源，允许安装插件与附带代码。" : "我已审阅并信任此来源，允许分发 Skill 与附带代码。"}</label><button type="button" className="pm-primary pm-install-submit" disabled={busy || UI_PREVIEW || !trust || !runtimes.length || (scope === "project" && !projectPath)} onClick={install}>{busy ? "正在安装…" : "安装到 " + runtimes.length + " 个 Agent"}</button><p className="pm-install-boundary">{mode.startsWith("native_") ? "原生整包安装；宿主工具和账号仍需单独核对。" : "仅分发 Skill，不安装 MCP、Hooks 或原生插件组件。"}</p></>}
    {detailInstalled && <p className="pm-installed-hint">已有 {detailInstalled.targets?.length ?? 0} 个安装目标 <button type="button" className="pm-link" disabled={busy} onClick={() => setView("installed")}>管理安装</button></p>}
    {notice && <p className="pm-notice" role="status">{notice}</p>}
    </div>
  </section> : <aside className="pm-inspector pm-inspector-empty"><PackageIcon size={38} /><h3>选择一个插件</h3><p>在左侧目录查看工具，了解来源和安装要求。</p></aside>;
  return <div className="crewvela-plugin-center pm-browser">
    <div className="pm-top-actions"><p>从精选目录中选择适合你的工具。</p><button type="button" className="pm-link" disabled={busy} onClick={() => setView(view === "catalog" ? "installed" : "catalog")}>{view === "catalog" ? "我的插件" : "返回精选目录"}</button><button type="button" className="pm-subtle" disabled={busy} onClick={() => { setCurationMarket(null); setCurationOpen(true); }}>管理精选</button><button type="button" className="pm-subtle" disabled={busy} onClick={() => setAdvanced(true)}><SlidersHorizontalIcon size={16} />高级</button></div>
    {UI_PREVIEW && <p className="pm-warning">设计预览 · 示例数据 · 安装操作已禁用</p>}
    {loadError && <div className="pm-warning" role="alert">读取插件失败：{loadError}<button type="button" disabled={busy} onClick={() => operation(refresh)}>重试</button></div>}
    {notice && (view !== "catalog" || !detail) && <p className="pm-notice" role="status">{notice}</p>}
    {view === "catalog" ? <div className="pm-browser-layout" ref={browserLayout}>
      <section className="pm-directory" aria-label="插件目录">
        <label className="pm-search"><MagnifyingGlassIcon size={20} /><input aria-label="搜索插件或使用场景" placeholder="搜索插件或任务" value={query} disabled={busy} onChange={event => setQuery(event.target.value)} /></label>
        <div className="pm-directory-toolbar"><nav className="pm-categories" aria-label="目录视图">{([["featured", "精选"], ["installed", "已安装"], ["pending", "待接入"]] as const).map(([id, title]) => <button type="button" key={id} disabled={busy} aria-pressed={catalogTab === id} className={catalogTab === id ? "active" : ""} onClick={() => setCatalogTab(id)}>{title}</button>)}</nav>
          <select aria-label="插件分类" value={category} disabled={busy} onChange={event => setCategory(event.target.value)}>{PLUGIN_CATEGORIES.map(item => <option key={item} value={item}>{item === "全部" ? "全部分类" : item}</option>)}</select>
          <select aria-label="适用 Agent" value={agent} disabled={busy} onChange={event => setAgent(event.target.value)}><option value="">适用 Agent</option>{PLUGIN_AGENTS.map(item => <option key={item.id} value={item.id}>{item.label}</option>)}</select>
        </div>
        <div className="pm-directory-count"><label className="pm-check"><input type="checkbox" checked={installableOnly} disabled={busy} onChange={event => setInstallableOnly(event.target.checked)} />仅看可安装</label><span>{entries.length} 项</span></div>
        {snapshot && catalogTab === "featured" && markets.filter(market => market.sourceKind === "builtin").reduce((sum, market) => sum + market.plugins.length, 0) < BUILTIN_CANDIDATE_COUNT && <p className="pm-catalog-nudge">内置 {BUILTIN_CANDIDATE_COUNT} 项候选，可继续加入精选。<button type="button" className="pm-link" disabled={busy} onClick={() => { setCurationMarket(null); setCurationOpen(true); }}>挑选更多</button></p>}
        {selectedMarket && <p className="pm-muted">当前来源：{selectedMarket}<button type="button" className="pm-link" disabled={busy} onClick={() => setSelectedMarket(null)}>显示全部</button></p>}
        <div className="pm-directory-results">{groups.map(group => <section className="pm-directory-group" key={group.label}><h3>{group.label}<small>共 {group.entries.length} 项</small></h3>{group.entries.map(entry => {
          const id = installedKey(entry.market.id, entry.plugin.name);
          const selected = !!detail && id === installedKey(detail.market.id, detail.plugin.name);
          return <button type="button" className={"pm-directory-row" + (selected ? " selected" : "")} key={id} aria-label={"查看 " + pluginTitle(entry.plugin) + " 详情"} aria-pressed={selected} disabled={busy} onClick={() => { if (!selected) openDetail(entry); }}><Icon name={entry.plugin.name} /><span className="pm-directory-copy"><strong>{pluginTitle(entry.plugin)}</strong><small>{entry.plugin.description || "查看来源与使用要求"}</small></span><span className="pm-directory-meta"><span className="pm-badge">{entry.market.sourceKind === "builtin" ? "Skill" : compatibilityLabel(entry.plugin)}</span><small>{entry.plugin.sourceDisplay.split(/\s+\/\s+/)[0] || entry.market.ownerName}</small></span><CaretRightIcon size={16} /></button>;
        })}</section>)}</div>
        {!snapshot && !loadError && <p className="pm-empty" role="status">正在读取精选目录…</p>}
        {snapshot && !entries.length && <div className="pm-empty"><h3>{catalogTab === "installed" ? "暂无符合条件的安装记录" : catalogTab === "pending" ? "暂无待接入插件" : !snapshot.selectionConfigured ? "尚未选择精选" : !(snapshot.selectedPluginIds ?? []).length ? "精选目录为空" : "没有符合条件的精选插件"}</h3><p>{query || category !== "全部" || agent || installableOnly ? "试试其他筛选条件。" : "通过管理精选挑选工具，安装记录保持不变。"}</p><button type="button" className="pm-outline" disabled={busy} onClick={() => { setCurationMarket(null); setCurationOpen(true); }}>管理精选目录</button></div>}
      </section>{details}
    </div> : <>

      <header className="pm-intro"><h2>已安装插件</h2><p>查看每个 Agent 的安装与配置状态。</p></header>
      <nav className="pm-categories" aria-label="管理 Agent"><button type="button" className={!agent ? "active" : ""} onClick={() => setAgent("")}>全部 Agent</button>{PLUGIN_AGENTS.map((item) => <button type="button" key={item.id} className={agent === item.id ? "active" : ""} onClick={() => setAgent(item.id)}>{item.label}</button>)}</nav>
      <div className="pm-installed-table-wrap"><table className="pm-installed-table"><thead><tr><th>插件</th><th>安装到</th><th>状态</th><th>管理</th></tr></thead><tbody>
        {installed.filter((plugin) => !agent || (plugin.targets ?? []).some((target) => target.runtime === agent)).map((plugin) => <Fragment key={plugin.id}>
          <tr><td><div className="pm-table-plugin"><Icon name={plugin.name} /><div><strong>{titleOf(plugin)}</strong><small>{plugin.version ?? "版本未知"}</small></div></div></td>
            <td>{(plugin.targets ?? []).length ? [...new Set((plugin.targets ?? []).map((target) => agentLabel(target.runtime)))].join(" · ") : "中央库缓存"}</td>
            <td><span className={(plugin.targets ?? []).some((target) => target.status !== "installed") ? "pm-warning-text" : "pm-success"}>{!(plugin.targets ?? []).length ? "待分发" : (plugin.targets ?? []).some((target) => target.status !== "installed") ? "需要检查" : (plugin.targets ?? []).every((target) => target.native) ? "原生安装已核验" : "Skill 已部署"}</span></td>
            <td><button type="button" className="pm-link" onClick={() => setExpanded(expanded === plugin.id ? null : plugin.id)}>{expanded === plugin.id ? "收起" : "查看"}</button></td>
          </tr>
          {expanded === plugin.id && <tr className="pm-expanded-row"><td colSpan={4}>
            <div className="pm-section-heading"><div><h4>安装详情</h4><small>来源：{findEntry(plugin)?.market.source ?? plugin.marketplaceId}</small></div><button type="button" className="pm-link" onClick={() => { const entry = findEntry(plugin); if (entry) showInstalledDetail(entry); }} disabled={!findEntry(plugin)}>添加安装目标</button></div>
            {!(plugin.targets ?? []).length && <p className="pm-warning">历史中央库记录；Agent 安装状态未知。原有全局 MCP 开关请到 MCP 页管理。</p>}
            {(plugin.targets ?? []).filter((target) => !agent || target.runtime === agent).map((target) => <div className="pm-target-row" key={target.id}>
              <div><strong>{agentLabel(target.runtime)}</strong><small>{target.native ? "原生插件 · 全局" : target.scope === "global" ? "Skill · 全局" : target.projectPath}</small></div>
              <span className={target.status === "installed" ? "pm-success" : "pm-warning-text"}>{statusLabel(target)}</span>
              <div className="pm-row-actions">{target.status !== "installed" && <button type="button" className="pm-outline" disabled={busy || !findEntry(plugin)} onClick={() => { const entry = findEntry(plugin); if (entry) { showInstalledDetail(entry); setMode(target.native ? target.native.runtime === "codex" ? "native_codex" : "native_claude" : "skills"); setRuntimes([target.runtime]); setScope(target.scope); setProjectPath(target.projectPath ?? ""); } }}>重试</button>}<button type="button" className="pm-subtle" disabled={busy || UI_PREVIEW} onClick={() => setRemoveSelection({ plugin, target })}>移除</button></div>
              {target.lastError && <p className="pm-target-error">{target.lastError}</p>}
            </div>)}
            {(plugin.targets ?? []).some((target) => !target.native) && (plugin.mcpServerNames.length > 0 || plugin.hasHooks || plugin.hasAgents) && <p className="pm-muted">Skill 分发模式不安装 MCP、Hooks 或子 Agent。</p>}
          </td></tr>}
        </Fragment>)}
      </tbody></table></div>
      {!installed.some((plugin) => !agent || (plugin.targets ?? []).some((target) => target.runtime === agent)) && <p className="pm-empty">此 Agent 暂无安装记录。</p>}
    </>}
    {curationOpen && <PluginCurationDialog preview={UI_PREVIEW} initialMarket={curationMarket} onClose={() => setCurationOpen(false)} onSaved={(fresh) => { snapshotSequence.current += 1; setSnapshot(fresh); setLoadError(null); setCurationOpen(false); setNotice("精选目录已保存，已有 Agent 安装保持不变。"); }} />}
    {advanced && <Modal className="pm-advanced crewvela-plugin-center" ariaLabel="高级插件来源" onClose={() => { if (!busy) setAdvanced(false); }}><div className="pm-dialog-heading"><h2>高级 · 市场来源</h2><button type="button" className="pm-subtle" aria-label="关闭高级" onClick={() => setAdvanced(false)} disabled={busy}><XIcon size={22} /></button></div><p className="pm-muted">添加候选来源后，再勾选进入精选。新增条目不会自动显示在首页，来源内容需自行审阅。</p><div className="pm-official-source"><div><strong>OpenAI 官方候选</strong><small>固定提交 0722921d5542 · 接入会下载市场目录，接入后仍需人工勾选精选。</small></div><button type="button" className="pm-outline" disabled={busy || UI_PREVIEW} onClick={() => operation(async () => { await invoke("plugin_market_add_official"); await refresh(); setAdvanced(false); setCurationMarket("openai-curated"); setCurationOpen(true); })}>接入 OpenAI 官方候选</button></div><form className="pm-add-source" onSubmit={(event) => { event.preventDefault(); void operation(async () => { const error = validateMarketSource(source); if (error) throw new Error(error); await invoke("plugin_market_add", { source: source.trim() }); await refresh(); setSource(""); setNotice("已添加市场来源"); }); }}><input disabled={busy} aria-label="市场源" placeholder="owner/repo、Git URL 或本地目录" value={source} onChange={(event) => setSource(event.target.value)} /><button type="submit" className="pm-primary" disabled={busy || UI_PREVIEW}>添加来源</button></form>{markets.map((market) => <div className="pm-market-row" key={market.id}><PackageIcon size={22} /><div><strong>{market.sourceKind === "builtin" ? "CrewVela 内置精选" : market.id}</strong><small>{market.source}</small>{market.lastError && <p role="alert">{market.lastError}</p>}</div><button type="button" className="pm-subtle" onClick={() => { setCurationMarket(market.id); setCurationOpen(true); setAdvanced(false); }}>挑选</button>{market.sourceKind !== "builtin" && <button type="button" className="pm-subtle" disabled={busy || UI_PREVIEW} onClick={() => operation(async () => { await invoke("plugin_market_refresh", { id: market.id }); await refresh(); setNotice("已刷新市场来源"); })}>刷新</button>}</div>)}<p className="pm-muted">前往设置的「集成」分组，分别在「Skills」管理技能、在「MCP」管理服务器。</p>{notice && <p className="pm-notice" role="status">{notice}</p>}</Modal>}
    {removeSelection && <Modal className="pm-confirm crewvela-plugin-center" ariaLabel="确认移除安装目标" onClose={() => { if (!busy) setRemoveSelection(null); }}><h3>从 {agentLabel(removeSelection.target.runtime)} 移除？</h3><p>{titleOf(removeSelection.plugin)} · {removeSelection.target.scope === "global" ? "全局" : removeSelection.target.projectPath}</p><p className="pm-muted">{removeSelection.target.native ? removeSelection.target.native.runtime === "codex" ? "通过 Codex CLI 移除这个插件；来源、版本或内容变化时拒绝删除，其他插件保留。" : "通过 Claude CLI 卸载这个插件，保留插件数据；路径变化或安装未核验会拒绝删除。" : "仅撤下这个安装目标的 Skill，其他 Agent 与中央库保留。"}</p><div className="pm-row-actions"><button type="button" className="pm-subtle" disabled={busy} onClick={() => setRemoveSelection(null)}>取消</button><button type="button" className="pm-primary" disabled={busy} onClick={() => operation(async () => { await invoke("plugin_market_remove_target", { installedId: removeSelection.plugin.id, targetId: removeSelection.target.id }); await refresh(); setRemoveSelection(null); setNotice("已移除此安装目标"); })}>确认移除</button></div>{notice && <p className="pm-warning" role="alert">{notice}</p>}</Modal>}
  </div>;
}
