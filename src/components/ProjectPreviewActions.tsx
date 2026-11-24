import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { CaretRight, Check, FileText, ListBullets, Play, SlidersHorizontal, Square, WarningCircle } from "@phosphor-icons/react";
import type { DeliveryDemo, ProjectPreviewConfig } from "../types";
import { Modal } from "./ui/Modal";

interface PreviewStatus {
  state: "idle" | "starting" | "running" | "failed" | "stopFailed";
  url?: string;
  error?: string;
  logs?: string;
}

type StartResult = { kind: "status"; status: PreviewStatus } | { kind: "confirm"; version: string; message: string };

const defaultConfig: ProjectPreviewConfig = { program: "", args: [], workingDirectory: ".", url: "http://127.0.0.1:3000/" };

function browse(url: string) {
  window.dispatchEvent(new CustomEvent("crewvela:browse", { detail: { url } }));
}

export function ProjectPreviewActions({ projectId, planId, config, demo, root, onTasks, subtitle, children }: {
  projectId: string;
  planId: string;
  config?: ProjectPreviewConfig;
  demo?: DeliveryDemo;
  root: string;
  onTasks?: () => void;
  subtitle?: string;
  children?: ReactNode;
}) {
  const [saved, setSaved] = useState<ProjectPreviewConfig>();
  const [status, setStatus] = useState<PreviewStatus>({ state: "idle" });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [form, setForm] = useState<(Omit<ProjectPreviewConfig, "args"> & { args: string }) | null>(null);
  const [formError, setFormError] = useState("");
  const [logsOpen, setLogsOpen] = useState(false);
  const action = useRef(false);
  const openAfterStart = useRef(false);
  const running = status.state === "starting" || status.state === "running";
  const editable = !running && status.state !== "stopFailed";
  const stored = saved ?? config;
  const preparing = status.state === "starting" || (demo?.state === "preparing" && status.state === "idle");
  const demoError = demo?.state === "unavailable" && status.state !== "running" && !preparing ? demo.error : undefined;
  const feedback = error || status.error || demoError;
  const demoAction = preparing ? "正在准备 Demo…" : demo?.state === "ready" || status.state === "running" ? "打开 Demo" : "重试 Demo";
  const stateLabel = preparing ? "启动中" : status.state === "running" ? "运行中" : feedback ? "不可用" : "未运行";
  const visualState = preparing ? "starting" : status.state === "running" ? "running" : feedback ? "failed" : status.state;
  const showLogs = status.state !== "idle" || !!feedback;

  useEffect(() => {
    let current = true;
    const refresh = () => {
      if (action.current) return;
      void invoke<PreviewStatus>("ma_project_preview_status", { projectId, planId, logs: logsOpen })
        .then(value => { if (current && !action.current) setStatus(value); })
        .catch(reason => { if (current) setError(String(reason)); });
    };
    refresh();
    const timer = running || preparing ? window.setInterval(refresh, 650) : undefined;
    return () => { current = false; window.clearInterval(timer); };
  }, [projectId, planId, logsOpen, running, preparing]);

  useEffect(() => {
    if (!openAfterStart.current) return;
    if (status.state === "running" && status.url) {
      openAfterStart.current = false;
      browse(status.url);
    } else if (status.state === "failed" || status.state === "stopFailed") {
      openAfterStart.current = false;
    }
  }, [status]);

  const withAction = async (run: () => Promise<void>) => {
    if (action.current) return;
    action.current = true;
    setBusy(true);
    setError("");
    try { await run(); }
    catch (reason) { setError(String(reason)); }
    finally { action.current = false; setBusy(false); }
  };

  const launch = async () => {
    let confirmedVersion: string | undefined;
    for (;;) {
      const result = await invoke<StartResult>("ma_project_preview_start", { projectId, planId, confirmedVersion });
      if (result.kind === "status") {
        openAfterStart.current = true;
        setStatus(result.status);
        return;
      }
      if (!window.confirm(result.message)) return;
      confirmedVersion = result.version;
    }
  };

  const showConfig = () => {
    const values = stored ?? defaultConfig;
    setFormError("");
    setForm({ ...values, args: values.args.join("\n") });
  };
  const save = (event: FormEvent) => {
    event.preventDefault();
    if (!form) return;
    const values: ProjectPreviewConfig = {
      program: form.program.trim(), args: form.args.split("\n").filter(Boolean),
      workingDirectory: form.workingDirectory.trim() || ".", url: form.url?.trim() ?? null,
    };
    void withAction(async () => {
      setFormError("");
      try { await invoke("ma_project_preview_save", { projectId, planId, config: values }); }
      catch (reason) { setFormError(String(reason)); return; }
      setSaved(values);
      setForm(null);
      await launch();
    });
  };

  return <>
    <header className="team-plan-heading team-delivery-heading">
      <div className="delivery-heading-main">
        <span className="delivery-heading-badge" aria-hidden="true"><Check size={14} weight="bold" /></span>
        <div className="delivery-heading-text">
          <h3>成品已交付</h3>
          {subtitle && <span className="delivery-heading-sub">{subtitle}</span>}
        </div>
      </div>
      <span className="project-preview-state" data-state={visualState} role="status">{stateLabel}</span>
    </header>
    <div className="delivery-actions-bar">
      {(!demo || stored || !editable) && <div className="project-preview-buttons">
        {demo ? (stored && <button type="button" className="project-preview-open" disabled={busy || preparing || status.state === "stopFailed"}
          onClick={() => void withAction(launch)}><Play size={13} weight="fill" aria-hidden="true" /><span>{demoAction}</span></button>)
          : status.state === "running" && status.url ? <button type="button" className="project-preview-open" onClick={() => browse(status.url!)}><Play size={13} weight="fill" aria-hidden="true" /><span>打开网页</span></button>
          : <button type="button" className="project-preview-open" disabled={busy || !editable}
            onClick={() => stored ? void withAction(launch) : showConfig()}><Play size={13} weight="fill" aria-hidden="true" /><span>{status.state === "starting" ? "启动中…" : "启动项目"}</span></button>}
        {(!!(demo && stored) || !editable) && <button type="button" className="project-preview-stop"
          data-compact={editable ? "true" : undefined}
          title={editable ? "当前未运行" : (demo ? "停止 Demo" : "停止项目")}
          disabled={busy || editable} onClick={() => void withAction(async () => {
          openAfterStart.current = false;
          setStatus(await invoke<PreviewStatus>("ma_project_preview_stop", { projectId, planId }));
        })}><Square size={12} weight="fill" aria-hidden="true" /><span className="delivery-stop-label">{demo ? "停止 Demo" : "停止项目"}</span></button>}
      </div>}
      <div className="project-preview-links">
        <button type="button" onClick={() => void withAction(async () => {
          await invoke("workspace_reveal_path", { root, relativePath: "README.md" });
        })}><FileText size={14} aria-hidden="true" /><span>查看启动说明</span></button>
        {!demo && stored && editable && <button type="button" disabled={busy} onClick={showConfig}><SlidersHorizontal size={14} aria-hidden="true" /><span>配置启动</span></button>}
        {onTasks && <button type="button" onClick={onTasks}><ListBullets size={14} aria-hidden="true" /><span>查看任务执行</span></button>}
      </div>
    </div>
    {(children || preparing || feedback || showLogs) && <div className="delivery-sunken-panel">
      {children}
      {preparing && <p className="project-preview-feedback" role="status">正在等待本地服务响应…</p>}
      {feedback && <p className="project-preview-feedback" role="alert"><WarningCircle size={14} aria-hidden="true" /><span>{demo && "Demo 暂不可用。"}{feedback}</span></p>}
      {showLogs && <footer className="project-preview-footer">
        {status.state === "running" && status.url && <code className="project-preview-url">{status.url}</code>}
        <details className="project-preview-logs" onToggle={event => setLogsOpen(event.currentTarget.open)}>
          <summary><CaretRight size={13} aria-hidden="true" />启动日志</summary><pre>{status.logs || "暂无启动输出"}</pre>
        </details>
      </footer>}
    </div>}
    {form && createPortal(<Modal className="collab-agent-editor project-preview-config" overlayClassName="dialog-overlay collab-agent-editor-overlay"
      ariaLabel="配置项目启动" onClose={() => { if (!busy) setForm(null); }}>
      <header><h3>配置项目启动</h3></header>
      <form onSubmit={save}>
        <label>启动程序<input autoComplete="off" required value={form.program} placeholder="node" onChange={event => setForm({ ...form, program: event.target.value })} /></label>
        <label>启动参数（每行一个）<textarea rows={2} value={form.args} placeholder="server.js" onChange={event => setForm({ ...form, args: event.target.value })} /></label>
        <label>工作目录<input value={form.workingDirectory} placeholder="." onChange={event => setForm({ ...form, workingDirectory: event.target.value })} /></label>
        {form.url !== null && <label>本地地址<input required value={form.url} placeholder="http://127.0.0.1:3000/" onChange={event => setForm({ ...form, url: event.target.value })} /></label>}
        <label className="project-preview-dynamic"><input type="checkbox" checked={form.url === null}
          onChange={event => setForm({ ...form, url: event.target.checked ? null : defaultConfig.url })} />从启动输出读取地址（PORT=0）</label>
        {formError && <p role="alert">{formError}</p>}
        <footer><button type="button" className="btn secondary" disabled={busy} onClick={() => setForm(null)}>取消</button><button type="submit" className="btn primary" disabled={busy}>保存并启动</button></footer>
      </form>
    </Modal>, document.body)}
  </>;
}
