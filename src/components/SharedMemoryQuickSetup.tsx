import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

interface Info { supported: boolean; version: string; agents: { id: string; installed: boolean }[] }
interface Job { id: string; projectDir: string; agents: string[]; state: string; steps: { id: string; label: string; state: string; detail: string }[] }
interface Props { hidden: boolean; disabled: boolean; onRunningChange: (running: boolean) => void; onComplete: () => void; onVerify: (project: string) => void }
const labels: Record<string, string> = { "claude-code": "Claude Code", codex: "Codex" };

export function SharedMemoryQuickSetup({ hidden, disabled, onRunningChange, onComplete, onVerify }: Props) {
  const [expanded, setExpanded] = useState(false);
  const [info, setInfo] = useState<Info | null>(null);
  const [job, setJob] = useState<Job | null>(null);
  const [project, setProject] = useState("");
  const [agents, setAgents] = useState<string[]>([]);
  const [archive, setArchive] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(true);
  const submittingRef = useRef(false);
  const completeId = useRef<string | null>(null);
  const callbacks = useRef({ onRunningChange, onComplete });
  callbacks.current = { onRunningChange, onComplete };
  const running = submitting || job?.state === "running";
  const locked = loading || running || disabled;

  const accept = useCallback((next: Job | null) => {
    if (!mounted.current) return;
    setJob(next);
    callbacks.current.onRunningChange(next?.state === "running");
    if (next && next.state !== "running" && completeId.current !== next.id) {
      completeId.current = next.id;
      callbacks.current.onComplete();
    }
  }, []);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; callbacks.current.onRunningChange(false); };
  }, []);

  const load = async () => {
    if (loading || submittingRef.current) return;
    setExpanded(true); setLoading(true); setError(null);
    try {
      const [detected, previous] = await Promise.all([invoke<Info>("ai_memory_quick_info"), invoke<Job | null>("ai_memory_quick_status")]);
      if (!mounted.current) return;
      setInfo(detected);
      setProject(previous?.projectDir ?? "");
      const available = detected.agents.filter(agent => agent.installed).map(agent => agent.id);
      setAgents(previous?.state === "running" ? previous.agents : previous?.agents.filter(id => available.includes(id)) ?? available);
      accept(previous);
    } catch (reason) { if (mounted.current) setError(String(reason)); }
    finally { if (mounted.current) setLoading(false); }
  };

  useEffect(() => {
    if (job?.state !== "running") return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await invoke<Job | null>("ai_memory_quick_status");
        if (cancelled || !mounted.current) return;
        if (!next || next.id !== job.id) throw new Error("启用任务状态已变化，请重新读取进度");
        setError(null); accept(next);
        if (next.state !== "running") return;
      } catch (reason) { if (cancelled || !mounted.current) return; setError(`读取进度失败，将继续重试：${String(reason)}`); }
      timer = setTimeout(() => void poll(), 1000);
    };
    timer = setTimeout(() => void poll(), 500);
    return () => { cancelled = true; clearTimeout(timer); };
  }, [job?.id, job?.state, accept]);

  const choose = async (kind: "project" | "archive") => {
    try {
      const selected = await open(kind === "project" ? { directory: true, multiple: false, title: "选择启用共享记忆的项目" } : { multiple: false, title: "选择官方 v2.6.0 Windows x64 安装包", filters: [{ name: "ZIP", extensions: ["zip"] }] });
      if (!mounted.current || typeof selected !== "string") return;
      if (kind === "project") setProject(selected); else setArchive(selected);
      setError(null);
    } catch (reason) { if (mounted.current) setError(`选择失败：${String(reason)}`); }
  };
  const start = async () => {
    if (submittingRef.current || locked || !project || !agents.length || !info?.supported) return;
    submittingRef.current = true; setSubmitting(true); setError(null); callbacks.current.onRunningChange(true);
    try {
      const next = await invoke<Job>("ai_memory_quick_start", { request: { projectDir: project, agents, localArchive: archive } });
      accept(next);
    } catch (reason) {
      callbacks.current.onRunningChange(false);
      if (mounted.current) setError(String(reason));
    } finally { submittingRef.current = false; if (mounted.current) setSubmitting(false); }
  };

  return <section hidden={hidden} className="sm-card sm-quick" aria-labelledby="sm-quick-title">
    <div className="sm-quick-heading"><div><h3 id="sm-quick-title">快速启用共享记忆</h3><p className="sm-muted">选择项目和智能体，自动安装本地服务并配置接入。</p></div>
      <button type="button" className="sm-button" disabled={loading || disabled || running} aria-expanded={expanded} onClick={() => { if (expanded && info) setExpanded(false); else void load(); }}>{loading ? "检查环境中…" : expanded && info ? "收起向导" : "开始配置"}</button>
    </div>
    {expanded && <div className="sm-quick-content">
      {error && <p role="alert" className="sm-error">{error}</p>}
      {!info && !loading && <button type="button" className="sm-button" onClick={() => void load()}>重新检查环境</button>}
      {info && <>
        <p className="sm-muted">兼容版本 v{info.version} · Windows x64 · 约 20MB。默认无需 API Key，记忆保存在当前电脑。</p>
        {!info.supported && <p role="status">当前平台暂不支持快速启用，请使用接入配置中的手动操作。</p>}
        <button type="button" className="sm-inline-link" disabled={locked} onClick={() => void load()}>重新检查 CLI 安装状态</button>
        <fieldset disabled={locked || !info.supported} className="sm-quick-fields">
          <legend>项目与智能体</legend>
          <div className="sm-project-picker"><code className="sm-project-path">{project || "尚未选择项目目录"}</code><button type="button" className="sm-button" onClick={() => void choose("project")}>选择启用项目</button></div>
          <div className="sm-quick-agents">{info.agents.map(agent => <label key={agent.id}><input type="checkbox" checked={agents.includes(agent.id)} disabled={!agent.installed} onChange={event => setAgents(current => event.target.checked ? [...current, agent.id] : current.filter(id => id !== agent.id))} />{labels[agent.id] ?? agent.id}{!agent.installed && <small>未检测到 CLI，请先安装并重新检查</small>}</label>)}</div>
          <p className="sm-muted">Claude Code 的钩子写入所选项目；MCP 和 Codex 钩子写入用户配置。使用 allowlist，仅捕获有 <code>.ai-memory.toml</code> 标记的项目。已有记忆、其他服务和自定义路由会保留。</p>
          <details><summary>下载较慢？使用本地官方安装包</summary><p><a href="https://github.com/akitaonrails/ai-memory/releases/tag/v2.6.0" target="_blank" rel="noreferrer">下载 v2.6.0 的 ai-memory-windows-x86_64.zip</a>。导入时同样校验 SHA256。</p><div className="sm-quick-archive"><button type="button" className="sm-button" onClick={() => void choose("archive")}>选择本地 ZIP</button>{archive && <><code>{archive}</code><button type="button" className="sm-button" onClick={() => setArchive(null)}>改用在线下载</button></>}</div></details>
        </fieldset>
        <div className="sm-quick-actions"><button type="button" className="sm-button sm-primary" disabled={locked || !info.supported || !project || !agents.length} onClick={() => void start()}>{running ? "正在启用…" : job?.state === "failed" ? "重试启用" : job?.state === "awaiting-session" ? "重新检查并配置" : "安装并启用"}</button><span className="sm-muted">不会自动重启正在运行的会话。</span></div>
      </>}
      {job && <div aria-live="polite" aria-atomic="false"><ol className="sm-quick-steps">{job.steps.map(step => <li key={step.id} data-state={step.state}><span aria-hidden="true">{step.state === "done" ? "✓" : step.state === "failed" ? "!" : step.state === "running" ? "…" : "·"}</span><div><strong>{step.label}</strong>{step.detail && <p>{step.detail}</p>}</div><small>{({ pending: "等待", running: "执行中", done: "完成", failed: "失败" })[step.state]}</small></li>)}</ol>
        {job.state === "awaiting-session" && <div className="sm-quick-result"><strong>配置完成，待新会话验证</strong><p>请在该项目新建或重新加载所选智能体会话，确认 ai-memory 工具可用。完成一次会话后，再检查真实捕获记录。</p><button type="button" className="sm-button" onClick={() => onVerify(job.projectDir)}>前往接入检查</button></div>}
      </div>}
    </div>}
  </section>;
}
