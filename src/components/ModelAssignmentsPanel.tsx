import { Fragment, useEffect, useId, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowRightIcon, MagnifyingGlassIcon } from "@phosphor-icons/react";
import type { ProviderConfig } from "../types";
import { AgentAvatar } from "./AgentAvatar";
import { assignmentDraft, assignmentPreview, assignmentSource, ASSIGNMENT_RUNTIME_LABELS, connectionLabel, filterAssignments, modelSuggestions, type ModelAssignmentRow, type ModelAssignmentSnapshot, type ModelSelection } from "../modelAssignments";
import "./model-assignments.css";

const PREVIEW = new URLSearchParams(location.search).get("ui-preview") === "1";
export function ModelAssignmentsPanel({ providers, onManageConnections }: { providers: ProviderConfig[]; onManageConnections: () => void }) {
  const [data, setData] = useState<ModelAssignmentSnapshot | null>(PREVIEW ? assignmentPreview() : null);
  const [project, setProject] = useState("");
  const [scope, setScope] = useState("all");
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<ModelSelection>({ connectionId: null, model: null });
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(!PREVIEW);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const sequence = useRef(0);
  const saving = useRef(false);
  const mounted = useRef(true);
  const modelListId = useId();
  const panelRef = useRef<HTMLElement>(null);
  const tableRef = useRef<HTMLDivElement>(null);
  const load = async () => {
    const request = ++sequence.current;
    setLoading(true); setError(null);
    try {
      const snapshot = await invoke<ModelAssignmentSnapshot>("model_assignments_snapshot");
      if (mounted.current && request === sequence.current) { setData(snapshot); setEditing(false); }
    } catch (reason) { if (mounted.current && request === sequence.current) setError(`读取模型配置失败：${String(reason)}`); }
    finally { if (mounted.current && request === sequence.current) setLoading(false); }
  };
  useEffect(() => {
    mounted.current = true;
    if (!PREVIEW) void load();
    return () => { mounted.current = false; sequence.current++; };
  }, []);
  const rows = useMemo(() => filterAssignments(data?.rows ?? [], project, scope, query), [data, project, scope, query]);
  const selected = data?.rows.find(row => row.id === selectedId) ?? null;
  const blocked = busy || editing;
  const select = (row: ModelAssignmentRow, edit: boolean) => {
    if (blocked) return;
    setSelectedId(row.id); setDraft(assignmentDraft(row)); setEditing(edit && row.editable); setError(null); setNotice(null);
  };
  useEffect(() => {
    if (!editing && !rows.some(row => row.id === selectedId)) setSelectedId(rows.find(row => row.scope === "member")?.id ?? rows[0]?.id ?? null);
  }, [rows, selectedId, editing]);
  useEffect(() => {
    const panel = panelRef.current, table = tableRef.current;
    if (!panel || !table) return;
    const host = panel.closest(".settings-content");
    if (!host) return;
    const measure = () => {
      let reserved = parseFloat(getComputedStyle(panel.parentElement!).paddingBottom) || 24;
      for (let sibling = table.nextElementSibling; sibling; sibling = sibling.nextElementSibling) {
        const style = getComputedStyle(sibling);
        reserved += sibling.getBoundingClientRect().height + (parseFloat(style.marginTop) || 0) + (parseFloat(style.marginBottom) || 0);
      }
      const offset = table.getBoundingClientRect().top - host.getBoundingClientRect().top + host.scrollTop;
      table.style.setProperty("--model-table-height", `${Math.max(160, Math.floor(host.clientHeight - offset - reserved))}px`);
    };
    measure();
    const observer = new ResizeObserver(measure); observer.observe(host); observer.observe(panel);
    window.addEventListener("resize", measure);
    return () => { observer.disconnect(); window.removeEventListener("resize", measure); };
  }, [data, selectedId, editing, error, notice]);
  const save = async (restore: boolean) => {
    if (!selected || !selected.editable || saving.current || PREVIEW) return;
    saving.current = true; setBusy(true); setError(null); setNotice(null);
    try {
      const snapshot = await invoke<ModelAssignmentSnapshot>("model_assignments_save", { input: { id: selected.id, version: selected.version, connectionId: draft.connectionId, model: draft.model?.trim() || null, restore } });
      if (mounted.current) { sequence.current++; setData(snapshot); setEditing(false); setNotice(restore ? "已恢复继承；运行中的会话保持不变。" : "配置已保存；运行中的会话保持不变。恢复或新建会话时使用新配置。"); }
    } catch (reason) { if (mounted.current) setError(`保存失败：${String(reason)}。当前编辑已保留。`); }
    finally { saving.current = false; if (mounted.current) setBusy(false); }
  };
  const suggestions = selected ? modelSuggestions(data?.rows ?? [], providers, selected.runtime, draft.connectionId) : [];
  const groups = [...new Set(rows.map(row => row.scope === "runtime" ? "Agent 默认" : row.scope === "session" ? "会话启动配置" : `项目 · ${row.projectName ?? "未命名"}`))];
  const groupOf = (row: ModelAssignmentRow) => row.scope === "runtime" ? "Agent 默认" : row.scope === "session" ? "会话启动配置" : `项目 · ${row.projectName ?? "未命名"}`;
  return <section className="model-assignments" aria-label="模型统一分配" ref={panelRef}>
    <div className="dialog-heading settings-heading model-assignment-heading"><div><h2 id="settings-title">模型设置</h2><p>默认、成员与会话配置，一处查看。</p></div><div className="model-assignment-connections"><span>{providers.filter(provider => !provider.hidden).length} 个服务连接</span><button type="button" disabled={blocked} onClick={onManageConnections}>管理连接 <ArrowRightIcon size={15} /></button></div></div>
    {PREVIEW && <p className="model-assignment-info">设计预览 · 示例配置 · 保存操作已禁用</p>}
    <div className="model-assignment-toolbar"><select aria-label="分配项目" disabled={blocked || loading} value={project} onChange={event => setProject(event.target.value)}><option value="">全部项目</option>{data?.projects.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select><nav aria-label="配置范围">{[["all", "全部配置"], ["runtime", "Agent 默认"], ["member", "项目成员"], ["session", "会话配置"]].map(([id, label]) => <button type="button" key={id} disabled={blocked} aria-pressed={scope === id} className={scope === id ? "active" : ""} onClick={() => setScope(id)}>{label}</button>)}</nav><label className="model-assignment-search"><MagnifyingGlassIcon size={17} /><input aria-label="搜索成员或模型" placeholder="搜索成员或模型" value={query} disabled={blocked} onChange={event => setQuery(event.target.value)} /></label></div>
    <p className="model-assignment-priority">Agent 默认 <ArrowRightIcon size={14} /> 成员覆盖 <ArrowRightIcon size={14} /> 会话覆盖</p>
    <details className="model-assignment-help"><summary>生效范围与模型候选</summary><p>CLI 默认用于新启动和项目成员；显式选择的服务连接优先。运行中的终端不会被自动切换，原生聊天请在原会话中设置。自然语言调度解析仍使用 Claude Code 的默认连接与模型。</p><p>模型候选来自已有配置，可手动填写 CLI 支持的模型 ID；候选出现不代表该账号已获授权。</p></details>
    {error && <div className="model-assignment-error" role="alert">{error}<button type="button" disabled={busy} onClick={() => void load()}>{editing ? "重新读取并取消编辑" : "重试读取"}</button></div>}
    <div className="model-assignment-table-wrap" ref={tableRef}><table><thead><tr><th>对象</th><th>Agent</th><th>模型连接</th><th>模型</th><th>配置来源</th><th><span className="sr-only">操作</span></th></tr></thead><tbody>{groups.map(group => <Fragment key={group}><tr className="model-assignment-group"><th colSpan={6}>{group}</th></tr>{rows.filter(row => groupOf(row) === group).map(row => <tr key={row.id} className={selectedId === row.id ? "selected" : ""}>
      <td><button type="button" className="model-assignment-object" disabled={blocked} onClick={() => select(row, false)}>{row.scope === "runtime" && <AgentAvatar runtimeKind={row.runtime} decorative />}<strong>{row.name}</strong></button></td>
      <td>{ASSIGNMENT_RUNTIME_LABELS[row.runtime] ?? row.runtime}</td>
      <td>{editing && row.id === selectedId && row.connectionEditable ? <select aria-label="模型连接" value={draft.connectionId ?? ""} disabled={busy} onChange={event => setDraft(current => ({ ...current, connectionId: event.target.value || null }))}><option value="">{row.scope === "member" ? "跟随 Agent 默认" : "本机配置"}</option><option value="local">Claude Code 本机配置</option>{draft.connectionId && draft.connectionId !== "local" && !providers.some(provider => provider.id === draft.connectionId) && <option value={draft.connectionId}>连接已缺失：{draft.connectionId}</option>}{providers.filter(provider => !provider.hidden || provider.id === draft.connectionId).map(provider => <option key={provider.id} value={provider.id}>{provider.name}</option>)}</select> : connectionLabel(row.effective.connectionId, row.runtime, providers)}</td>
      <td>{editing && row.id === selectedId ? <input aria-label="模型 ID" value={draft.model ?? ""} list={modelListId} placeholder={row.scope === "runtime" ? "本机默认模型" : "跟随 Agent 默认"} disabled={busy} onChange={event => setDraft(current => ({ ...current, model: event.target.value || null }))} /> : row.effective.model ?? (row.effective.source === "unrecorded" ? "未记录" : "本机默认")}</td>
      <td>{assignmentSource(row)}</td><td><button type="button" className="model-assignment-edit" disabled={blocked} onClick={() => select(row, row.editable)} aria-label={`${row.editable ? "编辑" : "查看"} ${row.name}`}>{row.editable ? "编辑" : "查看"}</button></td>
    </tr>)}</Fragment>)}</tbody></table></div>
    <datalist id={modelListId}>{suggestions.map(model => <option value={model} key={model} />)}</datalist>
    {loading && <p role="status" className="model-assignment-empty">正在读取模型分配…</p>}
    {!loading && data && !rows.length && <p className="model-assignment-empty">没有符合条件的配置。</p>}
    {selected && <div className="model-assignment-selection"><strong>{editing ? "正在编辑" : "已选"} {selected.name} · {editing ? draft.model || "跟随默认" : selected.effective.model || "本机默认"}</strong><p>{selected.note || (selected.scope === "member" ? "成员配置用于后续启动；运行中的会话不会被自动切换。" : "默认用于 CLI 新会话与项目成员；显式选择的连接和会话覆盖优先。")}</p></div>}
    {notice && <p className="model-assignment-info" role="status">{notice}</p>}
    <footer className="model-assignment-footer"><span>{editing ? "保存或取消当前编辑后，可继续筛选。" : "会话列表显示启动配置；运行中模型以原会话为准。"}</span><div>{editing && <button type="button" disabled={busy} onClick={() => { setEditing(false); setError(null); }}>取消</button>}<button type="button" disabled={!selected?.editable || busy || PREVIEW || !editing} onClick={() => void save(true)}>恢复继承</button><button type="button" className="primary" disabled={!selected?.editable || busy || PREVIEW || !editing} onClick={() => void save(false)}>{busy ? "正在保存…" : "保存更改"}</button></div></footer>
  </section>;
}
