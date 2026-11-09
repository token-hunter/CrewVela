import { createPortal } from "react-dom";
import { useRuntimeAvailability } from "../runtimeAvailability";
import { useRef, useState } from "react";
import { COLLABORATION_RUNTIMES, isRetiredRuntime } from "../runtimeTaxonomy";
import type { AgentPersona, AgentProfile, AgentRole, AgentRuntimeKind } from "../types";
import { AGENT_PERSONA_DESCRIPTIONS, AGENT_PERSONA_LABELS } from "../types";
import { RUNTIME_LABELS } from "../runtimeLogos";
import { AGENT_AVATARS } from "../agentAvatars";
import { AgentAvatar } from "./AgentAvatar";
import { AgentCard } from "./AgentCard";
import { Modal } from "./ui/Modal";

export interface AgentDraft {
  avatarId: string | null;
  name: string;
  runtimeKind: AgentRuntimeKind;
  role: AgentRole;
  description: string;
  persona: AgentPersona;
  personaCustom: string;
  model: string | null;
  enabled: boolean;
}
const AVAILABLE = COLLABORATION_RUNTIMES;
const ROLES: Record<AgentRole, string> = { orchestrator: "协调", worker: "执行", reviewer: "审查", integrator: "集成" };
const PERSONA_OPTIONS: AgentPersona[] = ["efficient", "concise", "rigorous", "innovative", "custom"];
const PERSONA_CUSTOM_MAX = 120;

