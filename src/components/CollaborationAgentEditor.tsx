import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { ImageSquare } from "@phosphor-icons/react";
import { useRuntimeAvailability } from "../runtimeAvailability";
import { useEffect, useId, useRef, useState } from "react";
import { COLLABORATION_RUNTIMES, isLegacyCollaborationRuntime, isRetiredRuntime } from "../runtimeTaxonomy";
import type { AgentPersona, AgentProfile, AgentRole, AgentRuntimeKind } from "../types";
import { AGENT_PERSONA_DESCRIPTIONS, AGENT_PERSONA_LABELS } from "../types";
import { RUNTIME_LABELS } from "../runtimeLogos";
import { AGENT_AVATARS, isImportedAgentAvatar } from "../agentAvatars";
import { AgentAvatar } from "./AgentAvatar";
import { AgentCard } from "./AgentCard";
import { memberModelSummary, type MemberModels, type MemberModelSelection } from "./ConversationMemberModels";
import { Modal } from "./ui/Modal";
import { useTranslation } from "../useInterfaceLanguage";
import { translate } from "../interfaceLanguage";
import { COLLABORATION_ROLE_LABELS } from "../collaborationRoles";
import { collaborationMemberTemplates, type CollaborationMemberTemplate } from "../collaborationMemberTemplates";

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
const ROLE_DESCRIPTIONS: Record<AgentRole, string> = {
  worker: "实现需求、修复问题并运行验证。",
  reviewer: "在托管中独立进行只读审查，给出问题和审查结论。",
  integrator: "汇总通过审查的改动，负责联调和交付验收。",
  orchestrator: "在团队托管中优先拟定任务、负责人和依赖。",
};
const IMPLEMENTATION_RESPONSIBILITIES = "实现需求并验证改动。先确认改动范围和验收标准，在授权的工作区内实现，任务要求隔离时使用独立工作树。运行相关测试并注明失败和未验证的部分。交付时说明改动、验证结果和剩余风险，把改动与验证记录交给审查成员。";
const implementationDescriptions = [IMPLEMENTATION_RESPONSIBILITIES, translate("en", IMPLEMENTATION_RESPONSIBILITIES)];
const includesImplementationResponsibilities = (description: string) => implementationDescriptions.some(text => description.includes(text));
const PERSONA_OPTIONS: AgentPersona[] = ["efficient", "concise", "rigorous", "innovative", "custom"];
const PERSONA_CUSTOM_MAX = 120;
const FOLLOW_RUNTIME: MemberModelSelection = { model: null, reasoning: null };

