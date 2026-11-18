import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { PencilSimpleIcon } from "@phosphor-icons/react/dist/csr/PencilSimple";
import { COLLABORATION_RUNTIMES } from "../runtimeTaxonomy";
import { RUNTIME_LABELS } from "../runtimeLogos";
import type { AgentProfile, AgentRole, MultiAgentProject, MultiAgentSnapshot } from "../types";
import { AgentAvatar } from "./AgentAvatar";
import { CollaborationAgentEditor, type AgentDraft } from "./CollaborationAgentEditor";
import { Modal } from "./ui/Modal";

const ROLE: Record<AgentRole, string> = { orchestrator: "协调", worker: "执行", reviewer: "审查", integrator: "集成" };
type LibraryProfile = Omit<AgentDraft, "model">;
type LibrarySnapshot = { members: { id: string; profile: LibraryProfile; isDefault: boolean; runtimeLocked: boolean }[]; autoApply: boolean };
type Application = { added: number; skipped: string[] };

function ImportTeamDialog({ projects, project, onSave, onClose }: {
  projects: MultiAgentProject[];
  project?: MultiAgentProject;
  onSave: (sourceProjectId: string, agentIds: string[], applyToProjectId?: string) => Promise<void>;
  onClose: () => void;
}) {
  const [sourceId, setSourceId] = useState(projects.find(item => item.id !== project?.id)?.id ?? project?.id ?? "");
  const [source, setSource] = useState<{ id: string; agents: AgentProfile[] }>();
  const [selected, setSelected] = useState<string[]>([]);
  const [alsoApply, setAlsoApply] = useState(true);
  const [loadError, setLoadError] = useState("");
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  const [saving, setSaving] = useState(false);
  const inFlight = useRef(false);
  const sourceControl = useRef<HTMLSelectElement>(null);

  useEffect(() => {
    if (!sourceId) return;
    let current = true;
    invoke<MultiAgentSnapshot>("ma_snapshot", { projectId: sourceId }).then(snapshot => {
      if (!current) return;
      const agents = snapshot.agents.filter(agent => agent.enabled && !agent.removedAt && COLLABORATION_RUNTIMES.includes(agent.runtimeKind));
      setSource({ id: sourceId, agents });
      setSelected(agents.map(agent => agent.id));
    }).catch(reason => { if (current) setLoadError(`读取项目成员失败：${String(reason)}`); });
    return () => { current = false; };
  }, [sourceId, attempt]);

  const save = async () => {
    if (inFlight.current || source?.id !== sourceId || !selected.length) return;
    inFlight.current = true;
    setSaving(true);
    setError("");
    try {
      await onSave(sourceId, selected, alsoApply && sourceId !== project?.id ? project?.id : undefined);
      onClose();
    } catch (reason) { setError(String(reason)); }
    finally { inFlight.current = false; setSaving(false); }
  };

  return createPortal(<Modal ariaLabel="从现有项目导入成员" initialFocusRef={sourceControl}
    className="collab-agent-editor collab-library-import" overlayClassName="dialog-overlay collab-agent-editor-overlay"
    onClose={() => { if (!inFlight.current) onClose(); }}>
    <header><h3>从现有项目导入成员</h3><button type="button" aria-label="关闭导入" disabled={saving} onClick={onClose}>×</button></header>
    <label>来源项目<select ref={sourceControl} value={sourceId} disabled={saving} onChange={event => {
      setSourceId(event.target.value); setSource(undefined); setSelected([]); setError(""); setLoadError("");
    }}>{projects.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label>
    {loadError ? <div role="alert">{loadError} <button type="button" className="btn secondary" onClick={() => { setLoadError(""); setAttempt(value => value + 1); }}>重新读取</button></div>
      : source?.id !== sourceId ? <p role="status">正在读取项目成员…</p>
      : !source.agents.length ? <p role="status">该项目暂无可导入的成员。</p>
      : <div className="collab-library-import-list" role="group" aria-label="待导入成员">{source.agents.map(agent => <label key={agent.id}>
        <input type="checkbox" checked={selected.includes(agent.id)} disabled={saving} onChange={event => {
          const checked = event.target.checked;
          setSelected(current => checked ? [...current, agent.id] : current.filter(id => id !== agent.id));
        }} />
        <AgentAvatar className="collab-library-avatar" avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative />
        <strong>{agent.name}</strong><small>{RUNTIME_LABELS[agent.runtimeKind]} · {ROLE[agent.role]}</small>
      </label>)}</div>}
    {project && sourceId !== project.id && <label className="collab-agent-enabled"><input type="checkbox" checked={alsoApply} disabled={saving}
      onChange={event => setAlsoApply(event.target.checked)} />同时应用到「{project.name}」</label>}
    {error && <p role="alert" className="collab-agent-error">{error}</p>}
    <footer><button type="button" className="btn secondary" disabled={saving} onClick={onClose}>取消</button>
      <button type="button" className="btn primary" disabled={saving || source?.id !== sourceId || !selected.length} onClick={() => void save()}>{saving ? "正在保存…" : "设为默认团队"}</button></footer>
  </Modal>, document.body);
}

export function CollaborationMemberLibrary({ projects, project, onProjectChanged }: {
  projects: MultiAgentProject[];
  project?: MultiAgentProject;
  onProjectChanged: () => void;
}) {
  const [snapshot, setSnapshot] = useState<LibrarySnapshot>();
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [importing, setImporting] = useState(false);
  const [editing, setEditing] = useState<LibrarySnapshot["members"][number]>();

  const reload = async () => {
    try { setSnapshot(await invoke<LibrarySnapshot>("ma_member_library")); setError(""); }
    catch (reason) { setError(`读取全局成员库失败：${String(reason)}`); }
  };
  useEffect(() => { void reload(); }, []);

  const apply = async (projectId: string, name: string) => {
    setBusy(true); setError(""); setNotice("");
    try {
      const result = await invoke<Application>("ma_member_library_apply", { projectId });
      setNotice(`已为「${name}」添加 ${result.added} 位成员${result.skipped.length ? `，跳过 ${result.skipped.join("、")}` : ""}。`);
      onProjectChanged();
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  };

  const updateAutoApply = async (enabled: boolean) => {
    setBusy(true); setError("");
    try {
      await invoke("ma_member_library_auto_apply", { enabled });
      setSnapshot(current => current ? { ...current, autoApply: enabled } : current);
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  };

  const defaultCount = snapshot?.members.filter(member => member.isDefault && member.profile.enabled).length ?? 0;
  const locked = busy || importing || !!editing;
  return <section className="collab-member-library" aria-label="全局成员库">
    <header><div><h3>全局成员库</h3><p>成员配置可在多个项目中使用</p></div>
      <button className="btn secondary" type="button" disabled={locked || !snapshot || !projects.length} onClick={() => setImporting(true)}>从现有项目导入</button></header>
    {error && <p role="alert">{error} <button className="btn secondary" type="button" onClick={() => void reload()}>重新读取</button></p>}
    {!snapshot && !error && <p role="status">正在读取成员库…</p>}
    {snapshot && <>
      {snapshot.members.length ? <div className="collab-library-table-wrap"><table className="collab-library-table">
        <thead><tr><th scope="col">成员</th><th scope="col">Runtime</th><th scope="col">角色</th><th scope="col" aria-label="操作" /></tr></thead>
        <tbody>{snapshot.members.map(member => <tr key={member.id}>
          <td><span className="collab-library-member"><AgentAvatar className="collab-library-avatar" avatarId={member.profile.avatarId} runtimeKind={member.profile.runtimeKind} decorative />
            <strong>{member.profile.name}</strong>{!member.profile.enabled && <small>已停用</small>}</span></td>
          <td>{RUNTIME_LABELS[member.profile.runtimeKind]}</td><td><span className="collab-tag role">{ROLE[member.profile.role]}</span></td>
          <td><button className="btn secondary" type="button" disabled={locked} aria-label={`编辑 ${member.profile.name}`} onClick={() => setEditing(member)}><PencilSimpleIcon size={15} />编辑</button></td>
        </tr>)}</tbody>
      </table></div> : <p className="collab-library-empty">从现有项目导入成员，即可建立默认团队。</p>}
      <footer><strong>默认团队 · {defaultCount} 位成员</strong>
        <label><input type="checkbox" checked={snapshot.autoApply} disabled={locked} onChange={event => void updateAutoApply(event.target.checked)} />新项目自动使用</label>
        <button type="button" className="btn primary" disabled={locked || !project || !defaultCount} onClick={() => project && void apply(project.id, project.name)}>应用到当前项目</button>
      </footer>
    </>}
    {notice && <p role="status">{notice}</p>}
    {importing && <ImportTeamDialog projects={projects} project={project} onClose={() => setImporting(false)} onSave={async (sourceProjectId, agentIds, applyToProjectId) => {
      const result = await invoke<Application>("ma_member_library_import", { input: { sourceProjectId, agentIds, applyToProjectId } });
      setNotice(applyToProjectId && project
        ? `已保存默认团队，为「${project.name}」添加 ${result.added} 位成员${result.skipped.length ? `，跳过 ${result.skipped.join("、")}` : ""}。`
        : "已保存默认团队。");
      onProjectChanged();
      await reload();
    }} />}
    {editing && <CollaborationAgentEditor libraryScope runtimeLocked={editing.runtimeLocked} onClose={() => setEditing(undefined)}
      agent={{ ...editing.profile, id: editing.id, projectId: "", model: null, maxConcurrency: 1, permissionProfile: "default", capabilities: [], createdAt: 0, updatedAt: 0 }}
      onSave={async profile => {
        await invoke("ma_member_library_update", { memberId: editing.id, profile });
        onProjectChanged();
        await reload();
      }} />}
  </section>;
}
