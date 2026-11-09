import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowLeftIcon, ArrowRightIcon, CodeIcon, FileTextIcon, MagnifyingGlassIcon, PackageIcon, SlidersHorizontalIcon, TableIcon, TestTubeIcon, XIcon } from "@phosphor-icons/react";
import { Modal } from "./ui/Modal";
import type { Project } from "../types";
import { agentLabel, compatibilityLabel, pluginModes, pluginSupportsAgent, filterMarketPlugins, installedKey, PLUGIN_AGENTS, PLUGIN_CATEGORIES, CURATED_PLUGIN_PREVIEW, pluginCategory, pluginTitle, validateMarketSource, type InstalledPlugin, type MarketPluginEntry, type PluginMarketSnapshot, type PluginTarget } from "../pluginMarket";
import "./plugin-market.css";
import { PluginCurationDialog } from "./PluginCurationDialog";

const UI_PREVIEW = new URLSearchParams(window.location.search).get("ui-preview") === "1";
type Adapter = { runtime: string; installed: boolean };
function Icon({ name }: { name: string }) {
  const Component = name.includes("design") ? CodeIcon : name.includes("testing") ? TestTubeIcon : name.includes("xlsx") ? TableIcon : FileTextIcon;
  return <span className={"pm-icon pm-icon-" + (name.includes("design") ? "purple" : name.includes("testing") ? "green" : "blue")}><Component size={29} weight="duotone" /></span>;
}
const statusLabel = (target: PluginTarget) => target.status === "installed" ? target.native ? "原生插件已安装" : "Skill 已部署" : target.status === "failed" ? "安装失败" : "需要检查";