export function CollaborationAgentEditor({ agent, runtimeLocked, projectId, libraryScope = false, onClose, onSave }: {
  agent: AgentProfile | null;
  runtimeLocked: boolean;
  projectId?: string;
  libraryScope?: boolean;
  onClose: () => void;
  onSave: (draft: AgentDraft, chatModels?: MemberModelSelection) => Promise<void>;
  onPolicyChanged?: () => void;
}) {
  const t = useTranslation();
  const { ready, error: availabilityError, isRuntimeEnabled } = useRuntimeAvailability();
  const [draft, setDraft] = useState<AgentDraft>({ avatarId: agent?.avatarId ?? null, name: agent?.name ?? "", runtimeKind: agent?.runtimeKind ?? "claude_code", role: agent?.role ?? "worker", description: agent?.description ?? "", persona: agent?.persona ?? "efficient", personaCustom: agent?.personaCustom ?? "", model: agent?.model ?? null, enabled: isRetiredRuntime(agent?.runtimeKind) ? false : agent?.enabled ?? true });
  const [templateDefaults, setTemplateDefaults] = useState<Pick<AgentDraft, "name" | "description" | "role">>();
  const templatePicker = useRef<HTMLDetailsElement>(null);
  const [saving, setSaving] = useState(false);
  const [importing, setImporting] = useState(false);
  const [customAvatarId, setCustomAvatarId] = useState(() => isImportedAgentAvatar(agent?.avatarId) ? agent.avatarId : null);
  const [avatarError, setAvatarError] = useState("");
  const saveInFlight = useRef(false);
  const importInFlight = useRef(false);
  const nameInputRef = useRef<HTMLInputElement>(null);
  const descriptionInput = useRef<HTMLTextAreaElement>(null);
  const descriptionId = useId();
  const [error, setError] = useState<string | null>(null);
  const hasDescription = !!draft.description.trim();
  const hasImplementationResponsibilities = includesImplementationResponsibilities(draft.description);
  const expectedRole = templateDefaults?.role ?? agent?.role ?? "worker";
  const showRoleReview = hasDescription && (draft.role !== expectedRole
    || (draft.role !== "worker" && hasImplementationResponsibilities));
  const applyTemplate = (template: CollaborationMemberTemplate) => {
    const next = { name: t(template.name), description: t(template.description), role: template.role };
    setDraft(current => ({ ...current,
      name: !current.name.trim() || current.name === templateDefaults?.name ? next.name : current.name,
      description: !current.description.trim() || current.description === templateDefaults?.description ? next.description : current.description,
      role: current.role === expectedRole ? next.role : current.role,
    }));
    setTemplateDefaults(next);
    if (templatePicker.current) templatePicker.current.open = false;
    nameInputRef.current?.focus();
  };
  const addImplementationResponsibilities = () => {
    setDraft(current => {
      if (current.role !== "worker" || includesImplementationResponsibilities(current.description)) return current;
      const description = current.description.trimEnd();
      const preset = t(IMPLEMENTATION_RESPONSIBILITIES);
      return { ...current, description: description ? `${description}\n\n${preset}` : preset };
    });
    descriptionInput.current?.focus();
  };
  const [catalog, setCatalog] = useState<MemberModels>();
  const [selection, setSelection] = useState(FOLLOW_RUNTIME);
  const [modelChanged, setModelChanged] = useState(false);
  const [modelBusy, setModelBusy] = useState(false);
  const [modelError, setModelError] = useState("");
  const modelRequest = useRef(0);
  const runtimeEnabled = ready && isRuntimeEnabled(draft.runtimeKind);
  const readModels = async (next?: MemberModelSelection) => {
    const ticket = ++modelRequest.current;
    setModelBusy(true);
    setModelError("");
    try {
      const data = !next && agent?.runtimeKind === draft.runtimeKind
        ? await invoke<MemberModels>("ma_conversation_member_models", { projectId, agentId: agent.id })
        : await invoke<MemberModels>("ma_conversation_draft_model_options", { runtimeKind: draft.runtimeKind, model: next?.model ?? null });
      if (ticket !== modelRequest.current) return;
      if (!data?.selection || !Array.isArray(data.model) || !Array.isArray(data.reasoning)) {
        throw new Error(t("无法读取模型选项"));
      }
      setCatalog(data);
      if (!next) setSelection(data.selection);
    } catch (reason) {
      if (ticket === modelRequest.current) setModelError(String(reason));
    } finally {
      if (ticket === modelRequest.current) setModelBusy(false);
    }
  };
  useEffect(() => {
    setCatalog(undefined);
    setSelection(FOLLOW_RUNTIME);
    setModelChanged(false);
    setModelBusy(false);
    setModelError("");
    if (projectId && runtimeEnabled) void readModels();
    return () => { modelRequest.current++; };
  }, [projectId, agent?.id, draft.runtimeKind, runtimeEnabled]);
  const changeModel = (model: string | null) => {
    const next = { model, reasoning: null };
    setSelection(next);
    setModelChanged(true);
    void readModels(next);
  };
  const modelPreview = catalog && memberModelSummary({ ...catalog, selection });
  const modelsPending = modelChanged && (modelBusy || (!!modelError && !!(selection.model || selection.reasoning)));
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
    if (saveInFlight.current || importInFlight.current || modelsPending) return;
    if (isRetiredRuntime(draft.runtimeKind) && draft.enabled) { setError("请选择可用 Runtime 后再启用成员"); return; }
    if (!AVAILABLE.includes(draft.runtimeKind) && !keepingLegacy) { setError("新增或切换协作成员请选择 CLI 执行引擎"); return; }
    if (!ready || ((!agent || draft.runtimeKind !== agent.runtimeKind) && !isRuntimeEnabled(draft.runtimeKind))) { setError(availabilityError ?? "请选择已启用的 Runtime"); return; }
    if (!draft.name.trim()) { setError("请填写成员名称"); return; }
    saveInFlight.current = true;
    setSaving(true); setError(null);
    try {
      await onSave({ ...draft, name: draft.name.trim(), description: draft.description.trim(), personaCustom: draft.personaCustom.trim() }, modelChanged ? selection : undefined);
      onClose();
    }
    catch (reason) { setError(String(reason)); }
    finally { saveInFlight.current = false; setSaving(false); }
  };
  // Keep the editor outside the settings scroll/paint layers (notably WKWebView).
  const title = libraryScope ? "编辑全局成员" : agent ? "编辑团队成员" : "新增团队成员";
  const avatarBusy = saving || importing;
  return createPortal(<Modal initialFocusRef={nameInputRef} overlayClassName="dialog-overlay collab-agent-editor-overlay" className="collab-agent-editor" ariaLabel={title} onClose={() => { if (!saveInFlight.current && !importInFlight.current) onClose(); }}>
    <header><h3>{title}</h3><button type="button" aria-label="关闭成员编辑" disabled={avatarBusy} onClick={onClose}>×</button></header>
    <p>{libraryScope ? "维护多个项目共用的成员配置。" : "按项目分工设置成员，同一个 Runtime 可以创建多个成员。"}</p>
    {!agent && projectId && <details className="collab-agent-templates" ref={templatePicker}>
      <summary>{t("从模板创建成员")}{templateDefaults && <small>{templateDefaults.name}</small>}</summary>
      <div className="collab-agent-template-options">
        {collaborationMemberTemplates.map(template => <button key={template.id} type="button" className="btn secondary"
          aria-label={t(template.name)} aria-pressed={templateDefaults?.name === t(template.name)}
          disabled={avatarBusy} onClick={() => applyTemplate(template)}>
          <strong>{t(template.name)}</strong><small>{t(template.summary)}</small>
        </button>)}
      </div>
      <small>{t("套用模板时会保留手动填写的名称和职责。")}</small>
    </details>}
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
        <label>Runtime<select aria-label="Runtime" value={draft.runtimeKind} disabled={saving || runtimeLocked || !ready} onChange={(e) => setDraft({ ...draft, runtimeKind: e.target.value as AgentRuntimeKind, model: agent && e.target.value === agent.runtimeKind ? agent.model : null })}>{AVAILABLE.filter(isRuntimeEnabled).map((kind) => <option value={kind} key={kind}>{kind === "deepseek_harness" ? "DeepSeek Harness CLI（dsh）" : RUNTIME_LABELS[kind]}</option>)}{agent && isLegacyCollaborationRuntime(agent.runtimeKind) && isRuntimeEnabled(agent.runtimeKind) && <option value={agent.runtimeKind}>{RUNTIME_LABELS[agent.runtimeKind]}（已有成员兼容）</option>}{!isRuntimeEnabled(draft.runtimeKind) && <option value={draft.runtimeKind} disabled>{RUNTIME_LABELS[draft.runtimeKind]}（不可用）</option>}</select></label>
        {projectId && <div className="collab-agent-model-settings" aria-busy={modelBusy}>
          <div className="collab-agent-editor-pair">
            {(["model", "reasoning"] as const).map(field => {
              const options = catalog?.[field] ?? [];
              const value = selection[field] ?? "";
              return <label key={field}>
                {t(field === "model" ? "模型" : "推理级别")}
                <select aria-label={t(field === "model" ? "模型" : "推理级别")} value={value}
                  disabled={saving || !runtimeEnabled || modelBusy || (!options.length && !value) || (field === "reasoning" && !!modelError)}
                  onChange={event => {
                    const next = event.target.value || null;
                    if (field === "model") changeModel(next);
                    else { setSelection(current => ({ ...current, reasoning: next })); setModelChanged(true); }
                  }}>
                  <option value="">{t("跟随 Runtime")}</option>
                  {value && !options.some(option => option.value === value) && <option value={value} disabled>{value} · {t("已不可用")}</option>}
                  {options.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
                </select>
                {catalog && !options.length && !modelBusy && !modelError && <small>{t("Runtime 未提供此选项")}</small>}
              </label>;
            })}
          </div>
          <small>{t("用于团队聊天的原生 Runtime，从下一次派发起生效。")}</small>
          {modelBusy && <small role="status">{t("正在读取可用选项…")}</small>}
          {modelError && <div className="collab-agent-model-error">
            <small role="status">{modelError}</small>
            <button type="button" className="btn secondary" disabled={saving} onClick={() => void readModels(modelChanged ? selection : undefined)}>{t("重试")}</button>
          </div>}
        </div>}
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
        <div className="collab-agent-responsibilities">
          <div className="collab-agent-responsibilities-heading">
            <label htmlFor={descriptionId}>{t("职责定位")}</label>
            {draft.role === "worker" && !hasImplementationResponsibilities && <button type="button" className="conversation-text-button"
              aria-controls={descriptionId} disabled={saving} onClick={addImplementationResponsibilities}>
              {t(hasDescription ? "追加代码实现职责" : "填入代码实现职责")}
            </button>}
          </div>
          <textarea ref={descriptionInput} id={descriptionId} rows={4} value={draft.description} disabled={saving}
            aria-describedby={showRoleReview ? `${descriptionId}-role-review` : undefined}
            onChange={event => { const description = event.target.value; setDraft(current => ({ ...current, description })); }}
            placeholder={t("例如：擅长后端接口和数据存储，优先关注接口兼容性与数据一致性。")} />
          {showRoleReview && <small id={`${descriptionId}-role-review`} role="status">{t("请核对当前托管角色与职责定位是否一致。")}</small>}
        </div>
        <details className="collab-agent-managed-settings">
          <summary>{t("托管设置")} <small>{t(COLLABORATION_ROLE_LABELS[draft.role])}</small></summary>
          <div>
            <label>{t("托管角色")}
              <select aria-label={t("托管角色")} value={draft.role} disabled={saving}
                onChange={event => setDraft(current => ({ ...current, role: event.target.value as AgentRole }))}>
                {Object.entries(COLLABORATION_ROLE_LABELS).map(([value, label]) => <option key={value} value={value}>{t(label)}</option>)}
              </select>
            </label>
            <small>{t(ROLE_DESCRIPTIONS[draft.role])}</small>
            <small>{t("角色用于推荐接手成员和检查托管分工。实际工具权限与审批由本轮授权和 Runtime 设置决定。")}</small>
          </div>
        </details>
        {agent && <label className="collab-agent-enabled"><input type="checkbox" checked={draft.enabled} disabled={saving || isRetiredRuntime(draft.runtimeKind)} onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })} />启用成员（停用后不再领取新任务）</label>}
        <small>{libraryScope ? "修改会用于关联项目的后续任务。单独编辑过的项目成员使用该项目的配置。" : "配置用于后续任务；执行中的任务须完成或停止后再编辑。切换 Runtime 会停止空闲旧终端并保留只读归档，下次使用新会话；旧收件箱待发消息标为失败，需要重新发送。人格随每次派单注入提示词，保存即生效。"}</small>
        {error && <div className="collab-agent-error" role="alert">{error}</div>}
      </div>
      <aside className="collab-agent-editor-preview">
        <span className="collab-agent-editor-preview-label">名片预览</span>
        <AgentCard agent={projectId && catalog?.supported ? { ...draft, model: modelPreview?.model ?? t("跟随 Runtime") } : draft}
          reasoning={modelPreview?.reasoning} />
      </aside>
    </div>
    <footer><button className="btn secondary" disabled={avatarBusy} onClick={onClose}>取消</button><button className="btn primary" disabled={avatarBusy || modelsPending} onClick={() => void save()}>{saving ? "保存中…" : "保存成员"}</button></footer>
  </Modal>, document.body);
}
