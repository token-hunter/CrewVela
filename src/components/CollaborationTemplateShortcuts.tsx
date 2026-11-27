import { useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { ArrowRight, CalendarBlank, ClipboardText, GitPullRequest, Tag, X } from "@phosphor-icons/react";
import type { AgentProfile, AgentRuntimeKind, Automation, ConversationSnapshot } from "../types";
import { collaborationTemplates, templateBody, templateIds, templateMembers, templateSchedule, templateTrial, type TemplateAction, type TemplateId } from "../collaborationTemplates";
import { RUNTIME_LABELS } from "../runtimeLogos";
import { useRuntimeAvailability } from "../runtimeAvailability";
import { useTranslation } from "../useInterfaceLanguage";
import { Modal } from "./ui/Modal";
import "../styles/collaboration-templates.css";

interface Props {
  projectId: string;
  conversationId: string;
  agents: AgentProfile[];
  supportedRuntimes: AgentRuntimeKind[];
  disabled?: boolean;
  onChanged: () => void;
}

const icons = { pr_review: GitPullRequest, issue_triage: Tag, release_check: ClipboardText };

export function CollaborationTemplateShortcuts(props: Props) {
  const t = useTranslation();
  const [opened, setOpened] = useState<{ template: TemplateId; topic: string; project: string } | null>(null);
  const { isRuntimeEnabled } = useRuntimeAvailability();
  const members = templateMembers(props.agents, props.projectId, props.supportedRuntimes.filter(isRuntimeEnabled));
  const open = opened?.project === props.projectId && opened.topic === props.conversationId ? opened : null;
  return <section className="collab-template-shortcuts" aria-label={t("协作模板")}>
    <h3>{t("协作模板")}</h3>
    {templateIds.map(id => {
      const Icon = icons[id];
      return <button key={id} type="button" disabled={!props.conversationId || props.disabled} onClick={() => setOpened({ template: id, project: props.projectId, topic: props.conversationId })}>
        <Icon size={15} aria-hidden="true" /><span>{t(collaborationTemplates[id].title)}</span><ArrowRight size={13} aria-hidden="true" />
      </button>;
    })}
    {open && <TemplateDialog key={open.template + open.topic} template={open.template} members={members}
      projectId={props.projectId} conversationId={props.conversationId} disabled={props.disabled}
      onChanged={props.onChanged} onClose={() => setOpened(null)} />}
  </section>;
}

function TemplateDialog({ template, members, projectId, conversationId, disabled, onChanged, onClose }: Pick<Props, "projectId" | "conversationId" | "disabled" | "onChanged"> & {
  template: TemplateId; members: AgentProfile[]; onClose: () => void;
}) {
  const t = useTranslation();
  const heading = useId();
  const spec = collaborationTemplates[template];
  const [material, setMaterial] = useState("");
  const [agentId, setAgentId] = useState(members[0]?.id ?? "");
  const member = members.find(item => item.id === agentId);
  const [reviewSchedule, setReviewSchedule] = useState(false);
  const [weekdays, setWeekdays] = useState(true);
  const [time, setTime] = useState("09:00");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [receipt, setReceipt] = useState("");
  const [savedPlan, setSavedPlan] = useState(false);
  const [lastAccepted, setLastAccepted] = useState("");
  const flight = useRef(false);
  const trialId = useRef<{ key: string; id: string } | null>(null);
  const planId = useRef<{ key: string; id: string } | null>(null);
  const first = useRef<HTMLTextAreaElement>(null);
  const body = templateBody(template, material, t);
  const key = JSON.stringify([agentId, body]);
  const ready = !!material.trim() && !!member && !disabled && !busy;
  const action: TemplateAction = { kind: "conversation_template", templateId: template, projectId, conversationId, agentId, body };
  const close = () => { if (!flight.current) onClose(); };
  const run = async (schedule: boolean) => {
    if (!ready || flight.current) return;
    flight.current = true;
    setBusy(true); setError(""); setReceipt("");
    try {
      if (schedule) {
        const planKey = JSON.stringify([key, time, weekdays]);
        if (planId.current?.key !== planKey) planId.current = { key: planKey, id: crypto.randomUUID() };
        const input = templateSchedule(action, t(spec.title), time, weekdays, planId.current.id);
        const created = await invoke<Automation>("create_automation", { input });
        if (!created.enabled || !created.id || created.action.kind !== "conversation_template" || created.action.conversationId !== conversationId) {
          throw Error(t("请在自动化中核对计划状态"));
        }
        setSavedPlan(true);
        setReceipt(t("计划已启用，可在自动化中查看记录或暂停。"));
      } else {
        if (trialId.current?.key !== key) trialId.current = { key, id: crypto.randomUUID() };
        const input = templateTrial(action, trialId.current.id);
        const result = await invoke<ConversationSnapshot>("ma_conversation_send", { input, templateTrial: true });
        if (!result.messages.some(message => message.clientMessageId === input.clientMessageId && message.conversationId === conversationId)) {
          throw Error(t("尚未取得试跑回执，请核对话题后重试"));
        }
        setLastAccepted(key);
        setReceipt(t("本次试跑已提交，成员的回复会出现在本话题。"));
      }
      onChanged();
    } catch (reason) { setError(String(reason)); }
    finally { flight.current = false; setBusy(false); }
  };
  return createPortal(<Modal className="collab-agent-editor collab-template-dialog" overlayClassName="dialog-overlay collab-agent-editor-overlay"
    labelledBy={heading} initialFocusRef={first} onClose={close}>
    <header><h3 id={heading}>{t(spec.title)}</h3><button type="button" aria-label={t("关闭")} disabled={busy} onClick={close}><X size={17} /></button></header>
    <div className="collab-template-fields">
      <div className="collab-template-material"><label><span>{t(spec.input)}</span>
        <textarea ref={first} rows={3} maxLength={28000} value={material} placeholder={t(spec.placeholder)} disabled={busy || reviewSchedule}
          onChange={event => { setMaterial(event.target.value); setReceipt(""); }} /></label>
        {!reviewSchedule && <button className="conversation-text-button" type="button" disabled={busy} onClick={() => { setMaterial(t(spec.example)); setReceipt(""); }}>{t("填入示例")}</button>}
      </div>
      <label><span>{t("执行成员")}</span><select value={member?.id ?? ""} disabled={busy || reviewSchedule} onChange={event => { setAgentId(event.target.value); setReceipt(""); }}>
        {!member && <option value="">{t("选择项目成员")}</option>}
        {members.map(agent => <option key={agent.id} value={agent.id}>{agent.name} · {RUNTIME_LABELS[agent.runtimeKind]}</option>)}
      </select></label>
      {!members.length && <small role="status">{t("请先启用支持只读研究的 Claude Code 或 Pi 成员。")}</small>}
      <details className="collab-template-preview" open={reviewSchedule || undefined}>
        <summary>{t("预览实际任务")}</summary><pre>{body}</pre>
      </details>
      <details className="collab-template-preview"><summary>{t("查看示例结果")}</summary><p>{t(spec.output)}</p></details>
      <small>{t("只读检查，回复保存在本话题。")}</small>
      {reviewSchedule && <div className="collab-template-schedule">
        <strong><CalendarBlank size={16} aria-hidden="true" />{t("定时运行")}</strong>
        <div><label>{t("重复")}<select value={weekdays ? "weekdays" : "daily"} disabled={busy || savedPlan} onChange={e => setWeekdays(e.target.value === "weekdays")}>
          <option value="weekdays">{t("工作日")}</option><option value="daily">{t("每天")}</option></select></label>
          <label>{t("本机时间")}<input type="time" value={time} disabled={busy || savedPlan} onChange={e => setTime(e.target.value)} /></label></div>
        <small>{t("应用运行时按此时间执行，沿用上方材料、成员和话题。")}</small>
      </div>}
      {receipt && <p role="status" className="collab-template-receipt">{receipt}</p>}
      {error && <p role="alert" className="collab-agent-error">{error}</p>}
    </div>
    <footer>{savedPlan ? <button type="button" className="btn primary" onClick={close}>{t("完成")}</button> : reviewSchedule ? <>
      <button type="button" className="btn secondary" disabled={busy} onClick={() => { setReviewSchedule(false); setError(""); }}>{t("返回修改")}</button>
      <button type="button" className="btn primary" disabled={!ready || !time} onClick={() => void run(true)}>{t("确认启用定时")}</button>
    </> : <>
      <button type="button" className="btn secondary" disabled={!ready} onClick={() => { setReviewSchedule(true); setReceipt(""); setError(""); }}>{t("设置定时")}</button>
      {lastAccepted === key ? <button type="button" className="btn primary" onClick={close}>{t("返回话题")}</button>
        : <button type="button" className="btn primary" disabled={!ready} onClick={() => void run(false)}>{t("试跑一次")}</button>}
    </>}</footer>
  </Modal>, document.body);
}
