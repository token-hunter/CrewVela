import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { ChatCircleTextIcon, CopyIcon, XIcon } from "@phosphor-icons/react";
import { communicationExample, communicationGuideSeen, communicationMembers, COMMUNICATION_SEEN_EVENT, COMMUNICATION_SEEN_KEY, markCommunicationGuideSeen, requestCommunicationGuide, type CommunicationParticipant, type CommunicationScope } from "../communicationGuide";
import type { AgentProfile, MultiAgentProject, MultiAgentSnapshot } from "../types";
import { RUNTIME_LOGOS } from "../runtimeLogos";
import "../styles/communication-guide.css";

export function CommunicationDiscoveryHint({ suppressed = false, projectId }: { suppressed?: boolean; projectId?: string }) {
  const [seen, setSeen] = useState(communicationGuideSeen);
  const [warning, setWarning] = useState("");
  useEffect(() => {
    const local = () => setSeen(true);
    const external = (event: StorageEvent) => { if (event.key === COMMUNICATION_SEEN_KEY || event.key === null) setSeen(communicationGuideSeen()); };
    window.addEventListener(COMMUNICATION_SEEN_EVENT, local); window.addEventListener("storage", external);
    return () => { window.removeEventListener(COMMUNICATION_SEEN_EVENT, local); window.removeEventListener("storage", external); };
  }, []);
  const dismiss = () => { setSeen(true); try { markCommunicationGuideSeen(); } catch { setWarning("本次已收起提示，但无法保存偏好，下次启动可能再次显示。"); } };
  if (suppressed) return null;
  return <>{!seen && <section className="communication-hint" aria-label="发现会话通信">
    <button type="button" className="communication-icon-button communication-hint-close" aria-label="关闭通信提示" onClick={dismiss}><XIcon size={15} /></button>
    <strong>{projectId ? "项目成员也可以互相联系" : "Agent 之间可以互相通信"}</strong>
    <p>{projectId ? "同一项目的成员可以互发消息、核对结果。" : "打开 Claude Code、Codex 等会话后，可以让它们互发消息、协作完成任务。"}</p>
    <button type="button" className="communication-text-button" onClick={() => requestCommunicationGuide({ scope: projectId ? "project" : "sessions", projectId })}>查看教程</button>
    <small>关闭后，可从「会话通信」再次查看。</small>
  </section>}{warning && <p className="communication-notice" role="status">{warning}</p>}</>;
}

const demoProject = { id: "preview-collaboration-project", name: "从 0 到 1 · Vibecoding 项目" } as MultiAgentProject;
const demoAgents = [
  { id: "guide-frontend", projectId: demoProject.id, name: "前端助手", runtimeKind: "claude_code", enabled: true },
  { id: "guide-reviewer", projectId: demoProject.id, name: "代码审查员", runtimeKind: "codex", enabled: true },
] as AgentProfile[];

