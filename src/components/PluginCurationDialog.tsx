import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { MagnifyingGlassIcon, XIcon } from "@phosphor-icons/react";
import { Modal } from "./ui/Modal";
import { CURATED_PLUGIN_PREVIEW, compatibilityLabel, filterMarketPlugins, installedKey, PLUGIN_AGENTS, PLUGIN_CATEGORIES, pluginCategory, pluginTitle, validSelection, type PluginCandidateSnapshot, type PluginMarketSnapshot } from "../pluginMarket";

interface Props {
  preview: boolean;
  initialMarket: string | null;
  onClose: () => void;
  onSaved: (snapshot: PluginMarketSnapshot) => void;
}

export function PluginCurationDialog({ preview, initialMarket, onClose, onSaved }: Props) {
  const [candidates, setCandidates] = useState<PluginCandidateSnapshot | null>(null);
  const [draft, setDraft] = useState<string[]>([]);
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState("全部");
  const [market, setMarket] = useState<string | null>(initialMarket);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const saveInFlight = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const [missing, setMissing] = useState(0);
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let cancelled = false;
    setLoading(true); setError(null);
    const request = preview ? Promise.resolve({ marketplaces: CURATED_PLUGIN_PREVIEW.marketplaces, selectionConfigured: true,
      selectedPluginIds: CURATED_PLUGIN_PREVIEW.selectedPluginIds ?? [], componentHints: {} }) : invoke<PluginCandidateSnapshot>("plugin_market_candidates");
    void request.then((snapshot) => {
      if (cancelled) return;
      const valid = validSelection(snapshot.marketplaces, snapshot.selectedPluginIds);
      setCandidates(snapshot); setDraft(valid); setMissing(snapshot.selectedPluginIds.length - valid.length);
    }).catch((reason) => { if (!cancelled) setError(`读取候选失败：${String(reason)}`); }).finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [preview, retry]);
  const entries = useMemo(() => filterMarketPlugins(candidates?.marketplaces ?? [], market, query).filter(({ plugin }) =>
    category === "全部" || pluginCategory(plugin.category) === category), [candidates, market, query, category]);
  const sourceFailed = candidates?.marketplaces.some((source) => source.lastError) ?? false;
  const count = candidates?.marketplaces.reduce((sum, source) => sum + source.plugins.length, 0) ?? 0;
  const toggle = (id: string, checked: boolean) => setDraft((current) => checked ? current.includes(id) ? current : [...current, id] : current.filter((entry) => entry !== id));
  const save = async () => {
    if (saveInFlight.current || loading || !candidates || preview || sourceFailed) return;
    saveInFlight.current = true;
    setSaving(true); setError(null);
    try {
      const snapshot = await invoke<PluginMarketSnapshot>("plugin_market_save_selection", { pluginIds: draft });
      onSaved(snapshot);
    } catch (reason) { setError(`保存失败：${String(reason)}。当前勾选已保留，请重试。`); }
    finally { saveInFlight.current = false; setSaving(false); }
  };
  return <Modal className="pm-curation crewvela-plugin-center" ariaLabel="挑选精选插件" onClose={() => { if (!saving) onClose(); }}>
    <div className="pm-dialog-heading"><div><h2>挑选你的精选插件</h2><p className="pm-muted">勾选后进入首页；这里只保存目录，不会安装或下载插件。</p></div><button type="button" className="pm-subtle" disabled={saving} aria-label="关闭挑选精选" onClick={onClose}><XIcon size={22} /></button></div>
    <div className="pm-curation-filters"><label className="pm-search"><MagnifyingGlassIcon size={18} /><input aria-label="搜索候选插件" placeholder="搜索名称、用途或作者" value={query} onChange={(event) => setQuery(event.target.value)} /></label><label>来源<select aria-label="候选来源" value={market ?? ""} onChange={(event) => setMarket(event.target.value || null)}><option value="">全部来源</option>{candidates?.marketplaces.map((source) => <option key={source.id} value={source.id}>{source.sourceKind === "builtin" ? "随应用提供" : source.id}</option>)}</select></label></div>
    <nav className="pm-categories" aria-label="候选分类">{PLUGIN_CATEGORIES.map((item) => <button type="button" key={item} className={category === item ? "active" : ""} onClick={() => setCategory(item)}>{item}</button>)}</nav>
    <div className="pm-curation-boundary"><strong>安装方式与 Agent 边界</strong><p>Codex / Claude 原生插件按各自格式全局整包安装，不互相转换。只有已验证独立 Skill 可兼容分发；连接器待授权仅展示。原生安装核验不代表宿主工具、账号或完整功能可用。</p><details><summary>查看各 Agent 适配情况</summary><div className="pm-curation-agent-grid">{PLUGIN_AGENTS.map((agent) => <span key={agent.id}><strong>{agent.label}</strong><small>{agent.support ? (agent.id === "claude_code" || agent.id === "codex") ? "各自原生插件（需 CLI 校验） / 已验证 Skill" : "Skill 分发通路；不保证整包兼容" : "安装适配待验证"}</small></span>)}</div></details></div>
    {sourceFailed && <p className="pm-warning" role="alert">部分来源读取失败，暂不能保存，原有精选名单保留。<button type="button" className="pm-link" onClick={() => setRetry((value) => value + 1)}>重新读取</button></p>}
    {!sourceFailed && missing > 0 && <p className="pm-warning">{missing} 个历史精选已不在候选目录，保存后会移出目录；已有安装保留。</p>}
    {error && <p className="pm-warning" role="alert">{error}{!candidates && <button type="button" className="pm-link" onClick={() => setRetry((value) => value + 1)}>重新读取</button>}</p>}
    <div className="pm-curation-results">
      {loading ? <p className="pm-empty">正在读取缓存候选…</p> : candidates && <>
        {candidates.marketplaces.filter((source) => source.lastError).map((source) => <p key={source.id} className="pm-warning">{source.id}：{source.lastError}</p>)}
        <div className="pm-catalog-grid">{entries.map(({ market: source, plugin }) => {
          const id = installedKey(source.id, plugin.name);
          return <label className={`pm-candidate-card${draft.includes(id) ? " selected" : ""}`} key={id}><input type="checkbox" disabled={saving} checked={draft.includes(id)} onChange={(event) => toggle(id, event.target.checked)} aria-label={`精选 ${pluginTitle(plugin)}（${source.id}）`} /><div><h4>{pluginTitle(plugin)}</h4><small>{source.sourceKind === "builtin" ? "随应用提供" : source.id} · {pluginCategory(plugin.category)}</small><p>{plugin.description || "用途未提供，请查阅来源说明。"}</p><small className="pm-install-state">{compatibilityLabel(plugin)}</small><small className="pm-component-hint">{candidates.componentHints[id] ?? (source.sourceKind === "builtin" ? "1 个 Skill 包 · 固定来源" : "组件未知 · 安装前检查")}</small>{plugin.compatibility?.dependencies?.map((item) => <small className="pm-component-hint" key={item}>{item}</small>)}{plugin.compatibility?.license && <small className="pm-component-hint">许可：{plugin.compatibility.license}</small>}<span className="pm-candidate-source">{plugin.sourceDisplay}</span>{!plugin.installable && <small className="pm-warning-text">{plugin.note || "该来源当前尚无安装适配；可以收藏到目录"}</small>}</div></label>;
        })}</div>
        {!entries.length && <p className="pm-empty">没有符合筛选条件的候选插件。</p>}
      </>}
    </div>
    <footer className="pm-curation-footer"><div><strong>已选 {draft.length} / {count}</strong><small>{draft.length === 0 ? "保存后首页精选为空，已有 Agent 安装保留。" : "只有选中项进入首页，其他候选不会默认显示。"}</small></div><div className="pm-row-actions"><button type="button" className="pm-subtle" onClick={onClose} disabled={saving}>取消</button><button type="button" className="pm-primary" disabled={saving || loading || !candidates || preview || sourceFailed} onClick={save}>{saving ? "正在保存…" : draft.length === 0 ? "保存空精选" : "保存精选"}</button></div></footer>
  </Modal>;
}
