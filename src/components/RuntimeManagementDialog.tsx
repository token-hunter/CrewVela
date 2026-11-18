import { useEffect, useRef, useState, type DragEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowDown, ArrowUp, CaretDown, CaretRight, DotsSixVertical, X } from "@phosphor-icons/react";
import { useRuntimeAvailability } from "../runtimeAvailability";
import { RUNTIME_LABELS, RUNTIME_LOGOS } from "../runtimeLogos";
import { COLLABORATION_RUNTIMES, isLegacyCollaborationRuntime, runtimeCategory, isRetiredRuntime, type RuntimeCategory } from "../runtimeTaxonomy";
import type { AgentRuntimeKind } from "../types";
import type { RuntimeDropPosition } from "../runtimePreferences";
import { Modal } from "./ui/Modal";
import "../styles/runtime-management.css";
import "../styles/cli-launch.css";
import { RuntimeInstallWizard } from "./RuntimeInstallWizard";
import { supportsChatInstallation, supportsRuntimeInstallation, type RuntimeChatDependencies } from "../runtimeInstallation";
import { isCliRuntime, launchChoices } from "../runtimeLaunch";

export function RuntimeManagementDialog({ order, onReorder, orderError, onClose, initialInstaller }: {
  order: AgentRuntimeKind[];
  onReorder: (kind: AgentRuntimeKind, target: AgentRuntimeKind, position: RuntimeDropPosition) => void;
  orderError?: string | null;
  onClose: () => void;
  initialInstaller?: AgentRuntimeKind;
}) {
  const availability = useRuntimeAvailability();
  const [pending, setPending] = useState<AgentRuntimeKind | null>(null);
  const [installing, setInstalling] = useState<AgentRuntimeKind | null>(initialInstaller ?? null);
  const [cliCollapsed, setCliCollapsed] = useState(initialInstaller !== undefined);
  const [chatDependencies, setChatDependencies] = useState<RuntimeChatDependencies | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dragging, setDragging] = useState<AgentRuntimeKind | null>(null);
  const [dropTarget, setDropTarget] = useState<{ kind: AgentRuntimeKind; position: RuntimeDropPosition } | null>(null);
  const dragSource = useRef<AgentRuntimeKind | null>(null);
  const suppressClickUntil = useRef(0);
  const hasChats = order.some(supportsChatInstallation);
  useEffect(() => {
    if (installing || !hasChats) return;
    let live = true;
    const timer = window.setTimeout(() => {
      void invoke<RuntimeChatDependencies>("runtime_chat_dependencies")
        .then(result => { if (live) setChatDependencies(result ?? {}); })
        .catch(() => { if (live) setChatDependencies({}); });
    }, 100);
    return () => { live = false; window.clearTimeout(timer); };
  }, [installing, hasChats]);
  const clearDrag = () => {
    dragSource.current = null;
    setDragging(null);
    setDropTarget(null);
    suppressClickUntil.current = Date.now() + 250;
  };
  const canDrop = (target: AgentRuntimeKind) => {
    const source = dragSource.current;
    return source !== null && source !== target && managed.includes(source)
      && runtimeCategory(source) === runtimeCategory(target);
  };
  const showDropTarget = (kind: AgentRuntimeKind, event: DragEvent<HTMLDivElement>) => {
    if (!dragSource.current) return;
    event.preventDefault();
    if (!canDrop(kind)) { event.dataTransfer.dropEffect = "none"; setDropTarget(null); return; }
    event.dataTransfer.dropEffect = "move";
    const bounds = event.currentTarget.getBoundingClientRect();
    const position = event.clientY < bounds.top + bounds.height / 2 ? "before" : "after";
    setDropTarget((current) => current?.kind === kind && current.position === position ? current : { kind, position });
  };
  const toggle = async (kind: AgentRuntimeKind, enabled: boolean) => {
    if (pending) return;
    setPending(kind);
    setError(null);
    try { await availability.setRuntimeEnabled(kind, enabled); }
    catch (reason) { setError(String(reason)); }
    finally { setPending(null); }
  };
  const saveMode = async (kind: AgentRuntimeKind, mode: string) => {
    if (pending) return;
    setPending(kind); setError(null);
    try { await availability.setRuntimeLaunchDefault(kind, mode); }
    catch (reason) { setError(`保存默认启动模式失败：${String(reason)}`); }
    finally { setPending(null); }
  };
  const managed = order.filter((kind) => !isRetiredRuntime(kind));
  const groups: Array<[RuntimeCategory, string]> = [
    ["terminal", "CLI 终端 Agent"],
    ["chat", "原生聊天 Agent"],
  ];
  const renderItem = (kind: AgentRuntimeKind, index: number, group: AgentRuntimeKind[]) => <div
    className={`collab-runtime-management-row${isCliRuntime(kind) ? " cli-mode-row" : " chat-install-row"}${dragging === kind ? " dragging" : ""}`} key={kind}
    data-runtime-order={kind} data-drop-position={dropTarget?.kind === kind ? dropTarget.position : undefined}
    draggable
    onDragStart={(event) => {
      if ((event.target as Element).closest("button, input, select")) { event.preventDefault(); return; }
      dragSource.current = kind;
      setDragging(kind);
      setDropTarget(null);
      event.dataTransfer.effectAllowed = "move";
      event.dataTransfer.setData("text/plain", kind);
    }}
    onDragEnter={(event) => showDropTarget(kind, event)}
    onDragOver={(event) => showDropTarget(kind, event)}
    onDragLeave={(event) => {
      if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDropTarget((current) => current?.kind === kind ? null : current);
    }}
    onDrop={(event) => {
      event.preventDefault();
      event.stopPropagation();
      const source = dragSource.current;
      if (source && canDrop(kind)) {
        const bounds = event.currentTarget.getBoundingClientRect();
        onReorder(source, kind, event.clientY < bounds.top + bounds.height / 2 ? "before" : "after");
      }
      clearDrag();
    }}
    onDragEnd={clearDrag}
    onClickCapture={(event) => {
      if (dragSource.current || Date.now() < suppressClickUntil.current) { event.preventDefault(); event.stopPropagation(); }
    }}
  >
    <span className="collab-runtime-drag-handle" aria-hidden="true" title="按住拖动，调整上下顺序"><DotsSixVertical size={17} /></span>
    <label className="collab-runtime-toggle">
      <img src={RUNTIME_LOGOS[kind]} data-runtime={kind} alt="" draggable={false} />
      <span>{RUNTIME_LABELS[kind]}<small className="collab-runtime-scope">{COLLABORATION_RUNTIMES.includes(kind) ? "CLI 协作执行" : isLegacyCollaborationRuntime(kind) ? "原生聊天 · 已有协作成员兼容" : "仅原生聊天"}</small>
        {supportsChatInstallation(kind) && <small className="runtime-chat-dependency" data-missing={chatDependencies?.[kind]?.needsSetup || undefined}>{chatDependencies?.[kind]?.label ?? (chatDependencies ? "打开详情检查启动依赖" : "正在检查启动依赖…")}</small>}
      </span>
      <small>{pending === kind ? "保存中…" : availability.ready ? availability.isRuntimeEnabled(kind) ? "已启用" : "已禁用" : "加载中…"}</small>
      <input type="checkbox" aria-label={`启用 ${RUNTIME_LABELS[kind]}`} checked={availability.ready && availability.isRuntimeEnabled(kind)} disabled={!availability.ready || pending !== null} onChange={(event) => void toggle(kind, event.target.checked)} />
    </label>
    {isCliRuntime(kind) && <select className="runtime-default-mode" aria-label={`默认启动模式 · ${RUNTIME_LABELS[kind]}`} value={availability.prefs.defaultLaunchModes?.[kind] ?? "cli_default"} disabled={!availability.ready || pending !== null} onChange={event => void saveMode(kind, event.target.value)}>
      {!launchChoices(kind).some(choice => choice.id === (availability.prefs.defaultLaunchModes?.[kind] ?? "cli_default")) && <option disabled value={availability.prefs.defaultLaunchModes?.[kind]}>已保存模式不可用</option>}
      {launchChoices(kind).map(choice => <option key={choice.id} value={choice.id}>{choice.label}</option>)}
    </select>}
    {supportsRuntimeInstallation(kind) && <button type="button" className="btn secondary collab-runtime-install-entry" onClick={() => setInstalling(kind)}>安装与版本</button>}
    <div className="collab-runtime-order-actions" role="group" aria-label={`调整 ${RUNTIME_LABELS[kind]} 顺序`}>
      <button type="button" aria-label={`上移 ${RUNTIME_LABELS[kind]}`} title="上移" disabled={index === 0} onClick={() => onReorder(kind, group[index - 1], "before")}><ArrowUp size={14} aria-hidden="true" /></button>
      <button type="button" aria-label={`下移 ${RUNTIME_LABELS[kind]}`} title="下移" disabled={index === group.length - 1} onClick={() => onReorder(kind, group[index + 1], "after")}><ArrowDown size={14} aria-hidden="true" /></button>
    </div>
  </div>;
  return <Modal className={`collab-runtime-management${installing ? " runtime-installation-modal" : ""}`} ariaLabel="管理 Runtime" onClose={onClose}>
    <header><h3>管理 Runtime</h3><button type="button" className="btn secondary" aria-label="关闭" onClick={onClose}>{installing ? <X size={21} /> : "关闭"}</button></header>
    {installing ? <RuntimeInstallWizard key={installing} kind={installing} onBack={() => setInstalling(null)} onComplete={installing === initialInstaller ? onClose : undefined} /> : <>
    {(error || availability.error) && <p role="alert" className="collaboration-settings-error">{error || availability.error}</p>}
    {orderError && <p role="alert" className="collaboration-settings-error">{orderError}</p>}
    {!availability.ready && <button type="button" className="btn secondary" onClick={() => void availability.refresh()}>重新加载配置</button>}
    <div className="collab-runtime-management-list" onDragOver={(event) => {
      if (!dragSource.current) return;
      event.preventDefault();
    }} onDrop={(event) => { event.preventDefault(); clearDrag(); }}>{groups.map(([category, title]) => {
      const items = managed.filter(kind => runtimeCategory(kind) === category);
      if (!items.length) return null;
      const terminal = category === "terminal";
      const collapsed = terminal && cliCollapsed;
      return <section key={category} className="collab-runtime-management-group-section">
        <h4 className="collab-runtime-management-group">{terminal ? <>
          <button type="button" className="runtime-group-toggle" aria-expanded={!collapsed} onClick={() => setCliCollapsed(!collapsed)}>
            {collapsed ? <CaretRight size={14} /> : <CaretDown size={14} />}{title}（{items.length}）
          </button>
          {!collapsed && <span className="runtime-default-heading">默认启动模式</span>}
        </> : title}</h4>
        {!collapsed && items.map(renderItem)}
      </section>;
    })}
    </div>
    <p className="runtime-launch-footnote">默认模式用于之后新建的 CLI 会话；已打开的会话保持原设置。</p>
    </>}
  </Modal>;
}