interface Props {
  initialScope: CommunicationScope; initialProjectId?: string | null;
  sessions: CommunicationParticipant[]; selectedSessionId?: string | null; preview?: boolean; hidden?: boolean;
  onClose: () => void; onDiagnostics: (projectId: string | undefined, scope: CommunicationScope) => void;
}
export function CommunicationGuide({ initialScope, initialProjectId, sessions, selectedSessionId, preview = false, hidden = false, onClose, onDiagnostics }: Props) {
  const [scope, setScope] = useState(initialScope);
  const [projects, setProjects] = useState<MultiAgentProject[]>([]);
  const [projectId, setProjectId] = useState(initialProjectId ?? "");
  const [members, setMembers] = useState<CommunicationParticipant[]>([]);
  const [loadingProjects, setLoadingProjects] = useState(false);
  const [loadingMembers, setLoadingMembers] = useState(false);
  const loading = loadingProjects || loadingMembers;
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  const [sourceId, setSourceId] = useState(selectedSessionId ?? "");
  const [targetId, setTargetId] = useState("");
  const [copyState, setCopyState] = useState("");
  const [copyBusy, setCopyBusy] = useState(false);
  const copyPending = useRef(false);
  const copyRevision = useRef(0);
  const closeRef = useRef<HTMLButtonElement>(null);
  const selection = useRef(""); selection.current = `${scope}:${projectId}`;
  useEffect(() => {
    const previous = document.activeElement;
    closeRef.current?.focus();
    return () => { copyRevision.current++; if (previous instanceof HTMLElement && document.contains(previous)) previous.focus(); };
  }, []);
  useEffect(() => {
    if (scope !== "project") { setLoadingProjects(false); setLoadingMembers(false); return; }
    let cancelled = false;
    const selected = `project:${projectId}`;
    setLoadingProjects(true); setLoadingMembers(false); setError(""); setProjects([]); setMembers([]);
    (preview ? Promise.resolve([demoProject]) : invoke<MultiAgentProject[]>("ma_list_projects")).then(next => {
      if (cancelled || selection.current !== selected) return;
      if (!Array.isArray(next)) throw new Error("项目列表格式异常");
      setProjects(next);
      if (projectId && !next.some(project => project.id === projectId)) setError("所选项目已不可用，请明确选择其他项目。");
    }).catch(reason => { if (!cancelled && selection.current.startsWith("project:")) setError(`读取协作项目失败：${String(reason)}`); }).finally(() => { if (!cancelled && selection.current.startsWith("project:")) setLoadingProjects(false); });
    return () => { cancelled = true; };
  }, [scope, preview, retry]);
  useEffect(() => {
    if (scope !== "project" || !projectId || !projects.some(project => project.id === projectId)) { setLoadingMembers(false); return; }
    let cancelled = false;
    const selected = `project:${projectId}`;
    setLoadingMembers(true); setError(""); setMembers([]);
    (preview ? Promise.resolve({ project: demoProject, agents: demoAgents } as MultiAgentSnapshot) : invoke<MultiAgentSnapshot>("ma_snapshot", { projectId })).then(snapshot => {
      if (cancelled || selection.current !== selected) return;
      if (snapshot.project.id !== projectId || !Array.isArray(snapshot.agents)) throw new Error("返回的项目范围不匹配");
      setMembers(communicationMembers(snapshot.agents, projectId));
    }).catch(reason => { if (!cancelled && selection.current === selected) setError(`读取项目成员失败：${String(reason)}`); }).finally(() => { if (!cancelled && selection.current === selected) setLoadingMembers(false); });
    return () => { cancelled = true; };
  }, [scope, projectId, projects, preview]);
  const participants = scope === "sessions" ? sessions : members;
  const source = participants.find(item => item.id === sourceId) ?? participants.find(item => item.id === selectedSessionId) ?? participants[0];
  const target = participants.find(item => item.id === targetId && item.id !== source?.id) ?? participants.find(item => item.id !== source?.id && item.family !== source?.family) ?? participants.find(item => item.id !== source?.id);
  const example = communicationExample(scope, participants, source?.id ?? "", target?.id ?? "");
  const exampleKey = JSON.stringify([scope, projectId, source?.id, target?.id, example]);
  const activeExample = useRef(exampleKey); activeExample.current = exampleKey;
  useEffect(() => { copyRevision.current++; setCopyState(""); }, [exampleKey]);
  const copy = async () => {
    if (!example || copyPending.current) return;
    copyPending.current = true; setCopyBusy(true); setCopyState("");
    const revision = copyRevision.current, key = exampleKey;
    try {
      if (preview) await navigator.clipboard.writeText(example); else await writeText(example);
      if (revision === copyRevision.current && key === activeExample.current) setCopyState("示例已复制，请粘贴到发送方会话；尚未发送消息。");
    } catch (reason) { if (revision === copyRevision.current && key === activeExample.current) setCopyState(`复制失败：${String(reason)}`); }
    finally { copyPending.current = false; setCopyBusy(false); }
  };
  return <aside className="communication-guide" hidden={hidden} aria-labelledby="communication-guide-title" onKeyDown={event => { if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); onClose(); } }}>
    <header><div><h2 id="communication-guide-title">会话通信</h2><p>让不同 Agent 互相发消息、核对结果。</p></div><button ref={closeRef} type="button" className="communication-icon-button" onClick={onClose} aria-label="关闭通信指南"><XIcon size={19} /></button></header>
    <div className="communication-guide-body">
      <div className="communication-scopes" role="group" aria-label="通信场景">{([["sessions", "独立会话"], ["project", "项目成员"]] as const).map(([id, label]) => <button type="button" key={id} aria-pressed={scope === id} onClick={() => setScope(id)}>{label}</button>)}</div>
      {scope === "project" && <label className="communication-project">当前范围<select aria-label="通信示例项目" value={projectId} disabled={loading && projects.length === 0} onChange={event => { if (event.target.value === projectId) return; setProjectId(event.target.value); setMembers([]); setSourceId(""); setTargetId(""); }}><option value="">选择协作项目</option>{projects.map(project => <option key={project.id} value={project.id}>{project.name}</option>)}</select></label>}
      {error && <div role="alert" className="communication-error"><p>{error}</p><button type="button" className="communication-text-button" onClick={() => setRetry(value => value + 1)}>重新读取</button></div>}
      <h3>{scope === "project" ? "让两个项目成员配合" : "让 Agent 互相发消息"}</h3>
      <ol className="communication-steps">{(scope === "project" ? ["在同一项目中启用两位成员", "在成员会话中说出要联系谁", "在项目消息中查看往来"] : ["在 CrewVela 中打开两个 Agent 会话", "在其中一个会话里说出要联系的对象", "到对方会话查看消息和回复"]).map(step => <li key={step}>{step}</li>)}</ol>
      {loading ? <p role="status" className="communication-muted">正在读取项目成员…</p> : participants.length < 2 ? <p className="communication-empty">{scope === "project" ? projectId ? "当前项目不足两位已启用成员。请在协作中心核对成员与运行时配置。" : "先选择协作项目，再查看成员通信示例。" : "在 CrewVela 中打开两个受支持的 Agent 会话后，这里会生成对应示例。普通 Shell 和已停止的会话不计入。"}</p> : <>
        <div className="communication-participants" aria-label="示例对象">{[source, target].map((item, index) => item && <div key={item.id}><img src={RUNTIME_LOGOS[item.family as keyof typeof RUNTIME_LOGOS]} alt="" /><span><strong>{item.name}</strong><small>{index === 0 ? "发送方" : "接收方"} · {item.runtime}</small></span></div>)}</div>
        {participants.length >= 2 && <details className="communication-choose"><summary>更换示例对象</summary><label>发送方<select aria-label="示例发送方" value={source?.id ?? ""} onChange={event => { setSourceId(event.target.value); setTargetId(""); }}>{participants.map(item => <option key={item.id} value={item.id}>{item.name} · {item.runtime}</option>)}</select></label><label>接收方<select aria-label="示例接收方" value={target?.id ?? ""} onChange={event => setTargetId(event.target.value)}>{participants.filter(item => item.id !== source?.id).map(item => <option key={item.id} value={item.id}>{item.name} · {item.runtime}</option>)}</select></label></details>}
        <h3 className="communication-example-heading">试着对{source?.name}说</h3>
        <p className="communication-example">{example}</p>
        <button type="button" className="communication-copy" disabled={!example || copyBusy} onClick={() => void copy()}><CopyIcon size={17} />{copyBusy ? "复制中…" : "复制示例"}</button>
        {copyState && <p role={copyState.startsWith("复制失败") ? "alert" : "status"} className="communication-copy-state">{copyState}</p>}
      </>}
      <button type="button" className="communication-diagnostics" disabled={scope === "project" && Boolean(projectId) && !projects.some(project => project.id === projectId)} onClick={() => onDiagnostics(scope === "project" ? projectId || undefined : undefined, scope)}>{scope === "project" && !projectId ? "打开协作中心" : "打开通信诊断"}</button>
      <p className="communication-footnote">{scope === "project" ? "消息只发给指定成员。" : "对方忙碌时，消息会排队等待。"}已排队不代表已回复。{scope === "project" ? "请先启动成员任务或进入成员会话。" : "旧会话可能需要重新打开，以加载通信工具。"}</p>
    </div>
  </aside>;
}

export function CommunicationGuideButton({ projectId, scope, className }: { projectId?: string | null; scope?: CommunicationScope; className?: string }) {
  return <button type="button" className={className} data-communication-trigger title="会话通信" aria-label="会话通信" onClick={() => requestCommunicationGuide(scope || projectId ? { scope: scope ?? "project", projectId } : {})}><ChatCircleTextIcon size={18} aria-hidden="true" /><span>会话通信</span></button>;
}
