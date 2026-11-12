import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

interface Info { supported: boolean; version: string | null; installedVersion: string | null; managed: boolean; recoveryPending: boolean; updateError?: string | null; package: { platform: string; asset: string; extensions: string[] } | null; agents: { id: string; installed: boolean }[] }
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
  const [confirmMajor, setConfirmMajor] = useState(false);
  const [confirmRecovery, setConfirmRecovery] = useState(false);
  const mounted = useRef(true);
  const submittingRef = useRef(false);
  const completeId = useRef<string | null>(null);
  const activeVersion = useRef<string | null>(null);
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
      if (next.state === "awaiting-session" && activeVersion.current) setInfo(current => current ? {...current,installedVersion:activeVersion.current,managed:true} : current);
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
      setConfirmMajor(false);
      setProject(current => previous?.projectDir ?? current);
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
        if (next.state !== "running") {
          // The transaction may have restored an old version or left a pending
          // recovery journal. Refresh detection without clearing the user's draft.
          void invoke<Info>("ai_memory_quick_info").then(detected => {
            if (mounted.current && completeId.current === next.id) setInfo(detected);
          }).catch(reason => { if (mounted.current) setError(`更新状态复核失败，请点击检查更新：${String(reason)}`); });
          return;
        }
      } catch (reason) { if (cancelled || !mounted.current) return; setError(`读取进度失败，将继续重试：${String(reason)}`); }
      timer = setTimeout(() => void poll(), 1000);
    };
    timer = setTimeout(() => void poll(), 500);
    return () => { cancelled = true; clearTimeout(timer); };
  }, [job?.id, job?.state, accept]);

  const choose = async (kind: "project" | "archive") => {
    try {
      const selected = await open(kind === "project" ? { directory: true, multiple: false, title: "选择启用共享记忆的项目" } : { multiple: false, title: `选择官方 v${info?.version} ${info?.package?.platform ?? ""} 安装包`, filters: [{ name: info?.package?.asset.endsWith(".zip") ? "ZIP" : "TAR.GZ", extensions: info?.package?.extensions ?? ["zip", "gz"] }] });
      if (!mounted.current || typeof selected !== "string") return;
      if (kind === "project") setProject(selected); else setArchive(selected);
      setError(null);
    } catch (reason) { if (mounted.current) setError(`选择失败：${String(reason)}`); }
  };
  const newer = Boolean(info?.version && info.installedVersion && info.version.split(".").reduce((order,part,index) => order || Number(part) - Number(info.installedVersion!.split(".")[index]),0) > 0);
  const majorChange = Boolean(info?.version && info.installedVersion && info.version.split(".")[0] !== info.installedVersion.split(".")[0]);
  const start = async (update = false) => {
    if (submittingRef.current || locked || !project || !agents.length || !info?.supported) return;
    submittingRef.current = true; setSubmitting(true); setError(null); callbacks.current.onRunningChange(true);
    try {
      activeVersion.current = update || !info.installedVersion ? info.version : null;
      const next = await invoke<Job>("ai_memory_quick_start", { request: { projectDir: project, agents, localArchive: archive, targetVersion:activeVersion.current, update, confirmMajor } });
      accept(next);
    } catch (reason) {
      callbacks.current.onRunningChange(false);
      if (mounted.current) setError(String(reason));
    } finally { submittingRef.current = false; if (mounted.current) setSubmitting(false); }
  };

  const recover = async () => {
    if (locked || !confirmRecovery) return;
    setSubmitting(true); callbacks.current.onRunningChange(true); setError(null);
    try { await invoke("ai_memory_recover_update",{confirmed:true}); callbacks.current.onComplete(); setInfo(current=>current?{...current,recoveryPending:false}:current); }
    catch(reason) { setError(String(reason)); }
    finally { setSubmitting(false); callbacks.current.onRunningChange(false); }
  };

  return <section hidden={hidden} className="sm-card sm-quick" aria-labelledby="sm-quick-title">
    <div className="sm-quick-heading"><div><h3 id="sm-quick-title">快速启用共享记忆</h3><p className="sm-muted">选择项目和智能体，自动安装本地服务并配置接入。</p></div>
      <div className="sm-actions"><button type="button" className="sm-button" disabled={locked} onClick={() => void load()}>检查更新</button><button type="button" className="sm-button" disabled={loading || disabled || running} aria-expanded={expanded} onClick={() => { if (expanded && info) setExpanded(false); else void load(); }}>{loading ? "检查环境中…" : expanded && info ? "收起向导" : "开始配置"}</button></div>
    </div>
    {expanded && <div className="sm-quick-content">
      {error && <p role="alert" className="sm-error">{error}</p>}
      {!info && !loading && <button type="button" className="sm-button" onClick={() => void load()}>重新检查环境</button>}
      {info && <>
        <p className="sm-muted">当前版本：{info.installedVersion ? `v${info.installedVersion}` : "未安装"} · 官方最新稳定版：{info.version ? `v${info.version}` : "未获取"}{info.package && ` · ${info.package.platform}`}。默认无需模型 API Key，记忆保存在当前电脑。</p>
        {info.updateError && <p role="status">{info.updateError}。不会影响当前版本继续运行。</p>}
        {info.installedVersion && !info.managed && <p className="sm-muted">检测到外部安装，请通过 Homebrew、Scoop 或原安装方式更新；CrewVela 不会覆盖。</p>}
        {info.recoveryPending && <div role="alert"><p>上次更新未完成，请先恢复升级前快照。候选数据会保留在备份目录，不会自动合并升级后的写入。</p><label><input type="checkbox" checked={confirmRecovery} onChange={e=>setConfirmRecovery(e.target.checked)} />确认恢复旧快照，并保留候选数据供核对</label><button type="button" className="sm-button" disabled={locked||!confirmRecovery} onClick={()=>void recover()}>恢复升级前快照</button></div>}
        {!info.supported && <p role="status">当前平台暂不支持快速启用，请使用接入配置中的手动操作。</p>}
        <button type="button" className="sm-inline-link" disabled={locked} onClick={() => void load()}>重新检查 CLI 安装状态</button>
        <fieldset disabled={locked || !info.supported} className="sm-quick-fields">
          <legend>项目与智能体</legend>
          <div className="sm-project-picker"><code className="sm-project-path">{project || "尚未选择项目目录"}</code><button type="button" className="sm-button" onClick={() => void choose("project")}>选择启用项目</button></div>
          <div className="sm-quick-agents">{info.agents.map(agent => <label key={agent.id}><input type="checkbox" checked={agents.includes(agent.id)} disabled={!agent.installed} onChange={event => setAgents(current => event.target.checked ? [...current, agent.id] : current.filter(id => id !== agent.id))} />{labels[agent.id] ?? agent.id}{!agent.installed && <small>未检测到 CLI，请先安装并重新检查</small>}</label>)}</div>
          <p className="sm-muted">Claude Code 的钩子写入所选项目；MCP 和 Codex 钩子写入用户配置。使用 allowlist，仅捕获有 <code>.ai-memory.toml</code> 标记的项目。已有记忆、其他服务和自定义路由会保留。</p>
          {info.version && !info.updateError && <details><summary>下载较慢？使用本地官方安装包</summary><p><a href={`https://github.com/akitaonrails/ai-memory/releases/tag/v${info.version}`} target="_blank" rel="noreferrer">下载 v{info.version} 的 {info.package?.asset ?? "对应平台安装包"}</a>。导入时同样校验 SHA256 和架构；仍需联网读取官方校验信息。</p><div className="sm-quick-archive"><button type="button" className="sm-button" onClick={() => void choose("archive")}>选择本地{info.package?.asset.endsWith(".zip") ? " ZIP" : " TAR.GZ"}</button>{archive && <><code>{archive}</code><button type="button" className="sm-button" onClick={() => setArchive(null)}>改用在线下载</button></>}</div></details>}
        </fieldset>
        {newer && info.managed && <div><p className="sm-muted">更新将短暂停止记忆服务并备份完整数据和所选 Agent 配置。请先暂停依赖记忆的会话；升级后的数据回退需使用旧快照，候选数据另行保留。</p>{majorChange && <label><input type="checkbox" checked={confirmMajor} disabled={locked} onChange={e=>setConfirmMajor(e.target.checked)} />确认跨大版本迁移，已暂停依赖会话并了解快照恢复范围</label>}</div>}
        <div className="sm-quick-actions"><button type="button" className="sm-button sm-primary" disabled={locked || info.recoveryPending || !info.supported || !project || !agents.length || (!info.installedVersion && (!info.version||!!info.updateError))} onClick={() => void start()}>{running ? "正在启用…" : job?.state === "failed" ? "重试启用" : job?.state === "awaiting-session" ? "重新检查并配置" : "安装并启用"}</button>{newer && info.managed && <button type="button" className="sm-button" disabled={locked||info.recoveryPending||!!info.updateError||!project||!agents.length||(majorChange&&!confirmMajor)} onClick={()=>void start(true)}>安装更新</button>}<span className="sm-muted">不会自动重启正在运行的会话。</span></div>
      </>}
      {job && <div aria-live="polite" aria-atomic="false"><ol className="sm-quick-steps">{job.steps.map(step => <li key={step.id} data-state={step.state}><span aria-hidden="true">{step.state === "done" ? "✓" : step.state === "failed" ? "!" : step.state === "running" ? "…" : "·"}</span><div><strong>{step.label}</strong>{step.detail && <p>{step.detail}</p>}</div><small>{({ pending: "等待", running: "执行中", done: "完成", failed: "失败" })[step.state]}</small></li>)}</ol>
        {job.state === "awaiting-session" && <div className="sm-quick-result"><strong>配置完成，待新会话验证</strong><p>请在该项目新建或重新加载所选智能体会话，确认 ai-memory 工具可用。完成一次会话后，再检查真实捕获记录。</p><button type="button" className="sm-button" onClick={() => onVerify(job.projectDir)}>前往接入检查</button></div>}
      </div>}
    </div>}
  </section>;
}
