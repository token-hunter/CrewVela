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
import { collaborationMemberTemplates, IMPLEMENTATION_RESPONSIBILITIES, type CollaborationMemberTemplate } from "../collaborationMemberTemplates";
import "../styles/collaboration-member-editor.css";

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
  const [saving, setSaving] = useState(false);
  const [importing, setImporting] = useState(false);
  const [customAvatarId, setCustomAvatarId] = useState(() => isImportedAgentAvatar(agent?.avatarId) ? agent.avatarId : null);
  const [avatarError, setAvatarError] = useState("");
  const saveInFlight = useRef(false);
  const importInFlight = useRef(false);
  const nameInputRef = useRef<HTMLInputElement>(null);
  const descriptionInput = useRef<HTMLTextAreaElement>(null);
  const descriptionId = useId();
  const personaLabelId = useId();
  const saveNoteId = useId();
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
    nameInputRef.current?.focus();
  };
  const clearTemplate = () => {
    if (templateDefaults) {
      const defaults = templateDefaults;
      setDraft(current => ({ ...current,
        name: current.name === defaults.name ? "" : current.name,
        description: current.description === defaults.description ? "" : current.description,
        role: current.role === defaults.role ? agent?.role ?? "worker" : current.role,
      }));
      setTemplateDefaults(undefined);
    }
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
  const saveNote = libraryScope
    ? "修改会用于关联项目的后续任务。单独编辑过的项目成员使用该项目的配置。"
    : "配置用于后续任务；执行中的任务须完成或停止后再编辑。切换 Runtime 会停止空闲旧终端并保留只读归档，下次使用新会话；旧收件箱待发消息标为失败，需要重新发送。人格随每次派单注入提示词，保存即生效。";
  const runtimeField = (
    <label className="collab-editor-runtime">
      Runtime
      <select
        aria-label="Runtime"
        value={draft.runtimeKind}
        disabled={saving || runtimeLocked || !ready}
        onChange={(e) => setDraft({
          ...draft,
          runtimeKind: e.target.value as AgentRuntimeKind,
          model: agent && e.target.value === agent.runtimeKind ? agent.model : null,
        })}
      >
        {AVAILABLE.filter(isRuntimeEnabled).map((kind) => (
          <option value={kind} key={kind}>
            {kind === "deepseek_harness" ? "DeepSeek Harness CLI（dsh）" : RUNTIME_LABELS[kind]}
          </option>
        ))}
        {agent && isLegacyCollaborationRuntime(agent.runtimeKind) && isRuntimeEnabled(agent.runtimeKind) && (
          <option value={agent.runtimeKind}>{RUNTIME_LABELS[agent.runtimeKind]}（已有成员兼容）</option>
        )}
        {!isRuntimeEnabled(draft.runtimeKind) && (
          <option value={draft.runtimeKind} disabled>{RUNTIME_LABELS[draft.runtimeKind]}（不可用）</option>
        )}
      </select>
    </label>
  );
  return createPortal(
    <Modal
      initialFocusRef={nameInputRef}
      overlayClassName="dialog-overlay collab-agent-editor-overlay"
      className="collab-agent-editor collab-member-editor"
      ariaLabel={title}
      onClose={() => { if (!saveInFlight.current && !importInFlight.current) onClose(); }}
    >
      <header className="collab-member-editor-head">
        <div>
          <h3>{title}</h3>
          <p>{libraryScope ? "维护多个项目共用的成员配置。" : "按项目分工设置成员，同一个 Runtime 可以创建多个成员。"}</p>
        </div>
        <button type="button" className="collab-member-editor-close" aria-label="关闭成员编辑" disabled={avatarBusy} onClick={onClose}>×</button>
      </header>

      <div className="collab-member-editor-scroll">
        {!agent && projectId && (
          <section className="collab-agent-templates" aria-label={t("从模板创建成员")}>
            <div className="collab-agent-templates-head">
              <strong>{t("从模板创建成员")}</strong>
              <small>{t("套用模板时会保留手动填写的名称和职责。")}</small>
            </div>
            <div className="collab-agent-template-options">
              <button
                type="button"
                className={`collab-agent-template${templateDefaults ? "" : " selected"}`}
                aria-label={t("空白成员")}
                aria-pressed={!templateDefaults}
                disabled={avatarBusy}
                onClick={clearTemplate}
              >
                <strong>{t("空白成员")}</strong>
                <small>{t("从零开始自定义名称、职责与角色。")}</small>
              </button>
              {collaborationMemberTemplates.map(template => {
                const selected = templateDefaults?.name === t(template.name);
                return (
                  <button
                    key={template.id}
                    type="button"
                    className={`collab-agent-template${selected ? " selected" : ""}`}
                    aria-label={t(template.name)}
                    aria-pressed={selected}
                    disabled={avatarBusy}
                    onClick={() => applyTemplate(template)}
                  >
                    <strong>{t(template.name)}</strong>
                    <small>{t(template.summary)}</small>
                  </button>
                );
              })}
            </div>
          </section>
        )}

        <div className="collab-agent-editor-body">
          <aside className="collab-member-editor-side">
            <div className="collab-member-editor-side-inner">
              <div className="collab-agent-editor-preview">
                <span className="collab-agent-editor-preview-label">名片预览</span>
                <AgentCard
                  agent={projectId && catalog?.supported ? { ...draft, model: modelPreview?.model ?? t("跟随 Runtime") } : draft}
                  reasoning={modelPreview?.reasoning}
                />
              </div>

              <fieldset className="collab-avatar-picker">
                <legend>成员头像</legend>
                <div className="collab-avatar-top">
                  <button
                    type="button"
                    className={draft.avatarId === null ? "selected" : ""}
                    aria-pressed={draft.avatarId === null}
                    disabled={avatarBusy}
                    onClick={() => selectAvatar(null)}
                  >
                    <AgentAvatar runtimeKind={draft.runtimeKind} decorative />
                    <span>Runtime 默认</span>
                  </button>
                  {customAvatarId && (
                    <button
                      type="button"
                      className={draft.avatarId === customAvatarId ? "selected" : ""}
                      aria-pressed={draft.avatarId === customAvatarId}
                      disabled={avatarBusy}
                      onClick={() => selectAvatar(customAvatarId)}
                    >
                      <AgentAvatar avatarId={customAvatarId} runtimeKind={draft.runtimeKind} decorative />
                      <span>自定义头像</span>
                    </button>
                  )}
                  <button
                    type="button"
                    className="collab-avatar-import"
                    disabled={avatarBusy}
                    title="PNG、JPG、WebP · 最大 10 MB"
                    onClick={() => void importAvatar()}
                  >
                    <ImageSquare size={16} aria-hidden="true" />
                    <span>{importing ? "正在导入…" : "导入头像"}</span>
                  </button>
                </div>
                <div className="collab-avatar-grid">
                  {AGENT_AVATARS.map((avatar) => (
                    <button
                      type="button"
                      key={avatar.id}
                      className={draft.avatarId === avatar.id ? "selected" : ""}
                      aria-label={avatar.name}
                      title={avatar.name}
                      aria-pressed={draft.avatarId === avatar.id}
                      disabled={avatarBusy}
                      onClick={() => selectAvatar(avatar.id)}
                    >
                      <AgentAvatar avatarId={avatar.id} runtimeKind={draft.runtimeKind} decorative />
                    </button>
                  ))}
                </div>
                {avatarError && <div className="collab-agent-error" role="alert">{avatarError}</div>}
                <small>头像只标识成员，Runtime 与职责可以独立调整。<br />导入支持 PNG、JPG、WebP · 最大 10 MB</small>
              </fieldset>
            </div>
          </aside>

          <div className="collab-agent-editor-form">
            <section className="collab-member-section" aria-label={t("基本信息")}>
              <h4><b>1</b>{t("基本信息")}</h4>
              <label>
                成员名称
                <input
                  ref={nameInputRef}
                  value={draft.name}
                  maxLength={100}
                  disabled={saving}
                  onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                  placeholder="例如：Codex 后端工程师"
                />
              </label>
            </section>

            <section className="collab-member-section collab-agent-model-settings" aria-label={t("执行引擎")} aria-busy={modelBusy}>
              <h4><b>2</b>{t("执行引擎")}</h4>
              <div className={`collab-member-engine-row${projectId ? "" : " single"}`}>
                {runtimeField}
                {projectId && (["model", "reasoning"] as const).map(field => {
                  const options = catalog?.[field] ?? [];
                  const value = selection[field] ?? "";
                  return (
                    <label key={field}>
                      {t(field === "model" ? "模型" : "推理级别")}
                      <select
                        aria-label={t(field === "model" ? "模型" : "推理级别")}
                        value={value}
                        disabled={saving || !runtimeEnabled || modelBusy || (!options.length && !value) || (field === "reasoning" && !!modelError)}
                        onChange={event => {
                          const next = event.target.value || null;
                          if (field === "model") changeModel(next);
                          else { setSelection(current => ({ ...current, reasoning: next })); setModelChanged(true); }
                        }}
                      >
                        <option value="">{t("跟随 Runtime")}</option>
                        {value && !options.some(option => option.value === value) && (
                          <option value={value} disabled>{value} · {t("已不可用")}</option>
                        )}
                        {options.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
                      </select>
                      {catalog && !options.length && !modelBusy && !modelError && <small>{t("Runtime 未提供此选项")}</small>}
                    </label>
                  );
                })}
              </div>
              {projectId && <small>{t("用于团队聊天的原生 Runtime，从下一次派发起生效。")}</small>}
              {projectId && modelBusy && <small className="collab-member-loading" role="status">{t("正在读取可用选项…")}</small>}
              {projectId && modelError && (
                <div className="collab-agent-model-error">
                  <small role="status">{modelError}</small>
                  <button type="button" className="btn secondary" disabled={saving} onClick={() => void readModels(modelChanged ? selection : undefined)}>
                    {t("重试")}
                  </button>
                </div>
              )}
              {runtimeLocked && (
                <small>{libraryScope ? "该成员已有关联项目会话，保留当前 Runtime。" : "该成员有执行中的任务，等待空闲后可切换 Runtime。"}</small>
              )}
              {keepingLegacy && (
                <small>保留该成员原有 ZCode Desktop 配置和执行记录。选择 CLI 并保存才会切换，不会自动替换账号或模型。</small>
              )}
              {agent && isLegacyCollaborationRuntime(agent.runtimeKind) && !keepingLegacy && (
                <small>保存后改用所选 CLI 的登录和默认模型；原有执行记录保留。</small>
              )}
              {draft.runtimeKind === "deepseek_harness" && (
                <small>通过本机 dsh 执行任务，沿用其登录与模型设置。</small>
              )}
            </section>

            <section className="collab-member-section" aria-label={t("人格与职责")}>
              <h4><b>3</b>{t("人格与职责")}</h4>
              <div className="collab-member-field">
                <div className="collab-member-field-label">
                  <span id={personaLabelId}>人格</span>
                  <small>{t("只约束沟通风格")}</small>
                </div>
                <div className="collab-persona-chips" role="radiogroup" aria-labelledby={personaLabelId}>
                  {PERSONA_OPTIONS.map((value) => (
                    <label key={value} className={`collab-persona-chip${draft.persona === value ? " selected" : ""}`}>
                      <input
                        type="radio"
                        name={`${personaLabelId}-persona`}
                        value={value}
                        checked={draft.persona === value}
                        disabled={saving}
                        onChange={() => setDraft({ ...draft, persona: value })}
                      />
                      <span>{AGENT_PERSONA_LABELS[value]}</span>
                    </label>
                  ))}
                </div>
                <small className="collab-persona-hint">
                  {AGENT_PERSONA_DESCRIPTIONS[draft.persona]}
                  {draft.persona !== "custom" ? "。人格只约束沟通风格，职责仍由下方职责描述决定。" : ""}
                </small>
              </div>
              {draft.persona === "custom" && (
                <label className="collab-persona-custom">
                  自定义工作风格
                  <textarea
                    rows={2}
                    value={draft.personaCustom}
                    maxLength={PERSONA_CUSTOM_MAX}
                    disabled={saving}
                    onChange={(e) => setDraft({ ...draft, personaCustom: e.target.value.slice(0, PERSONA_CUSTOM_MAX) })}
                    placeholder="例如：先复盘历史任务再动手，遇到歧义先提问，完成后主动给出验证步骤"
                  />
                  <span className="collab-persona-count">{draft.personaCustom.length} / {PERSONA_CUSTOM_MAX}</span>
                </label>
              )}
              <div className="collab-agent-responsibilities">
                <div className="collab-agent-responsibilities-heading">
                  <label htmlFor={descriptionId}>{t("职责定位")}</label>
                  {draft.role === "worker" && !hasImplementationResponsibilities && (
                    <button
                      type="button"
                      className="conversation-text-button"
                      aria-controls={descriptionId}
                      disabled={saving}
                      onClick={addImplementationResponsibilities}
                    >
                      {t(hasDescription ? "追加代码实现职责" : "填入代码实现职责")}
                    </button>
                  )}
                </div>
                <textarea
                  ref={descriptionInput}
                  id={descriptionId}
                  rows={4}
                  value={draft.description}
                  disabled={saving}
                  aria-describedby={showRoleReview ? `${descriptionId}-role-review` : undefined}
                  onChange={event => {
                    const description = event.target.value;
                    setDraft(current => ({ ...current, description }));
                  }}
                  placeholder={t("例如：擅长后端接口和数据存储，优先关注接口兼容性与数据一致性。")}
                />
                {showRoleReview && (
                  <small className="collab-role-review" id={`${descriptionId}-role-review`} role="status">
                    {t("请核对当前托管角色与职责定位是否一致。")}
                  </small>
                )}
              </div>
            </section>

            <section className="collab-member-section" aria-label={t("托管与状态")}>
              <h4><b>4</b>{t("托管与状态")}</h4>
              <details className="collab-agent-managed-settings">
                <summary>
                  <span className="collab-managed-summary-left">
                    <strong>{t("托管设置")}</strong>
                    <small className="collab-member-role-chip">{t(COLLABORATION_ROLE_LABELS[draft.role])}</small>
                  </span>
                  <svg className="collab-managed-caret" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" aria-hidden="true">
                    <path d="m6 9 6 6 6-6" />
                  </svg>
                </summary>
                <div>
                  <fieldset className="collab-role-options" role="radiogroup" aria-label={t("托管角色")}>
                    <legend>{t("托管角色")}</legend>
                    {(Object.keys(COLLABORATION_ROLE_LABELS) as AgentRole[]).map(value => (
                      <label key={value} className={`collab-role-option${draft.role === value ? " selected" : ""}`}>
                        <input
                          type="radio"
                          name={`${descriptionId}-role`}
                          value={value}
                          checked={draft.role === value}
                          disabled={saving}
                          onChange={() => setDraft(current => ({ ...current, role: value }))}
                        />
                        <span className="collab-role-dot" aria-hidden="true" />
                        <span>
                          <strong>{t(COLLABORATION_ROLE_LABELS[value])}</strong>
                          <small>{t(ROLE_DESCRIPTIONS[value])}</small>
                        </span>
                      </label>
                    ))}
                  </fieldset>
                  <small>{t("角色用于推荐接手成员和检查托管分工。实际工具权限与审批由本轮授权和 Runtime 设置决定。")}</small>
                </div>
              </details>
              {agent && (
                <label className="collab-agent-enabled">
                  <span>
                    <strong>启用成员</strong>
                    <small>停用后不再领取新任务</small>
                  </span>
                  <input
                    type="checkbox"
                    className="collab-member-switch"
                    checked={draft.enabled}
                    disabled={saving || isRetiredRuntime(draft.runtimeKind)}
                    onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })}
                  />
                </label>
              )}
            </section>
          </div>
        </div>
      </div>

      {error && <div className="collab-agent-error collab-member-editor-error" role="alert">{error}</div>}
      <footer className="collab-member-editor-foot">
        <span className="collab-member-note" tabIndex={0} aria-describedby={saveNoteId}>
          <i aria-hidden="true">i</i>
          {t("保存与生效说明")}
          <span className="collab-member-note-tip" role="tooltip" id={saveNoteId}>{saveNote}</span>
        </span>
        <div className="collab-member-editor-actions">
          <button type="button" className="btn secondary" disabled={avatarBusy} onClick={onClose}>取消</button>
          <button type="button" className="btn primary" disabled={avatarBusy || modelsPending} onClick={() => void save()}>
            {saving ? "保存中…" : "保存成员"}
          </button>
        </div>
      </footer>
    </Modal>,
    document.body,
  );
}
