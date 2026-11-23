import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { ImageSquare } from "@phosphor-icons/react";
import { useRuntimeAvailability } from "../runtimeAvailability";
import { useRef, useState } from "react";
import { COLLABORATION_RUNTIMES, isLegacyCollaborationRuntime, isRetiredRuntime } from "../runtimeTaxonomy";
import type { AgentPersona, AgentProfile, AgentRole, AgentRuntimeKind } from "../types";
import { AGENT_PERSONA_DESCRIPTIONS, AGENT_PERSONA_LABELS } from "../types";
import { RUNTIME_LABELS } from "../runtimeLogos";
import { AGENT_AVATARS, isImportedAgentAvatar } from "../agentAvatars";
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

export function CollaborationAgentEditor({ agent, runtimeLocked, libraryScope = false, onClose, onSave }: {
  agent: AgentProfile | null;
  runtimeLocked: boolean;
  libraryScope?: boolean;
  onClose: () => void;
  onSave: (draft: AgentDraft) => Promise<void>;
  onPolicyChanged?: () => void;
}) {
  const { ready, error: availabilityError, isRuntimeEnabled } = useRuntimeAvailability();
  const [draft, setDraft] = useState<AgentDraft>({ avatarId: agent?.avatarId ?? null, name: agent?.name ?? "", runtimeKind: agent?.runtimeKind ?? "claude_code", role: agent?.role ?? "worker", description: agent?.description ?? "", persona: agent?.persona ?? "efficient", personaCustom: agent?.personaCustom ?? "", model: agent?.model ?? null, enabled: isRetiredRuntime(agent?.runtimeKind) ? false : agent?.enabled ?? true });
  const [saving, setSaving] = useState(false);
  const [importing, setImporting] = useState(false);
  const [customAvatarId, setCustomAvatarId] = useState(() => isImportedAgentAvatar(agent?.avatarId) ? agent.avatarId : null);
  const [avatarError, setAvatarError] = useState("");
  const saveInFlight = useRef(false);
  const importInFlight = useRef(false);
  const nameInputRef = useRef<HTMLInputElement>(null);
  const [error, setError] = useState<string | null>(null);
  const keepingLegacy = !!agent && agent.runtimeKind === draft.runtimeKind && isLegacyCollaborationRuntime(draft.runtimeKind);
  const selectAvatar = (avatarId: string | null) => {
    setAvatarError("");
    setDraft(current => ({ ...current, avatarId }));
  };
  const importAvatar = async () => {
    if (importInFlight.current || saveInFlight.current) return;
    importInFlight.current = true;
    setImporting(true);
    setAvatarError("");
    try {
      const path = await open({ title: "导入头像", directory: false, multiple: false,
        filters: [{ name: "头像图片", extensions: ["png", "jpg", "jpeg", "webp"] }] });
      if (!path) return;
      const avatarId = await invoke<string>("ma_import_agent_avatar", { path });
      setCustomAvatarId(avatarId);
      selectAvatar(avatarId);
    } catch (reason) {
      setAvatarError(`导入头像失败：${String(reason)}`);
    } finally {
      importInFlight.current = false;
      setImporting(false);
    }
  };
  const save = async () => {
    if (saveInFlight.current || importInFlight.current) return;
    if (isRetiredRuntime(draft.runtimeKind) && draft.enabled) { setError("请选择可用 Runtime 后再启用成员"); return; }
    if (!AVAILABLE.includes(draft.runtimeKind) && !keepingLegacy) { setError("新增或切换协作成员请选择 CLI 执行引擎"); return; }
    if (!ready || ((!agent || draft.runtimeKind !== agent.runtimeKind) && !isRuntimeEnabled(draft.runtimeKind))) { setError(availabilityError ?? "请选择已启用的 Runtime"); return; }
    if (!draft.name.trim()) { setError("请填写成员名称"); return; }
    saveInFlight.current = true;
    setSaving(true); setError(null);
    try { await onSave({ ...draft, name: draft.name.trim(), description: draft.description.trim(), personaCustom: draft.personaCustom.trim() }); onClose(); }
    catch (reason) { setError(String(reason)); }
    finally { saveInFlight.current = false; setSaving(false); }
  };
  // Keep the editor outside the settings scroll/paint layers (notably WKWebView).
  const title = libraryScope ? "编辑全局成员" : agent ? "编辑团队成员" : "新增团队成员";
  const avatarBusy = saving || importing;
  return createPortal(<Modal initialFocusRef={nameInputRef} overlayClassName="dialog-overlay collab-agent-editor-overlay" className="collab-agent-editor" ariaLabel={title} onClose={() => { if (!saveInFlight.current && !importInFlight.current) onClose(); }}>
    <header><h3>{title}</h3><button type="button" aria-label="关闭成员编辑" disabled={avatarBusy} onClick={onClose}>×</button></header>
    <p>{libraryScope ? "维护多个项目共用的成员配置。" : "按项目分工设置成员，同一个 Runtime 可以创建多个成员。"}</p>
    <div className="collab-agent-editor-body">
      <div className="collab-agent-editor-form">
        <fieldset className="collab-avatar-picker"><legend>成员头像</legend>
          <div className="collab-avatar-top">
            <button type="button" className={draft.avatarId === null ? "selected" : ""} aria-pressed={draft.avatarId === null} disabled={avatarBusy} onClick={() => selectAvatar(null)}><AgentAvatar runtimeKind={draft.runtimeKind} decorative /><span>Runtime 默认</span></button>
            {customAvatarId && <button type="button" className={draft.avatarId === customAvatarId ? "selected" : ""} aria-pressed={draft.avatarId === customAvatarId} disabled={avatarBusy}
              onClick={() => selectAvatar(customAvatarId)}><AgentAvatar avatarId={customAvatarId} runtimeKind={draft.runtimeKind} decorative /><span>自定义头像</span></button>}
            <button type="button" disabled={avatarBusy} title="PNG、JPG、WebP · 最大 10 MB" onClick={() => void importAvatar()}>
              <ImageSquare size={18} aria-hidden="true" /><span>{importing ? "正在导入…" : "导入头像"}</span>
            </button>
          </div>
          <div className="collab-avatar-grid">{AGENT_AVATARS.map((avatar) => <button type="button" key={avatar.id} className={draft.avatarId === avatar.id ? "selected" : ""} aria-pressed={draft.avatarId === avatar.id} disabled={avatarBusy} onClick={() => selectAvatar(avatar.id)}><AgentAvatar avatarId={avatar.id} runtimeKind={draft.runtimeKind} decorative /><span>{avatar.name}</span></button>)}</div>
          {avatarError && <div className="collab-agent-error" role="alert">{avatarError}</div>}
          <small>头像只标识成员，Runtime 与职责可以独立调整。</small>
        </fieldset>
        <label>成员名称<input ref={nameInputRef} value={draft.name} maxLength={100} disabled={saving} onChange={(e) => setDraft({ ...draft, name: e.target.value })} placeholder="例如：Codex 后端工程师" /></label>
        <div className="collab-agent-editor-pair"><label>Runtime<select aria-label="Runtime" value={draft.runtimeKind} disabled={saving || runtimeLocked || !ready} onChange={(e) => setDraft({ ...draft, runtimeKind: e.target.value as AgentRuntimeKind, model: agent && e.target.value === agent.runtimeKind ? agent.model : null })}>{AVAILABLE.filter(isRuntimeEnabled).map((kind) => <option value={kind} key={kind}>{kind === "deepseek_harness" ? "DeepSeek Harness CLI（dsh）" : RUNTIME_LABELS[kind]}</option>)}{agent && isLegacyCollaborationRuntime(agent.runtimeKind) && isRuntimeEnabled(agent.runtimeKind) && <option value={agent.runtimeKind}>{RUNTIME_LABELS[agent.runtimeKind]}（已有成员兼容）</option>}{!isRuntimeEnabled(draft.runtimeKind) && <option value={draft.runtimeKind} disabled>{RUNTIME_LABELS[draft.runtimeKind]}（不可用）</option>}</select></label>
        <label>角色<select aria-label="角色" value={draft.role} disabled={saving} onChange={(e) => setDraft({ ...draft, role: e.target.value as AgentRole })}>{Object.entries(ROLES).map(([value,label]) => <option key={value} value={value}>{label}</option>)}</select></label></div>
        {runtimeLocked && <small>{libraryScope ? "该成员已有关联项目会话，保留当前 Runtime。" : "该成员有执行中的任务，等待空闲后可切换 Runtime。"}</small>}
        {keepingLegacy && <small>保留该成员原有 ZCode Desktop 配置和执行记录。选择 CLI 并保存才会切换，不会自动替换账号或模型。</small>}
        {agent && isLegacyCollaborationRuntime(agent.runtimeKind) && !keepingLegacy && <small>保存后改用所选 CLI 的登录和默认模型；原有执行记录保留。</small>}
        {draft.runtimeKind === "deepseek_harness" && <small>通过本机 dsh 执行任务，沿用其登录与模型设置。</small>}
        <div className="collab-agent-editor-pair">
          <label>人格<select aria-label="人格" value={draft.persona} disabled={saving} onChange={(e) => setDraft({ ...draft, persona: e.target.value as AgentPersona })}>{PERSONA_OPTIONS.map((value) => <option key={value} value={value}>{AGENT_PERSONA_LABELS[value]}</option>)}</select></label>
        </div>
        <small className="collab-persona-hint">{AGENT_PERSONA_DESCRIPTIONS[draft.persona]}{draft.persona !== "custom" ? "。人格只约束沟通风格，职责仍由下方职责描述决定。" : ""}</small>
        {draft.persona === "custom" && (
          <label>自定义工作风格<textarea rows={2} value={draft.personaCustom} maxLength={PERSONA_CUSTOM_MAX} disabled={saving} onChange={(e) => setDraft({ ...draft, personaCustom: e.target.value.slice(0, PERSONA_CUSTOM_MAX) })} placeholder="例如：先复盘历史任务再动手，遇到歧义先提问，完成后主动给出验证步骤" /><span className="collab-persona-count">{draft.personaCustom.length} / {PERSONA_CUSTOM_MAX}</span></label>
        )}
        <label>职责定位<textarea aria-label="职责定位" rows={4} value={draft.description} disabled={saving} onChange={(e) => setDraft({ ...draft, description: e.target.value })} placeholder="例如：擅长后端接口和数据存储，优先关注接口兼容性与数据一致性。" /></label>
        <small>角色设定只说明专长、分工和沟通风格。工具、联网、Skill、MCP 与权限确认沿用 CLI 自身设置；当前消息中的任务要求优先。</small>
        {agent && <label className="collab-agent-enabled"><input type="checkbox" checked={draft.enabled} disabled={saving || isRetiredRuntime(draft.runtimeKind)} onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })} />启用成员（停用后不再领取新任务）</label>}
        <small>{libraryScope ? "修改会用于关联项目的后续任务。单独编辑过的项目成员使用该项目的配置。" : "配置用于后续任务；执行中的任务须完成或停止后再编辑。切换 Runtime 会停止空闲旧终端并保留只读归档，下次使用新会话；旧收件箱待发消息标为失败，需要重新发送。人格随每次派单注入提示词，保存即生效。"}</small>
        {error && <div className="collab-agent-error" role="alert">{error}</div>}
      </div>
      <aside className="collab-agent-editor-preview">
        <span className="collab-agent-editor-preview-label">名片预览</span>
        <AgentCard agent={draft} />
      </aside>
    </div>
    <footer><button className="btn secondary" disabled={avatarBusy} onClick={onClose}>取消</button><button className="btn primary" disabled={avatarBusy} onClick={() => void save()}>{saving ? "保存中…" : "保存成员"}</button></footer>
  </Modal>, document.body);
}
