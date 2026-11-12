import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

interface Info { supported: boolean; version: string | null; installedVersion: string | null; managed: boolean; recoveryPending: boolean; updateError?: string | null; package: { platform: string; asset: string; extensions: string[] } | null; agents: { id: string; installed: boolean }[] }
interface Job { id: string; projectDir: string; agents: string[]; update?: boolean; state: string; steps: { id: string; label: string; state: string; detail: string }[] }
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
      if (["awaiting-session", "updated"].includes(next.state) && activeVersion.current) setInfo(current => current ? {...current,installedVersion:activeVersion.current,managed:true} : current);
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
      setProject(current => previous?.projectDir || current);
      const available = detected.agents.filter(agent => agent.installed).map(agent => agent.id);
      setAgents(previous?.projectDir ? previous.agents.filter(id => available.includes(id)) : available);
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
  const updateDisabledReason = locked ? "有操作正在进行，请稍候。" : info?.recoveryPending ? "请先恢复上次更新的快照。" : !info?.supported ? "当前平台不支持托管更新。" : info.updateError ? info.updateError : !info.managed ? "仅支持更新 CrewVela 托管的服务。" : !newer ? "未发现可安装的新版本。" : majorChange && !confirmMajor ? "请先确认跨大版本迁移及快照恢复范围。" : "";
  const setupDisabledReason = locked ? "有操作正在进行，请稍候。" : info?.recoveryPending ? "请先恢复上次更新的快照。" : !info?.supported ? "当前平台不支持快速配置。" : !project ? "请选择需要接入的项目目录。" : !agents.length ? "请至少选择一个已安装的 Agent。" : !info.installedVersion && (!info.version || info.updateError) ? "请先获取可安装的兼容版本。" : "";
  const start = async (update = false) => {
    if (submittingRef.current || !info || (update ? updateDisabledReason : setupDisabledReason)) return;
    submittingRef.current = true; setSubmitting(true); setError(null); callbacks.current.onRunningChange(true);
    try {
      activeVersion.current = update || !info.installedVersion ? info.version : null;
      const common = { localArchive: archive, targetVersion: activeVersion.current, update, confirmMajor };
      const next = await invoke<Job>("ai_memory_quick_start", { request: update ? common : { ...common, projectDir: project, agents } });
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
    <div className="sm-quick-heading"><div><h3 id="sm-quick-title">共享记忆服务与接入</h3><p className="sm-muted">独立更新本地服务，或为项目配置智能体接入。</p></div>
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
        {info.version && !info.updateError && <details className="sm-quick-download"><summary>下载较慢？使用本地官方安装包</summary><p><a href={`https://github.com/akitaonrails/ai-memory/releases/tag/v${info.version}`} target="_blank" rel="noreferrer">下载 v{info.version} 的 {info.package?.asset ?? "对应平台安装包"}</a>。导入时同样校验 SHA256 和架构；仍需联网读取官方校验信息。</p><div className="sm-quick-archive"><button type="button" className="sm-button" disabled={locked} onClick={() => void choose("archive")}>选择本地{info.package?.asset.endsWith(".zip") ? " ZIP" : " TAR.GZ"}</button>{archive && <><code>{archive}</code><button type="button" className="sm-button" disabled={locked} onClick={() => setArchive(null)}>改用在线下载</button></>}</div></details>}
        {info.installedVersion && info.managed && <section className="sm-quick-update" aria-labelledby="sm-update-title">
          <h4 id="sm-update-title">更新服务</h4>
          <p className="sm-muted">沿用已有服务配置、项目标记和 Agent 接入，不需要选择项目或重新安装 Agent CLI。</p>
          {newer && <><p className="sm-muted">更新会短暂停止记忆服务，备份原安装、完整记忆数据与服务配置。请先暂停依赖记忆的会话；失败时恢复旧快照，候选数据另行保留。</p>
            {majorChange && <label className="sm-quick-confirm"><input type="checkbox" checked={confirmMajor} disabled={locked} onChange={e => setConfirmMajor(e.target.checked)} />确认跨大版本迁移，已暂停依赖会话并了解快照恢复范围</label>}
            <div className="sm-quick-actions"><button type="button" className="sm-button sm-primary" aria-describedby="sm-update-reason" disabled={!!updateDisabledReason} onClick={() => void start(true)}>{running && job?.update ? "正在更新…" : "安装更新"}</button><span id="sm-update-reason" className="sm-muted">{updateDisabledReason || "仅更新服务，已有 MCP 与 Hooks 配置不改写。"}</span></div>
          </>}
          {!newer && <p className="sm-muted">{info.updateError ? "暂时无法检查新版本，现有服务可继续使用。" : "当前未发现可安装的新版本。"}</p>}
        </section>}
        <button type="button" className="sm-inline-link" disabled={locked} onClick={() => void load()}>重新检查 CLI 安装状态</button>
        <fieldset disabled={locked || !info.supported} className="sm-quick-fields">
          <legend>配置项目接入</legend>
          <div className="sm-project-picker"><code className="sm-project-path">{project || "尚未选择项目目录"}</code><button type="button" className="sm-button" onClick={() => void choose("project")}>选择启用项目</button></div>
          <div className="sm-quick-agents">{info.agents.map(agent => <label key={agent.id}><input type="checkbox" checked={agents.includes(agent.id)} disabled={!agent.installed} onChange={event => setAgents(current => event.target.checked ? [...current, agent.id] : current.filter(id => id !== agent.id))} />{labels[agent.id] ?? agent.id}{!agent.installed && <small>未检测到 CLI，请先安装并重新检查</small>}</label>)}</div>
          <p className="sm-muted">Claude Code 的钩子写入所选项目；MCP 和 Codex 钩子写入用户配置。使用 allowlist，仅捕获有 <code>.ai-memory.toml</code> 标记的项目。已有记忆、其他服务和自定义路由会保留。</p>
          <div className="sm-quick-actions"><button type="button" className="sm-button sm-primary" aria-describedby="sm-setup-reason" disabled={!!setupDisabledReason} onClick={() => void start()}>{running && !job?.update ? "正在启用…" : job?.state === "failed" && !job.update ? "重试启用" : job?.state === "awaiting-session" ? "重新检查并配置" : info.installedVersion ? "配置项目接入" : "安装并启用"}</button><span id="sm-setup-reason" className="sm-muted">{setupDisabledReason || "仅配置所选项目与智能体，不更新已安装的服务。"}</span></div>
        </fieldset>
        <p className="sm-muted">不会自动重启正在运行的会话。</p>
      </>}
      {job && <div aria-live="polite" aria-atomic="false">{job.state === "failed" && <p role="alert" className="sm-error">{job.steps.find(step => step.state === "failed")?.label}：{job.steps.find(step => step.state === "failed")?.detail || "操作未完成，请检查上方提示后重试。"}</p>}<ol className="sm-quick-steps">{job.steps.map(step => <li key={step.id} data-state={step.state}><span aria-hidden="true">{step.state === "done" ? "✓" : step.state === "failed" ? "!" : step.state === "running" ? "…" : "·"}</span><div><strong>{step.label}</strong>{step.detail && <p>{step.detail}</p>}</div><small>{({ pending: "等待", running: "执行中", done: "完成", failed: "失败" })[step.state]}</small></li>)}</ol>
        {job.state === "awaiting-session" && <div className="sm-quick-result"><strong>配置完成，待新会话验证</strong><p>请在该项目新建或重新加载所选智能体会话，确认 ai-memory 工具可用。完成一次会话后，再检查真实捕获记录。</p><button type="button" className="sm-button" onClick={() => onVerify(job.projectDir)}>前往接入检查</button></div>}
        {job.state === "updated" && <div className="sm-quick-result"><strong>服务更新完成，已有接入配置保持不变</strong><p>升级前快照已保留。请复核原 Agent 的工具连接与捕获；跨版本接入如需调整，可在“配置项目接入”区域单独配置，服务健康不代表原会话已验证。</p></div>}
      </div>}
    </div>}
  </section>;
}
