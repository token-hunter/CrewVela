import { useEffect, useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { Play } from "@phosphor-icons/react";
import type { ProjectPreviewConfig } from "../types";
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

export function ProjectPreviewActions({ projectId, planId, config, root, onTasks }: {
  projectId: string;
  planId: string;
  config?: ProjectPreviewConfig;
  root: string;
  onTasks?: () => void;
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

  useEffect(() => {
    let current = true;
    const refresh = () => {
      if (action.current) return;
      void invoke<PreviewStatus>("ma_project_preview_status", { projectId, logs: logsOpen })
        .then(value => { if (current && !action.current) setStatus(value); })
        .catch(reason => { if (current) setError(String(reason)); });
    };
    refresh();
    const timer = running ? window.setInterval(refresh, 650) : undefined;
    return () => { current = false; window.clearInterval(timer); };
  }, [projectId, logsOpen, running]);

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
    <div className="team-plan-actions">
      {status.state === "running" && status.url ? <button type="button" className="conversation-primary" onClick={() => browse(status.url!)}>打开网页</button>
        : <button type="button" className="conversation-primary" disabled={busy || !editable}
          onClick={() => stored ? void withAction(launch) : showConfig()}><Play size={17} />{status.state === "starting" ? "启动中…" : "启动项目"}</button>}
      {!editable && <button type="button" className="conversation-text-button" disabled={busy} onClick={() => void withAction(async () => {
        openAfterStart.current = false;
        setStatus(await invoke<PreviewStatus>("ma_project_preview_stop", { projectId }));
      })}>停止项目</button>}
      <button type="button" className="conversation-text-button" onClick={() => void withAction(async () => {
        await invoke("workspace_reveal_path", { root, relativePath: "README.md" });
      })}>查看启动说明</button>
      {stored && editable && <button type="button" className="conversation-text-button" disabled={busy} onClick={showConfig}>配置启动</button>}
      {onTasks && <button type="button" className="conversation-text-button" onClick={onTasks}>查看任务执行</button>}
    </div>
    {status.state === "starting" && <p className="project-preview-feedback" role="status">正在等待本地服务响应…</p>}
    {status.state === "running" && <p className="project-preview-feedback" role="status">运行中 · {status.url}</p>}
    {(error || status.error) && <p className="project-preview-feedback" role="alert">{error || status.error}</p>}
    {(status.state !== "idle" || error) && <details className="project-preview-logs" onToggle={event => setLogsOpen(event.currentTarget.open)}>
      <summary>启动日志</summary><pre>{status.logs || "暂无启动输出"}</pre>
    </details>}
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