export function CollaborationAgentEditor({ agent, runtimeLocked, onClose, onSave }: {
  agent: AgentProfile | null;
  runtimeLocked: boolean;
  onClose: () => void;
  onSave: (draft: AgentDraft) => Promise<void>;
}) {
  const { ready, error: availabilityError, isRuntimeEnabled } = useRuntimeAvailability();
  const [draft, setDraft] = useState<AgentDraft>({ avatarId: agent?.avatarId ?? null, name: agent?.name ?? "", runtimeKind: agent?.runtimeKind ?? "claude_code", role: agent?.role ?? "worker", description: agent?.description ?? "", persona: agent?.persona ?? "efficient", personaCustom: agent?.personaCustom ?? "", model: agent?.model ?? null, enabled: isRetiredRuntime(agent?.runtimeKind) ? false : agent?.enabled ?? true });
  const [saving, setSaving] = useState(false);
  const saveInFlight = useRef(false);
  const nameInputRef = useRef<HTMLInputElement>(null);
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    if (saveInFlight.current) return;
    if (isRetiredRuntime(draft.runtimeKind) && draft.enabled) { setError("请选择可用 Runtime 后再启用成员"); return; }
    if (!ready || ((!agent || draft.runtimeKind !== agent.runtimeKind) && !isRuntimeEnabled(draft.runtimeKind))) { setError(availabilityError ?? "请选择已启用的 Runtime"); return; }
    if (!draft.name.trim()) { setError("请填写成员名称"); return; }
    saveInFlight.current = true;
    setSaving(true); setError(null);
    try { await onSave({ ...draft, name: draft.name.trim(), description: draft.description.trim(), personaCustom: draft.personaCustom.trim() }); onClose(); }
    catch (reason) { setError(String(reason)); }
    finally { saveInFlight.current = false; setSaving(false); }
  };
  // Keep the editor outside the settings scroll/paint layers (notably WKWebView).
  return createPortal(<Modal initialFocusRef={nameInputRef} overlayClassName="dialog-overlay collab-agent-editor-overlay" className="collab-agent-editor" ariaLabel={agent ? "编辑团队成员" : "新增团队成员"} onClose={() => { if (!saving) onClose(); }}>
    <header><h3>{agent ? "编辑团队成员" : "新增团队成员"}</h3><button type="button" aria-label="关闭成员编辑" disabled={saving} onClick={onClose}>×</button></header>
    <p>按项目分工设置成员，同一个 Runtime 可以创建多个成员。</p>
    <div className="collab-agent-editor-body">
      <div className="collab-agent-editor-form">
        <fieldset className="collab-avatar-picker"><legend>成员头像</legend>
          <button type="button" className={draft.avatarId === null ? "selected" : ""} aria-pressed={draft.avatarId === null} disabled={saving} onClick={() => setDraft({ ...draft, avatarId: null })}><AgentAvatar runtimeKind={draft.runtimeKind} decorative /><span>Runtime 默认</span></button>
          <div>{AGENT_AVATARS.map((avatar) => <button type="button" key={avatar.id} className={draft.avatarId === avatar.id ? "selected" : ""} aria-pressed={draft.avatarId === avatar.id} disabled={saving} onClick={() => setDraft({ ...draft, avatarId: avatar.id })}><AgentAvatar avatarId={avatar.id} runtimeKind={draft.runtimeKind} decorative /><span>{avatar.name}</span></button>)}
        </div><small>头像只标识成员，Runtime 与职责可以独立调整。</small></fieldset>
        <label>成员名称<input ref={nameInputRef} value={draft.name} maxLength={100} disabled={saving} onChange={(e) => setDraft({ ...draft, name: e.target.value })} placeholder="例如：Codex 后端工程师" /></label>
        <div className="collab-agent-editor-pair"><label>Runtime<select aria-label="Runtime" value={draft.runtimeKind} disabled={saving || runtimeLocked || !ready} onChange={(e) => setDraft({ ...draft, runtimeKind: e.target.value as AgentRuntimeKind })}>{AVAILABLE.filter(isRuntimeEnabled).map((kind) => <option value={kind} key={kind}>{RUNTIME_LABELS[kind]}</option>)}{!isRuntimeEnabled(draft.runtimeKind) && <option value={draft.runtimeKind} disabled>{RUNTIME_LABELS[draft.runtimeKind]}（不可用）</option>}</select></label>
        <label>角色<select aria-label="角色" value={draft.role} disabled={saving} onChange={(e) => setDraft({ ...draft, role: e.target.value as AgentRole })}>{Object.entries(ROLES).map(([value,label]) => <option key={value} value={value}>{label}</option>)}</select></label></div>
        {runtimeLocked && <small>该成员有执行中的任务，等待空闲后可切换 Runtime。</small>}
        <div className="collab-agent-editor-pair">
          <label>人格<select aria-label="人格" value={draft.persona} disabled={saving} onChange={(e) => setDraft({ ...draft, persona: e.target.value as AgentPersona })}>{PERSONA_OPTIONS.map((value) => <option key={value} value={value}>{AGENT_PERSONA_LABELS[value]}</option>)}</select></label>
          <label>模型<input value={draft.model ?? ""} disabled={saving} onChange={(e) => setDraft({ ...draft, model: e.target.value })} placeholder="留空使用 Runtime 默认模型" /></label>
        </div>
        <small className="collab-persona-hint">{AGENT_PERSONA_DESCRIPTIONS[draft.persona]}{draft.persona !== "custom" ? "。人格只约束沟通风格，职责仍由下方职责描述决定。" : ""}</small>
        {draft.persona === "custom" && (
          <label>自定义工作风格<textarea rows={2} value={draft.personaCustom} maxLength={PERSONA_CUSTOM_MAX} disabled={saving} onChange={(e) => setDraft({ ...draft, personaCustom: e.target.value.slice(0, PERSONA_CUSTOM_MAX) })} placeholder="例如：先复盘历史任务再动手，遇到歧义先提问，完成后主动给出验证步骤" /><span className="collab-persona-count">{draft.personaCustom.length} / {PERSONA_CUSTOM_MAX}</span></label>
        )}
        <label>职责与工作边界<textarea rows={4} value={draft.description} disabled={saving} onChange={(e) => setDraft({ ...draft, description: e.target.value })} placeholder="例如：负责后端接口和数据存储，不修改前端文件；与前端成员通过 CrewVela 会话通信协调接口。" /></label>
        {agent && <label className="collab-agent-enabled"><input type="checkbox" checked={draft.enabled} disabled={saving || isRetiredRuntime(draft.runtimeKind)} onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })} />启用成员（停用后不再领取新任务）</label>}
        <small>配置用于后续任务；执行中的任务须完成或停止后再编辑。切换 Runtime 会停止空闲旧终端并保留只读归档，下次使用新会话；旧收件箱待发消息标为失败，需要重新发送。模型变更在下次启动时生效。人格随每次派单注入提示词，保存即生效。</small>
        {error && <div className="collab-agent-error" role="alert">{error}</div>}
      </div>
      <aside className="collab-agent-editor-preview">
        <span className="collab-agent-editor-preview-label">名片预览</span>
        <AgentCard agent={draft} />
      </aside>
    </div>
    <footer><button className="btn secondary" disabled={saving} onClick={onClose}>取消</button><button className="btn primary" disabled={saving} onClick={() => void save()}>{saving ? "保存中…" : "保存成员"}</button></footer>
  </Modal>, document.body);
}
