import { LocalizedErrorText } from "./LocalizedErrorText";
import { useTranslation } from "../useInterfaceLanguage";
import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { MagnifyingGlassIcon } from "@phosphor-icons/react/dist/csr/MagnifyingGlass";
import { StarIcon } from "@phosphor-icons/react/dist/csr/Star";
import { PlusIcon } from "@phosphor-icons/react/dist/csr/Plus";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import { TerminalIcon } from "@phosphor-icons/react/dist/csr/Terminal";
import { ChatCircleIcon } from "@phosphor-icons/react/dist/csr/ChatCircle";
import { GearSixIcon } from "@phosphor-icons/react/dist/csr/GearSix";
import { FolderIcon } from "@phosphor-icons/react/dist/csr/Folder";
import { Modal } from "./ui/Modal";
import { useRuntimeAvailability } from "../runtimeAvailability";
import type { AgentRuntimeKind, ProviderConfig, SessionRecord } from "../types";
import { createQuickCapabilityCache } from "../quickCommandCapabilities";
import {
  ACTION_LABELS, SCOPE_LABELS, RUNTIME_LABELS, QUICK_COMMAND_CHANGED, QUICK_COMMAND_STORAGE_KEY,
  emptyQuickSettings, readQuickSettings, writeQuickSettings, quickCommandCatalog, scopeSessions,
  sessionRuntime, targetIdentity, type QuickAction, type QuickCommand, type QuickCommandSettings, type QuickContext, type QuickScope,
} from "../quickCommands";
import { applyDeliveryEvidence, executeQuickPlan, type QuickExecutionPlan, type QuickReceipt } from "../quickCommandExecution";
import { resolveSessionDirectory } from "../sessionDirectory";

export interface QuickCommandCenterProps {
  sessions: SessionRecord[];
  providers: ProviderConfig[];
  context?: QuickContext;
  manage?: boolean;
  initialCommand?: Partial<QuickCommand>;
  onClose?: () => void;
  onSessionsChange?: (sessions: SessionRecord[]) => void;
  onOpenSettings?: () => void;
}
const receiptLabels: Record<QuickReceipt["state"], string> = {
  queued: "已排队", text_written: "内容已写入", started: "已启动", stopped: "已停止", removed: "已从列表删除", failed: "失败",
};
const displayPath = (p?: string | null) =>
  !p ? "" : p.startsWith("\\\\?\\UNC\\") ? `\\\\${p.slice(8)}` : p.startsWith("\\\\?\\") && /^[A-Za-z]:[\\/]/.test(p.slice(4)) ? p.slice(4) : p;
