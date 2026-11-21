import { useEffect, useRef, useState } from "react";
import { open as openDirectory } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { X } from "@phosphor-icons/react";
import { RUNTIME_LABELS, RUNTIME_LOGOS } from "../runtimeLogos";
import { isCliRuntime, launchChoices, launchModeLabel, type RuntimeLaunchSelection } from "../runtimeLaunch";
import { CHAT_AGENTS, TERMINAL_AGENTS, type NewSessionAgent } from "../newSessionAgents";
import { resolveSessionDirectory } from "../sessionDirectory";
import type { Project, SessionDirectory } from "../types";
import { useRuntimeAvailability } from "../runtimeAvailability";
import { NEW_SESSION_CHAT_ORDER_STORAGE_KEY, NEW_SESSION_TERMINAL_ORDER_STORAGE_KEY, useRuntimeOrder } from "../runtimePreferences";
import { RuntimeManagementDialog } from "./RuntimeManagementDialog";
import type { RuntimeChatDependencies } from "../runtimeInstallation";
import { Modal } from "./ui/Modal";
import "../styles/cli-launch.css";

export function CliLaunchDialog({ initialRuntime, initialDirectory, projects, onLaunch, onClose }: {
  initialRuntime: NewSessionAgent;
  initialDirectory?: SessionDirectory | null;
  projects: Project[];
  onLaunch: (kind: NewSessionAgent, selection: RuntimeLaunchSelection, directory: SessionDirectory) => Promise<boolean>;
  onClose: () => void;
}) {
  const availability = useRuntimeAvailability();
  const [kind, setKind] = useState(initialRuntime);
  const [override, setOverride] = useState<string | null>(null);
  const [saveAsDefault, setSaveAsDefault] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [managing, setManaging] = useState(false);
  const [chatDependencies, setChatDependencies] = useState<RuntimeChatDependencies | null>(null);
  const [selectedDirectory, setSelectedDirectory] = useState(initialDirectory ?? null);
  const [picking, setPicking] = useState(false);
  const pickingRef = useRef(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  const order = useRuntimeOrder(TERMINAL_AGENTS, NEW_SESSION_TERMINAL_ORDER_STORAGE_KEY);
  const chatOrder = useRuntimeOrder(CHAT_AGENTS, NEW_SESSION_CHAT_ORDER_STORAGE_KEY);
  const submitting = useRef(false);
  const directory = selectedDirectory ? resolveSessionDirectory(selectedDirectory.path, projects) : null;
  const [isolated, setIsolated] = useState(false);
  const [baseRef, setBaseRef] = useState("HEAD");
  const [fetchUpstream, setFetchUpstream] = useState(false);
  const [branchAttempt, setBranchAttempt] = useState(0);
  const [branchResult, setBranchResult] = useState<{ cwd: string; refs?: string[]; error?: string } | null>(null);
  const cwd = directory?.path;
  useEffect(() => {
    let current = true;
    setBaseRef("HEAD");
    setFetchUpstream(false);
    setBranchResult(null);
    if (isolated && cwd) {
      void invoke<string[]>("worktree_branches", { cwd }).then(refs => {
        if (current) setBranchResult({ cwd, refs });
      }).catch(reason => { if (current) setBranchResult({ cwd, error: String(reason) }); });
    }
    return () => { current = false; };
  }, [isolated, cwd, branchAttempt]);
  const branches = branchResult?.cwd === cwd ? branchResult : null;
  const worktreeReady = !isolated || Boolean(branches?.refs);
  const cli = isCliRuntime(kind);
  const chatDependency = chatDependencies?.[kind];
  useEffect(() => {
    if (cli || managing) return;
    let live = true;
    const timer = window.setTimeout(() => {
      void invoke<RuntimeChatDependencies>("runtime_chat_dependencies").then(result => {
        if (live) setChatDependencies(result ?? null);
      }).catch(() => { if (live) setChatDependencies(null); });
    }, 100);
    return () => { live = false; window.clearTimeout(timer); };
  }, [cli, managing]);
  const defaultMode = availability.prefs.defaultLaunchModes?.[kind] ?? "cli_default";
  const mode = override ?? defaultMode;
  const choice = launchChoices(kind).find(choice => choice.id === mode);
  const busy = pending || picking;
  const close = () => { if (!submitting.current && !pickingRef.current) onClose(); };
  const pickDirectory = async () => {
    if (pickingRef.current || submitting.current) return;
    pickingRef.current = true;
    setPicking(true); setError(null);
    try {
      const path = await openDirectory({ directory: true, multiple: false, title: "选择工作目录", defaultPath: directory?.path });
      if (mounted.current && typeof path === "string" && path.trim()) setSelectedDirectory(resolveSessionDirectory(path, projects));
    } catch (reason) {
      if (mounted.current) setError(`选择目录失败：${String(reason)}`);
    } finally {
      pickingRef.current = false;
      if (mounted.current) setPicking(false);
    }
  };
  return <Modal className="cli-launch-dialog" ariaLabel="新建会话" onClose={close} closeOnOverlayClick>
    <header><h2>新建会话</h2><button type="button" className="icon-button" aria-label="关闭启动设置" disabled={busy} onClick={close}><X size={19} /></button></header>
    <form onSubmit={async event => {
      event.preventDefault();
      if (submitting.current || pickingRef.current || !directory || !worktreeReady || (cli && !choice) || !availability.ready || !availability.isRuntimeEnabled(kind)) return;
      submitting.current = true; setPending(true); setError(null);
      try {
        const started = await onLaunch(kind, { mode, saveAsDefault: cli && saveAsDefault, ...(isolated && { worktreeOptions: { baseRef, fetchUpstream } }) }, directory);
        if (!started && mounted.current) setError("尚未启动；选择已保留，可以调整后重试。");
      } catch (reason) { if (mounted.current) setError(`启动失败：${String(reason)}`); }
      finally { submitting.current = false; if (mounted.current) setPending(false); }
    }}>
      <label>Agent<span className="cli-launch-agent-field"><img src={RUNTIME_LOGOS[kind]} data-runtime={kind} alt="" /><select aria-label="启动 Agent" value={kind} disabled={busy} onChange={event => { setKind(event.target.value as NewSessionAgent); setOverride(null); setSaveAsDefault(false); setError(null); }}>
        {[{ label: "CLI 终端 Agent", items: order.order }, { label: "原生聊天 Agent", items: chatOrder.order }].map(group => <optgroup label={group.label} key={group.label}>
          {group.items.map(runtime => <option key={runtime} value={runtime} disabled={!availability.isRuntimeEnabled(runtime)}>{RUNTIME_LABELS[runtime]}</option>)}
        </optgroup>)}
      </select></span></label>
      <section className="cli-launch-directory" aria-label="工作目录">
        <label htmlFor="session-work-directory">工作目录</label>
        <div className="cli-launch-directory-controls">
          <select id="session-work-directory" aria-label="工作目录" value={directory?.id ?? (directory ? "temporary" : "")} disabled={busy} onChange={event => {
            const project = projects.find(item => item.id === event.target.value);
            if (project) { setSelectedDirectory(project); setError(null); }
          }}>
            {!directory && <option value="" disabled>请选择已有项目或浏览目录</option>}
            {projects.map(project => <option key={project.id} value={project.id}>{project.name} · {project.path}</option>)}
            {directory && !directory.id && <option value="temporary">临时目录 · {directory.name}</option>}
          </select>
          <button type="button" className="btn secondary" disabled={busy} onClick={() => void pickDirectory()}>{picking ? "正在选择…" : "浏览其他目录…"}</button>
        </div>
        <p className="cli-launch-directory-path">{directory?.path ?? "尚未选择工作目录"}</p>
        <p className="cli-launch-directory-hint">{directory ? directory.id ? `会话归入「${directory.name}」项目` : "临时使用此目录，会话归入未分组" : "选择目录后即可启动；临时目录无需添加为项目。"}</p>
      </section>
      <section className="cli-launch-directory" aria-label="工作树">
        <label className="cli-launch-save"><input type="checkbox" checked={isolated} disabled={busy || !directory} onChange={event => setIsolated(event.target.checked)} /> 使用独立工作树</label>
        {isolated && <>
          <label htmlFor="worktree-base-ref">基准分支</label>
          <select id="worktree-base-ref" value={baseRef} disabled={busy || !branches?.refs} onChange={event => setBaseRef(event.target.value)}>
            <option value="HEAD">当前检出提交（HEAD）</option>
            {branches?.refs?.map(ref => <option key={ref} value={ref}>{ref.replace(/^refs\/(heads|remotes)\//, "")}</option>)}
          </select>
          <label className="cli-launch-save"><input type="checkbox" checked={fetchUpstream} disabled={busy} onChange={event => setFetchUpstream(event.target.checked)} /> 创建前获取上游更新</label>
          {!branches && <small role="status">正在读取 Git 分支…</small>}
          {branches?.error && <div className="cli-launch-error" role="alert">{branches.error} <button type="button" className="btn secondary" onClick={() => setBranchAttempt(n => n + 1)}>重试</button></div>}
        </>}
      </section>
      {!cli && chatDependency?.needsSetup && <div className="cli-launch-chat-setup">
        <span>{chatDependency.label}</span>
        <button type="button" className="btn secondary" disabled={busy} onClick={() => setManaging(true)}>安装与版本</button>
      </div>}
      {cli && <><label>启动模式<select aria-label="本次启动模式" value={mode} disabled={!availability.ready || busy} onChange={event => setOverride(event.target.value)}>
        {!choice && <option value={mode} disabled>不支持的已保存模式：{mode}</option>}
        {launchChoices(kind).map(choice => <option key={choice.id} value={choice.id}>{choice.label}</option>)}
      </select></label>
      <p className="cli-launch-default">当前默认：{launchModeLabel(kind, defaultMode)}</p>
      {choice && <p className="cli-launch-description">{choice.description}</p>}
      <label className="cli-launch-save"><input type="checkbox" checked={saveAsDefault} disabled={busy} onChange={event => setSaveAsDefault(event.target.checked)} />设为 {RUNTIME_LABELS[kind]} 的默认模式</label></>}
      {(error || availability.error) && <p className="cli-launch-error" role="alert">{error || availability.error}</p>}
      {!availability.ready && <button type="button" className="btn secondary" onClick={() => void availability.refresh()}>重新读取配置</button>}
      <footer>
        {cli && <button type="button" className="cli-launch-manage" disabled={busy} onClick={() => setManaging(true)}>管理默认模式</button>}
        <button type="button" className="btn secondary" disabled={busy} onClick={close}>取消</button>
        <button type="submit" className="btn" disabled={!directory || !worktreeReady || (cli && !choice) || !availability.ready || busy || !availability.isRuntimeEnabled(kind)}>{pending ? "正在启动…" : "启动"}</button>
      </footer>
    </form>
    {managing && <RuntimeManagementDialog order={[...order.order, ...chatOrder.order]} orderError={order.error || chatOrder.error} initialInstaller={cli ? undefined : kind} onClose={() => { setChatDependencies(null); setManaging(false); }} onReorder={(kind, target, position) => {
      if (isCliRuntime(kind) && isCliRuntime(target)) order.reorder(kind, target, position);
      else if (CHAT_AGENTS.includes(kind as NewSessionAgent) && CHAT_AGENTS.includes(target as NewSessionAgent)) chatOrder.reorder(kind as NewSessionAgent, target as NewSessionAgent, position);
    }} />}
  </Modal>;
}