export function PluginMarketPanel({ projects = [] }: { projects?: Project[] }) {
  const [snapshot, setSnapshot] = useState<PluginMarketSnapshot | null>(UI_PREVIEW ? CURATED_PLUGIN_PREVIEW : null);
  const [adapters, setAdapters] = useState<Adapter[]>([]);
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState("全部");
  const [agent, setAgent] = useState("");
  const [installableOnly, setInstallableOnly] = useState(false);
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
  const entries = useMemo(() => filterMarketPlugins(markets, selectedMarket, query).filter(({ plugin, market }) =>
    (category === "全部" || pluginCategory(plugin.category) === category) && (!installableOnly || plugin.installable) &&
    (!agent || pluginSupportsAgent(plugin, market.sourceKind === "builtin", agent))), [markets, selectedMarket, query, category, installableOnly, agent]);
  const findEntry = (plugin: InstalledPlugin) => [...(snapshot?.installedCatalog ?? []), ...markets.flatMap((market) => market.plugins.map((entry) => ({ market, plugin: entry })))].find((entry) => installedKey(entry.market.id, entry.plugin.name) === plugin.id);
  const openDetail = (entry: MarketPluginEntry) => { setDetail(entry); const nextMode = pluginModes(entry.plugin, entry.market.sourceKind === "builtin")[0] ?? "unsupported"; setMode(nextMode); setRuntimes(nextMode === "native_codex" ? ["codex"] : nextMode === "native_claude" ? ["claude_code"] : agent && PLUGIN_AGENTS.find((item) => item.id === agent)?.support ? [agent] : []); if (nextMode.startsWith("native_")) setScope("global"); setTrust(false); setNotice(null); };
  const titleOf = (plugin: InstalledPlugin) => { const entry = findEntry(plugin); return entry ? pluginTitle(entry.plugin) : plugin.name; };
  const detailInstalled = detail ? installed.find((plugin) => plugin.id === installedKey(detail.market.id, detail.plugin.name)) : null;
  const install = () => operation(async () => {
    if (!detail) return;
    const result = await invoke<InstalledPlugin>("plugin_market_install", { input: { marketplaceId: detail.market.id, pluginName: detail.plugin.name, runtimes, scope, projectPath: scope === "project" ? projectPath : null, trustConfirmed: trust, mode } });
    await refresh();
    const failures = (result.targets ?? []).filter((target) => runtimes.includes(target.runtime) && target.status !== "installed");
    setNotice(failures.length ? failures.map((target) => agentLabel(target.runtime) + "：" + target.lastError).join("；") : mode.startsWith("native_") ? agentLabel(mode === "native_codex" ? "codex" : "claude_code") + " 原生插件安装记录已核验。请重开会话；宿主工具、账号、密钥和依赖仍需按上游要求配置。" : "Skill 已分发到 " + runtimes.map(agentLabel).join("、") + "。重开会话后使用；原生插件、MCP、Hooks 与子 Agent 未安装。");
  });
  return <div className="crewvela-plugin-center">
    <div className="pm-top-actions"><button type="button" className="pm-outline" onClick={() => { setCurationMarket(null); setCurationOpen(true); }}>挑选精选</button><button type="button" className="pm-subtle" onClick={() => setAdvanced(true)}><SlidersHorizontalIcon size={17} />高级</button></div>
    {UI_PREVIEW && <p className="pm-warning">设计预览 · 示例数据 · 安装操作已禁用</p>}
    {loadError && <div className="pm-warning" role="alert">读取插件失败：{loadError}<button type="button" disabled={busy} onClick={() => operation(refresh)}>重试</button></div>}
    {notice && !detail && <p className="pm-notice" role="status">{notice}</p>}
    {view === "catalog" ? <>
      <header className="pm-intro"><small>CrewVela 精选</small><h2>为每项任务，配好工具</h2><p>挑选实用的插件与 Skill，安装到你常用的 Agent。</p></header>
      <div className="pm-toolbar"><label className="pm-search"><MagnifyingGlassIcon size={20} /><input aria-label="搜索插件或使用场景" placeholder="搜索插件或使用场景" value={query} onChange={(event) => setQuery(event.target.value)} /></label><label className="pm-agent-filter">适用 Agent<select aria-label="适用 Agent" value={agent} onChange={(event) => setAgent(event.target.value)}><option value="">全部</option>{PLUGIN_AGENTS.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}</select></label><label className="pm-check"><input type="checkbox" checked={installableOnly} onChange={(event) => setInstallableOnly(event.target.checked)} />仅看可安装</label></div>
      <section className="pm-mine"><div className="pm-section-heading"><h3>我的插件 <span>{installed.length}</span></h3><button type="button" className="pm-link" onClick={() => setView("installed")}>管理安装 <ArrowRightIcon size={15} /></button></div>
        {installed.length ? <div className="pm-summary-grid">{installed.slice(0, 3).map((plugin) => <button type="button" className="pm-summary" key={plugin.id} onClick={() => setView("installed")}><Icon name={plugin.name} /><span><strong>{titleOf(plugin)}</strong><small>{(plugin.targets ?? []).length ? (plugin.targets ?? []).map((target) => agentLabel(target.runtime)).filter((value, index, all) => all.indexOf(value) === index).join(" · ") : "中央库缓存 · 尚无目标安装记录"}</small></span><i className={(plugin.targets ?? []).some((target) => target.status !== "installed") ? "attention" : ""}>{(plugin.targets ?? []).length ? (plugin.targets ?? []).some((target) => target.status !== "installed") ? "需检查" : (plugin.targets ?? []).every((target) => target.native) ? "原生插件已安装" : (plugin.targets ?? []).some((target) => target.native) ? "已安装" : "Skill 已部署" : "待分发"}</i></button>)}</div> : <p className="pm-empty-compact">还没有安装记录，从下面的精选目录开始。</p>}
      </section>
      <section><div className="pm-section-heading"><h3>精选目录 <small>按工作场景挑选</small></h3><span className="pm-muted">{entries.length} 项</span></div><nav className="pm-categories" aria-label="插件分类">{PLUGIN_CATEGORIES.map((item) => <button type="button" className={category === item ? "active" : ""} key={item} onClick={() => setCategory(item)}>{item}</button>)}</nav>
        {selectedMarket && <p className="pm-muted">当前来源：{selectedMarket}<button type="button" className="pm-link" onClick={() => setSelectedMarket(null)}>显示全部</button></p>}
        <div className="pm-catalog-grid">{entries.map((entry) => <article className="pm-card" key={installedKey(entry.market.id, entry.plugin.name)}><div className="pm-card-main"><Icon name={entry.plugin.name} /><div><h4>{pluginTitle(entry.plugin)}</h4><small>{entry.market.sourceKind === "builtin" ? "CrewVela 精选" : entry.market.ownerName} · {pluginCategory(entry.plugin.category)}</small><p>{entry.plugin.description || "查看来源与安装内容。"}</p></div></div><footer><span>{entry.market.sourceKind === "builtin" ? "1 个 Skill · 固定版本" : compatibilityLabel(entry.plugin)}</span><button type="button" className="pm-outline" disabled={!entry.plugin.installable} onClick={() => openDetail(entry)}>{entry.plugin.installable ? "选择 Agent" : (entry.plugin.compatibility?.status === "needs_auth" ? "待授权" : "尚未适配")}</button></footer></article>)}</div>
        {!snapshot && !loadError && <p className="pm-empty">正在读取精选目录…</p>}
        {snapshot && !entries.length && <div className="pm-empty"><p>{!snapshot.selectionConfigured ? "尚未选择精选" : !(snapshot.selectedPluginIds ?? []).length ? "精选目录为空" : "没有符合条件的精选插件"}</p><p className="pm-muted">{!snapshot.selectionConfigured ? "先从候选目录勾选你需要的插件，保存后才会显示在这里。" : !(snapshot.selectedPluginIds ?? []).length ? "你已保存空精选；已有 Agent 安装保留。" : "试试其他分类或关键词。"}</p><button type="button" className="pm-outline" onClick={() => { setCurationMarket(null); setCurationOpen(true); }}>开始挑选</button></div>}

      </section>
    </> : <>
      <button type="button" className="pm-link" onClick={() => setView("catalog")}><ArrowLeftIcon size={17} />返回精选目录</button>
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
            <div className="pm-section-heading"><div><h4>安装详情</h4><small>来源：{findEntry(plugin)?.market.source ?? plugin.marketplaceId}</small></div><button type="button" className="pm-link" onClick={() => { const entry = findEntry(plugin); if (entry) openDetail(entry); }} disabled={!findEntry(plugin)}>添加安装目标</button></div>
            {!(plugin.targets ?? []).length && <p className="pm-warning">历史中央库记录；Agent 安装状态未知。原有全局 MCP 开关请到 MCP 页管理。</p>}
            {(plugin.targets ?? []).filter((target) => !agent || target.runtime === agent).map((target) => <div className="pm-target-row" key={target.id}>
              <div><strong>{agentLabel(target.runtime)}</strong><small>{target.native ? "原生插件 · 全局" : target.scope === "global" ? "Skill · 全局" : target.projectPath}</small></div>
              <span className={target.status === "installed" ? "pm-success" : "pm-warning-text"}>{statusLabel(target)}</span>
              <div className="pm-row-actions">{target.status !== "installed" && <button type="button" className="pm-outline" disabled={busy || !findEntry(plugin)} onClick={() => { const entry = findEntry(plugin); if (entry) { openDetail(entry); setMode(target.native ? target.native.runtime === "codex" ? "native_codex" : "native_claude" : "skills"); setRuntimes([target.runtime]); setScope(target.scope); setProjectPath(target.projectPath ?? ""); } }}>重试</button>}<button type="button" className="pm-subtle" disabled={busy || UI_PREVIEW} onClick={() => setRemoveSelection({ plugin, target })}>移除</button></div>
              {target.lastError && <p className="pm-target-error">{target.lastError}</p>}
            </div>)}
            {(plugin.targets ?? []).some((target) => !target.native) && (plugin.mcpServerNames.length > 0 || plugin.hasHooks || plugin.hasAgents) && <p className="pm-muted">Skill 分发模式不安装 MCP、Hooks 或子 Agent。</p>}
          </td></tr>}
        </Fragment>)}
      </tbody></table></div>
      {!installed.some((plugin) => !agent || (plugin.targets ?? []).some((target) => target.runtime === agent)) && <p className="pm-empty">此 Agent 暂无安装记录。</p>}
    </>}
    {detail && <Modal className="pm-dialog crewvela-plugin-center" ariaLabel="插件详情与安装" onClose={() => { if (!busy) setDetail(null); }}>
      <div className="pm-dialog-heading"><small>插件详情与安装</small><button type="button" className="pm-subtle" aria-label="关闭插件详情" disabled={busy} onClick={() => setDetail(null)}><XIcon size={22} /></button></div>
      <div className="pm-detail-layout">
        <aside className="pm-plugin-overview">
          <Icon name={detail.plugin.name} /><h2>{pluginTitle(detail.plugin)}</h2><p>{detail.plugin.description}</p>
          {detail.plugin.compatibility?.dependencies?.length ? <div className="pm-host-dependencies"><h4>宿主与使用条件</h4>{detail.plugin.compatibility.dependencies.map((item) => <p key={item}>{item}</p>)}{detail.plugin.compatibility.license && <p>许可：{detail.plugin.compatibility.license}</p>}</div> : null}<h4>包含的组件</h4><p>{detail.plugin.compatibility?.components.length ? detail.plugin.compatibility.components.join(" · ") : detailInstalled ? detailInstalled.skillNames.length + " 个 Skill" : detail.market.sourceKind === "builtin" ? "1 个 Skill 及其资源文件" : "组件未知 · 安装前检查"}</p>
          <div className="pm-source"><strong>来源与版本</strong><span>{detail.plugin.sourceDisplay}</span>{detail.market.url && <a href={detail.market.url} target="_blank" rel="noreferrer">查看上游仓库 ↗</a>}</div>
          {mode === "native_codex" && detail.market.id === "openai-curated" && <p className="pm-muted">官方来源固定为 0722921d5542。当前 CLI 保留官方市场名，安装使用 CrewVela 自有目录 crewvela-openai-curated；原包、相对引用与许可保留。</p>}
          <p className="pm-warning">{mode.startsWith("native_") ? "通过 " + (mode === "native_codex" ? "Codex" : "Claude") + " CLI 全局安装整包，保留包内相对引用；宿主工具和依赖仍需核对，不代表功能已运行验证。" : "当前分发 Skill；原生插件、MCP、Hooks 与子 Agent 未安装。依赖和账号按上游说明配置。"}</p>
        </aside>
        <section className="pm-install-column">
          <h3>安装到哪些 Agent</h3><p className="pm-muted">选择安装目标，各 Agent 独立管理。</p>
          {detail.market.sourceKind !== "builtin" && <div className="pm-install-mode">{pluginModes(detail.plugin, false).map((candidate) => <label key={candidate}><input type="radio" name="plugin-install-mode" checked={mode === candidate} disabled={busy} onChange={() => { setMode(candidate); if (candidate.startsWith("native_")) { setScope("global"); setRuntimes([candidate === "native_codex" ? "codex" : "claude_code"]); } else setRuntimes([]); setTrust(false); }} />{candidate === "native_codex" ? "Codex 原生插件 · 全局" : candidate === "native_claude" ? "Claude 原生插件 · 全局" : "Skill 兼容分发"}</label>)}</div>}
          {detail.plugin.compatibility && <p className="pm-muted">{compatibilityLabel(detail.plugin)} · {detail.plugin.compatibility.note}</p>}
          <div className="pm-scope-row"><label>作用范围<select aria-label="安装作用域" value={scope} disabled={busy || mode.startsWith("native_")} onChange={(event) => setScope(event.target.value)}><option value="global">全局 · 此 Agent 的所有项目</option><option value="project">项目 · 仅选定目录</option></select></label>{scope === "project" && <label>项目<select aria-label="安装项目" value={projectPath} disabled={busy} onChange={(event) => setProjectPath(event.target.value)}><option value="">选择项目</option>{projects.map((project) => <option key={project.id} value={project.path}>{project.name}</option>)}{projectPath && !projects.some((project) => project.path === projectPath) && <option value={projectPath}>{projectPath}</option>}</select></label>}</div>
          <div className="pm-agent-options">{PLUGIN_AGENTS.map((item) => <label key={item.id} className={!item.support ? "unsupported" : ""}><input type="checkbox" disabled={!item.support || busy || (mode.startsWith("native_") && item.id !== (mode === "native_codex" ? "codex" : "claude_code")) || mode === "unsupported"} checked={runtimes.includes(item.id)} onChange={(event) => setRuntimes(event.target.checked ? [...runtimes, item.id] : runtimes.filter((id) => id !== item.id))} /><span><strong>{item.label}</strong><small>{item.kind} · {mode.startsWith("native_") ? item.id === (mode === "native_codex" ? "codex" : "claude_code") ? "使用 CLI 安装整包并核验记录" : "此包尚未适配该 Agent；不拆分兼容 Skill" : item.hint}</small></span><em>{mode.startsWith("native_") && item.id !== (mode === "native_codex" ? "codex" : "claude_code") ? "未适配" : !item.support ? "适配待验证" : adapters.find((adapter) => adapter.runtime === item.id)?.installed ? mode.startsWith("native_") ? "原生安装" : "可分发 Skill" : "CLI 未检测 / 可预置"}</em></label>)}</div>
          <label className="pm-trust"><input type="checkbox" checked={trust} disabled={busy} onChange={(event) => setTrust(event.target.checked)} />{mode.startsWith("native_") ? "我已审阅并信任此来源，允许安装插件与附带代码。" : "我已审阅并信任此来源，允许分发 Skill 与附带代码。"}</label>
          <div className="pm-install-footer"><small>安装后重开 Agent 会话。</small><button type="button" className="pm-primary" disabled={busy || UI_PREVIEW || mode === "unsupported" || !trust || !runtimes.length || (scope === "project" && !projectPath)} onClick={install}>{busy ? "正在安装…" : "安装到 " + runtimes.length + " 个 Agent"}</button></div>
        </section>
      </div>{notice && <p className="pm-notice" role="status">{notice}</p>}
    </Modal>}
    {curationOpen && <PluginCurationDialog preview={UI_PREVIEW} initialMarket={curationMarket} onClose={() => setCurationOpen(false)} onSaved={(fresh) => { snapshotSequence.current += 1; setSnapshot(fresh); setLoadError(null); setCurationOpen(false); setNotice("精选目录已保存，已有 Agent 安装保持不变。"); }} />}
    {advanced && <Modal className="pm-advanced crewvela-plugin-center" ariaLabel="高级插件来源" onClose={() => { if (!busy) setAdvanced(false); }}><div className="pm-dialog-heading"><h2>高级 · 市场来源</h2><button type="button" className="pm-subtle" aria-label="关闭高级" onClick={() => setAdvanced(false)} disabled={busy}><XIcon size={22} /></button></div><p className="pm-muted">添加候选来源后，再勾选进入精选。新增条目不会自动显示在首页，来源内容需自行审阅。</p><div className="pm-official-source"><div><strong>OpenAI 官方候选</strong><small>固定提交 0722921d5542 · 接入会下载市场目录，接入后仍需人工勾选精选。</small></div><button type="button" className="pm-outline" disabled={busy || UI_PREVIEW} onClick={() => operation(async () => { await invoke("plugin_market_add_official"); await refresh(); setAdvanced(false); setCurationMarket("openai-curated"); setCurationOpen(true); })}>接入 OpenAI 官方候选</button></div><form className="pm-add-source" onSubmit={(event) => { event.preventDefault(); void operation(async () => { const error = validateMarketSource(source); if (error) throw new Error(error); await invoke("plugin_market_add", { source: source.trim() }); await refresh(); setSource(""); setNotice("已添加市场来源"); }); }}><input disabled={busy} aria-label="市场源" placeholder="owner/repo、Git URL 或本地目录" value={source} onChange={(event) => setSource(event.target.value)} /><button type="submit" className="pm-primary" disabled={busy || UI_PREVIEW}>添加来源</button></form>{markets.map((market) => <div className="pm-market-row" key={market.id}><PackageIcon size={22} /><div><strong>{market.sourceKind === "builtin" ? "CrewVela 内置精选" : market.id}</strong><small>{market.source}</small>{market.lastError && <p role="alert">{market.lastError}</p>}</div><button type="button" className="pm-subtle" onClick={() => { setCurationMarket(market.id); setCurationOpen(true); setAdvanced(false); }}>挑选</button>{market.sourceKind !== "builtin" && <button type="button" className="pm-subtle" disabled={busy || UI_PREVIEW} onClick={() => operation(async () => { await invoke("plugin_market_refresh", { id: market.id }); await refresh(); setNotice("已刷新市场来源"); })}>刷新</button>}</div>)}<p className="pm-muted">前往设置的「集成」分组，分别在「Skill 中心」管理技能、在「MCP」管理服务器。</p>{notice && <p className="pm-notice" role="status">{notice}</p>}</Modal>}
    {removeSelection && <Modal className="pm-confirm crewvela-plugin-center" ariaLabel="确认移除安装目标" onClose={() => { if (!busy) setRemoveSelection(null); }}><h3>从 {agentLabel(removeSelection.target.runtime)} 移除？</h3><p>{titleOf(removeSelection.plugin)} · {removeSelection.target.scope === "global" ? "全局" : removeSelection.target.projectPath}</p><p className="pm-muted">{removeSelection.target.native ? removeSelection.target.native.runtime === "codex" ? "通过 Codex CLI 移除这个插件；来源、版本或内容变化时拒绝删除，其他插件保留。" : "通过 Claude CLI 卸载这个插件，保留插件数据；路径变化或安装未核验会拒绝删除。" : "仅撤下这个安装目标的 Skill，其他 Agent 与中央库保留。"}</p><div className="pm-row-actions"><button type="button" className="pm-subtle" disabled={busy} onClick={() => setRemoveSelection(null)}>取消</button><button type="button" className="pm-primary" disabled={busy} onClick={() => operation(async () => { await invoke("plugin_market_remove_target", { installedId: removeSelection.plugin.id, targetId: removeSelection.target.id }); await refresh(); setRemoveSelection(null); setNotice("已移除此安装目标"); })}>确认移除</button></div>{notice && <p className="pm-warning" role="alert">{notice}</p>}</Modal>}
  </div>;
}