const capabilityCache = createQuickCapabilityCache(invoke);
export function QuickCommandCenter({ sessions, providers, context = {}, manage = false, initialCommand, onClose, onSessionsChange, onOpenSettings }: QuickCommandCenterProps) {
  const t = useTranslation();
  const id = useId();
  const searchRef = useRef<HTMLInputElement>(null);
  const inFlight = useRef(false);
  const { ready, isRuntimeEnabled, error: availabilityError } = useRuntimeAvailability();
  const [initialSettings] = useState(() => {
    try { return { value: readQuickSettings(localStorage), error: null }; }
    catch (e) { return { value: emptyQuickSettings(), error: `读取快捷指令失败：${String(e)}。原数据保留，修复前不会覆盖。` }; }
  });
  const [settings, setSettings] = useState<QuickCommandSettings>(initialSettings.value);
  const [storageError, setStorageError] = useState<string | null>(initialSettings.error);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState("");
  const [query, setQuery] = useState("");
  const [tab, setTab] = useState<"all" | "favorites" | "recent">("all");
  const providerIds = JSON.stringify(providers.filter(p => !p.hidden).map(p => p.id).sort());
  const [capabilities, setCapabilities] = useState(() => capabilityCache.get(JSON.parse(providerIds)));
  const [loading, setLoading] = useState(() => !capabilityCache.fresh(JSON.parse(providerIds)));
  const [capabilityError, setCapabilityError] = useState<string | null>(null);
  const [sessionLoading, setSessionLoading] = useState(true);
  const [sessionError, setSessionError] = useState<string | null>(null);
  const probes = capabilities?.probes ?? [];
  const keys = capabilities?.keys ?? {};
  const [refresh, setRefresh] = useState(0);
  const [currentSessions, setCurrentSessions] = useState(sessions);
  const [draft, setDraft] = useState<QuickCommand>({ id: "builtin:send", title: t("向会话发送消息"), action: "send", scope: initialSettings.value.defaultScope, body: t("检查当前项目并给出下一步建议"), ...initialCommand });
  const [editing, setEditing] = useState(false);
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [cwd, setCwd] = useState(() => displayPath(context.cwd));
  const [count, setCount] = useState(1);
  const [busy, setBusy] = useState(false);
  const [confirmation, setConfirmation] = useState<QuickExecutionPlan | null>(null);
  const [removeCommand, setRemoveCommand] = useState<string | null>(null);
  const [receipts, setReceipts] = useState<QuickReceipt[]>([]);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  useEffect(() => setCurrentSessions(sessions), [sessions]);
  useEffect(() => {
    const load = () => {
      try { const value = readQuickSettings(localStorage); setSettings(value); setStorageError(null); }
      catch (e) { setStorageError(`读取快捷指令失败：${String(e)}。原数据保留，修复前不会覆盖。`); }
    };
    load();
    const sync = (e: StorageEvent) => { if (e.key === QUICK_COMMAND_STORAGE_KEY || e.key === null) load(); };
    window.addEventListener(QUICK_COMMAND_CHANGED, load);
    window.addEventListener("storage", sync);
    return () => { window.removeEventListener(QUICK_COMMAND_CHANGED, load); window.removeEventListener("storage", sync); };
  }, []);
  useEffect(() => {
    let alive = true;
    const ids = JSON.parse(providerIds) as string[];
    const force = refresh > 0;
    const cached = capabilityCache.get(ids);
    setCapabilities(cached);
    setLoading(force || !capabilityCache.fresh(ids));
    // Paint the page before starting CLI detection. It must not gate session reads.
    let backgroundFrame = 0;
    const firstFrame = requestAnimationFrame(() => {
      backgroundFrame = requestAnimationFrame(() => {
        void capabilityCache.load(ids, force)
          .then(data => { if (alive) { setCapabilities(data); setCapabilityError(null); } })
          .catch(e => { if (alive) setCapabilityError(`读取本机能力失败：${String(e)}；请重新检测。`); })
          .finally(() => { if (alive) setLoading(false); });
      });
    });
    return () => { alive = false; cancelAnimationFrame(firstFrame); cancelAnimationFrame(backgroundFrame); };
  }, [refresh, providerIds]);
  useEffect(() => {
    let alive = true;
    setSessionLoading(true);
    void invoke<SessionRecord[]>("session_snapshot")
      .then(fresh => { if (alive) { setCurrentSessions(fresh); setSessionError(null); } })
      .catch(e => { if (alive) setSessionError(`读取会话失败：${String(e)}；请重新检测。`); })
      .finally(() => { if (alive) setSessionLoading(false); });
    return () => { alive = false; };
  }, [refresh]);
  useEffect(() => {
    if (!receipts.some(r => r.requestId && r.state === "queued")) return;
    let alive = true;
    const update = async () => {
      try {
        const evidence = await invoke<Parameters<typeof applyDeliveryEvidence>[1]>("list_communication_evidence");
        if (alive) setReceipts(rows => applyDeliveryEvidence(rows, evidence));
      } catch (e) { if (alive) setNotice(`投递状态查询失败：${String(e)}；保留最后一次状态。`); }
    };
    void update();
    const timer = window.setInterval(update, 2500);
    return () => { alive = false; clearInterval(timer); };
  }, [receipts.some(r => r.requestId && r.state === "queued")]);

  const mutateSettings = (change: (current: QuickCommandSettings) => QuickCommandSettings) => {
    try {
      const latest = readQuickSettings(localStorage);
      const next = change(latest);
      writeQuickSettings(localStorage, next);
      setSettings(next);
      setStorageError(null);
      window.dispatchEvent(new Event(QUICK_COMMAND_CHANGED));
      return true;
    } catch (e) { setStorageError(`保存快捷指令失败：${String(e)}；未显示为保存成功。`); return false; }
  };
  const catalog = quickCommandCatalog(probes, providers, keys, isRuntimeEnabled, t);
  const commands = [...catalog, ...settings.commands];
  const filtered = commands.filter(c => (!query.trim() || `${c.title} ${c.body} ${t(ACTION_LABELS[c.action])}`.toLowerCase().includes(query.trim().toLowerCase())) &&
    (tab === "all" || (tab === "favorites" ? settings.favorites : settings.recent).includes(c.id)));
  if (tab === "recent") filtered.sort((a, b) => settings.recent.indexOf(a.id) - settings.recent.indexOf(b.id));
  const eligible = scopeSessions(currentSessions, draft.scope, context, draft.action).filter(s => isRuntimeEnabled(sessionRuntime(s)!) && (!draft.runtimeKind || sessionRuntime(s) === draft.runtimeKind));
  const targets = eligible.filter(s => selectedIds.includes(s.id));
  const runtimeReady = !!draft.runtimeKind && catalog.some(c => c.action === "launch" && c.runtimeKind === draft.runtimeKind && c.providerId === draft.providerId);
  const catalogPending = (!capabilities && loading) || (!ready && !availabilityError);
  const canExecute = ready && !busy && !editing && (draft.action === "launch" ? !loading && !capabilityError && runtimeReady && !!cwd.trim() && count >= 1 && count <= 12 : !sessionLoading && !sessionError && targets.length > 0 && (draft.action !== "send" || !!draft.body.trim()));
  const choose = (c: QuickCommand) => {
    setDraft({ ...c, scope: c.id.startsWith("builtin:") ? settings.defaultScope : c.scope });
    setCwd(displayPath(c.cwd ?? context.cwd)); setCount(c.count ?? 1);
    setSelectedIds([]); setEditing(false); setConfirmation(null); setRemoveCommand(null); setNotice("");
  };
  const updateDraft = (change: Partial<QuickCommand>) => { setDraft(c => ({ ...c, ...change })); setConfirmation(null); };
  const saveCommand = () => {
    const command = { ...draft, id: draft.id.startsWith("custom:") ? draft.id : `custom:${crypto.randomUUID()}`, ...(draft.action === "launch" ? { cwd: cwd.trim(), count } : {}) };
    if (command.action === "launch" && !command.runtimeKind) { setError("请为启动指令选择 Agent"); return; }
    if (mutateSettings(s => ({ ...s, commands: [...s.commands.filter(c => c.id !== command.id), command] }))) {
      setDraft(command); setEditing(false); setNotice("指令已保存在本机。目标会话将在每次执行前重新选择。");
    }
  };
  const run = async (plan: QuickExecutionPlan) => {
    if (inFlight.current) return;
    inFlight.current = true; setBusy(true); setError(null); setNotice(""); setConfirmation(null); setReceipts([]);
    try {
      await executeQuickPlan(plan, { invoke, enabled: isRuntimeEnabled, uuid: () => crypto.randomUUID(), progress: row => { if (mounted.current) setReceipts(rows => [...rows, row]); } });
      mutateSettings(s => ({ ...s, recent: [draft.id, ...s.recent.filter(i => i !== draft.id)].slice(0, 20) }));
      const fresh = await invoke<SessionRecord[]>("session_snapshot");
      if (mounted.current) { setCurrentSessions(fresh); setSelectedIds([]); }
      onSessionsChange?.(fresh);
    } catch (e) { if (mounted.current) setError(String(e)); }
    finally { inFlight.current = false; if (mounted.current) setBusy(false); }
  };
  const prepare = () => {
    const directory = draft.action === "launch" ? resolveSessionDirectory(cwd.trim(), context.projects ?? [], context.projectId) : undefined;
    const plan: QuickExecutionPlan = { command: { ...draft }, targets: targets.map(s => ({ ...s })), cwd: directory?.path ?? cwd.trim(), count, projectId: directory?.id };
    if (draft.action === "stop" || draft.action === "delete") setConfirmation(plan);
    else void run(plan);
  };
  const refreshButton = <button type="button" aria-busy={loading} title={loading ? t("正在检测本机能力") : undefined} disabled={busy || loading} onClick={() => setRefresh(n => n + 1)}>{t("重新检测")}</button>;
  const configButton = onOpenSettings ? <button type="button" disabled={busy} onClick={onOpenSettings}><GearSixIcon size={18} />{t("配置")}</button> : null;
  const searchField = <label className="quick-center-search"><MagnifyingGlassIcon size={19} /><input ref={searchRef} aria-label={t("搜索快捷指令")} placeholder={t("搜索指令、Agent 或已保存的操作")} value={query} onChange={e => setQuery(e.target.value)} /></label>;
  const content = <>
    {/* 设置页内嵌时，页面标题已说明用途：不再重复一段说明，并把「重新检测」并入搜索行节省纵向空间。 */}
    {!manage && <header className="quick-center-header">
      <h2 id={`${id}-title`}>{t("快捷指令")}</h2>
      <div className="quick-center-header-actions">{refreshButton}
        {configButton}
        <button type="button" aria-label={t("关闭快捷指令")} disabled={busy} onClick={onClose}><XIcon size={19} /></button>
      </div>
    </header>}
    {manage
      ? <div className="quick-center-toolbar">{searchField}{refreshButton}{configButton}</div>
      : searchField}
    {storageError && <p role="alert" className="quick-center-error"><LocalizedErrorText source={storageError} /></p>}
    {(error || availabilityError) && <p role="alert" className="quick-center-error"><LocalizedErrorText source={error || availabilityError || ""} /></p>}
    {capabilityError && <p role="alert" className="quick-center-error"><LocalizedErrorText source={capabilityError} /></p>}
    {sessionError && <p role="alert" className="quick-center-error"><LocalizedErrorText source={sessionError} /></p>}
    {notice && <p role="status" className="quick-center-notice">{t(notice)}</p>}
    <div className="quick-center-body">
      <aside className="quick-center-library" aria-label={t("快捷指令库")}>
      <div className="quick-center-tabs" role="tablist" aria-label={t("快捷指令分类")}>{([["favorites", t("收藏")], ["recent", t("最近")], ["all", t("全部")]] as const).map(([value, label]) => <button key={value} type="button" role="tab" aria-selected={tab === value} onClick={() => setTab(value)}>{label}</button>)}</div>
      <nav className="quick-center-list" aria-label={t("指令列表")} aria-busy={loading}>
        {!catalogPending && !filtered.length && <p className="quick-center-empty">{query ? t("没有匹配的指令") : tab === "favorites" ? t("暂无收藏，可在全部指令中点击星标。") : t("暂无记录")}</p>}
        {filtered.map(c => <div className={`quick-center-list-row${draft.id === c.id ? " selected" : ""}`} key={c.id}>
          <button type="button" disabled={busy} aria-pressed={draft.id === c.id} onClick={() => choose(c)}>{c.action === "launch" ? <TerminalIcon size={19} aria-hidden="true" /> : <ChatCircleIcon size={19} />}<span>{c.title}<small>{c.id.startsWith("custom:") ? t("自定义") : c.action === "launch" ? c.providerId ? t("已配置 Provider") : t("本地配置") : t(ACTION_LABELS[c.action])}</small></span></button>
          <button type="button" className="quick-center-favorite" aria-label={`${settings.favorites.includes(c.id) ? t("取消收藏") : t("收藏")} ${c.title}`} aria-pressed={settings.favorites.includes(c.id)} disabled={busy || !!storageError} onClick={() => mutateSettings(s => ({ ...s, favorites: s.favorites.includes(c.id) ? s.favorites.filter(i => i !== c.id) : [...s.favorites, c.id] }))}><StarIcon size={17} weight={settings.favorites.includes(c.id) ? "fill" : "regular"} /></button>
        </div>)}
        {catalogPending && <p className="quick-center-loading" role="status" aria-label={t("正在检测本机 Agent 与已配置服务")}>{t("正在检测本机 Agent 与已配置服务…")}</p>}
      </nav>
      <div className="quick-center-library-footer">
        <button type="button" className="quick-center-create" disabled={busy || !!storageError} onClick={() => { setDraft({ id: `custom:${crypto.randomUUID()}`, title: "", action: "send", scope: settings.defaultScope, body: "" }); setSelectedIds([]); setEditing(true); setConfirmation(null); }}><PlusIcon size={18} />{t("新建自定义指令")}</button>
        {manage && <label className="quick-center-default" title={t("仅影响默认值，不会自动发送。")}>{t("默认范围")}<select aria-label={t("默认范围")} value={settings.defaultScope} disabled={busy || !!storageError} onChange={e => mutateSettings(s => ({ ...s, defaultScope: e.target.value as QuickScope }))}>{Object.entries(SCOPE_LABELS).map(([v, label]) => <option key={v} value={v}>{t(label)}</option>)}</select></label>}
      </div>
      </aside>
      <div className="quick-center-workspace">
      <section className="quick-center-detail" aria-label={t("快捷指令配置")}>
        {editing ? <>
          <label>{t("指令名称")}<input autoComplete="off" maxLength={100} value={draft.title} placeholder={t("例如：向工作区收集进度")} onChange={e => updateDraft({ title: e.target.value })} /></label>
          <label>{t("明确动作")}<select value={draft.action} onChange={e => { updateDraft({ action: e.target.value as QuickAction, runtimeKind: undefined, providerId: undefined }); setSelectedIds([]); }}>{Object.entries(ACTION_LABELS).map(([v, label]) => <option key={v} value={v}>{t(label)}</option>)}</select></label>
        </> : <div className="quick-center-detail-heading"><h3>{draft.title}</h3><button type="button" disabled={busy || !!storageError} onClick={() => { setEditing(true); setConfirmation(null); }}>{draft.id.startsWith("custom:") ? t("编辑") : t("另存为自定义")}</button></div>}
        {(draft.action === "launch" || editing) && <>
          <label>{draft.action === "launch" ? t("执行 Agent") : t("限定 Agent（可选）")}<select value={draft.runtimeKind ?? ""} disabled={busy} onChange={e => { updateDraft({ runtimeKind: (e.target.value || undefined) as AgentRuntimeKind | undefined, providerId: undefined }); setSelectedIds([]); }}>
            <option value="">{draft.action === "launch" ? t("请选择本机可用 Agent") : t("全部支持的 Agent")}</option>
            {Object.entries(RUNTIME_LABELS).filter(([kind]) => catalog.some(c => c.action === "launch" && c.runtimeKind === kind)).map(([kind, label]) => <option key={kind} value={kind}>{label}</option>)}</select></label>
          {draft.action === "launch" && draft.runtimeKind === "claude_code" && <label>{t("配置来源")}<select value={draft.providerId ?? ""} disabled={busy} onChange={e => updateDraft({ providerId: e.target.value || undefined })}><option value="">{t("使用本地 Claude Code 配置")}</option>{providers.filter(p => !p.hidden && keys[p.id]).map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</select></label>}
        </>}
        {draft.action === "launch" ? <>
          <label>{t("工作目录")}<input value={cwd} disabled={busy} onChange={e => { setCwd(e.target.value); setConfirmation(null); }} placeholder={t("填写项目目录的完整路径")} /></label>
          <label>{t("启动数量")}<input type="number" min={1} max={12} value={count} disabled={busy} onChange={e => setCount(Number(e.target.value))} /></label>
          {(loading || !runtimeReady) && <p className="quick-center-notice">{loading ? t("正在核对本机能力，完成后可启动。") : t("该指令的 Agent 或 Provider 当前不可用，请检查安装、启用状态与配置。")}</p>}
        </> : <>
          <div className="quick-center-scope-row"><label>{t("范围")}<select aria-label={t("范围")} value={draft.scope} disabled={busy} onChange={e => { updateDraft({ scope: e.target.value as QuickScope }); setSelectedIds([]); }}>
            {Object.entries(SCOPE_LABELS).map(([v, label]) => <option key={v} value={v}>{v === "all" && draft.action === "delete" ? t("全部未归档 Agent 会话") : t(label)}</option>)}</select></label>
          <p className="quick-center-directory" title={displayPath(context.cwd) || t("未关联工作目录")}><FolderIcon size={19} aria-hidden="true" /><span>{displayPath(context.cwd) || t("未关联工作目录")}</span></p></div>
          {draft.scope === "workspace" && context.workspaceSessionIds === undefined && <p className="quick-center-notice">{t("此入口没有工作区上下文，请回主窗口或选择其他范围。")}</p>}
          {draft.scope === "project" && !context.projectId && <p className="quick-center-notice">{t("当前未选择项目，请回主窗口选择项目或使用其他范围。")}</p>}
          <div className="quick-center-target-heading"><strong>{t("目标会话 · 已选")} {targets.length} / {eligible.length}</strong><button type="button" disabled={busy || !eligible.length} onClick={() => { setConfirmation(null); setSelectedIds(targets.length === eligible.length ? [] : eligible.map(s => s.id)); }}>{targets.length === eligible.length && targets.length ? t("取消全选") : t("选择此范围全部")}</button></div>
          <div className="quick-center-targets">{eligible.map(s => <label key={s.id} title={`${s.title} · ${s.providerName} · ${displayPath(s.cwd)}`}><input type="checkbox" disabled={busy} checked={selectedIds.includes(s.id)} onChange={e => { setConfirmation(null); setSelectedIds(ids => e.target.checked ? [...ids, s.id] : ids.filter(i => i !== s.id)); }} /><span>{s.title}<small>{s.providerName}{draft.scope !== "project" && ` · ${displayPath(s.cwd)}`}</small></span></label>)}{!eligible.length && <p>{t("此范围暂无可操作的 Agent 会话。")}</p>}</div>
        </>}
        {(draft.action === "send" || draft.action === "launch") && <label className="quick-center-message">{draft.action === "launch" ? t("启动任务（可选）") : t("消息内容")}<textarea rows={4} disabled={busy} value={draft.body} onChange={e => updateDraft({ body: e.target.value })} placeholder={t("填写要执行的具体任务")} /></label>}
        <p className="quick-center-help">{t("已排队不代表已送达、已读或任务完成。")}</p>
        {editing && <div className="quick-center-editor-actions"><button type="button" className="primary" disabled={busy || !!storageError || !draft.title.trim()} onClick={saveCommand}>{t("保存指令")}</button><button type="button" onClick={() => { setEditing(false); choose(commands.find(c => c.id === draft.id) ?? catalog[0]); }}>{t("取消编辑")}</button></div>}
        {!editing && draft.id.startsWith("custom:") && <button type="button" className="quick-center-remove" disabled={busy || !!storageError} onClick={() => setRemoveCommand(draft.id)}>{t("删除这条自定义指令")}</button>}
        {removeCommand === draft.id && <div className="quick-center-confirm" role="group" aria-label={t("确认删除指令")}><p>{t("只删除这条本地指令，不删除会话。")}</p><button type="button" onClick={() => { if (mutateSettings(s => ({ ...s, commands: s.commands.filter(c => c.id !== draft.id), favorites: s.favorites.filter(i => i !== draft.id), recent: s.recent.filter(i => i !== draft.id) }))) { choose(catalog[0]); } }}>{t("确认删除指令")}</button><button type="button" onClick={() => setRemoveCommand(null)}>{t("保留")}</button></div>}
      </section>
    {confirmation && <div className="quick-center-confirm" role="group" aria-label={t("批量操作二次确认")}><strong>{t("确认")}{t(ACTION_LABELS[confirmation.command.action])} · {confirmation.targets.length} {t("个目标？")}</strong><ul>{confirmation.targets.map(s => <li key={targetIdentity(s)}>{s.title} · {displayPath(s.cwd)}</li>)}</ul><p>{draft.action === "delete" ? t("会话将从列表移除；不是归档。不会删除项目目录。") : t("将停止所选会话的运行进程，正在执行的工作会被中断。")}</p><button type="button" className="danger" onClick={() => void run(confirmation)}>{t("确认")}{draft.action === "delete" ? t("删除所选会话") : t("停止所选会话")}</button><button type="button" onClick={() => setConfirmation(null)}>{t("取消操作")}</button></div>}
    {receipts.length > 0 && <section className="quick-center-receipts" aria-label={t("执行结果")} aria-live="polite"><h3>{t("执行结果")}</h3>{receipts.map(r => <div key={`${r.targetId}-${r.requestId ?? r.state}`} data-state={r.state}><span>{r.title}</span><strong>{t(receiptLabels[r.state])}</strong>{r.detail && <small>{r.detail}</small>}</div>)}</section>}
    <footer className="quick-center-footer"><span>{draft.action === "launch" ? t("将在 {path} 启动 {count} 个会话", { path: displayPath(cwd) || t("未选择目录"), count }) : t("已选择 {count} 个目标 · {scope}", { count: targets.length, scope: t(SCOPE_LABELS[draft.scope]) })}</span><button type="button" className={draft.action === "stop" || draft.action === "delete" ? "danger" : "primary"} disabled={!canExecute || !!confirmation} onClick={prepare}>{busy ? t("执行中…") : draft.action === "launch" ? t("确认启动") : draft.action === "send" ? t("确认发送") : t("核对批量操作")}</button></footer>
      </div>
    </div>
  </>;
  return manage ? <div className="quick-center quick-center-embedded">{content}</div> : createPortal(<Modal className="quick-center quick-center-modal" overlayClassName="dialog-overlay quick-center-overlay" labelledBy={`${id}-title`} initialFocusRef={searchRef} onClose={() => { if (!inFlight.current) onClose?.(); }}>{content}</Modal>, document.body);
}
