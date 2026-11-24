import { useTranslation } from "../useInterfaceLanguage";
import { useState } from "react";
import { ArrowBendDownRight, DotsThree, Trash } from "@phosphor-icons/react";
import type { AgentProfile, ConversationMessage } from "../types";

interface Props {
  messages: ConversationMessage[];
  members: AgentProfile[];
  steerableRuns?: Record<string, string>;
  disabled: boolean;
  discussing: boolean;
  onUpdate: (message: ConversationMessage, body?: string, hold?: boolean) => Promise<ConversationMessage | undefined>;
  onResume: (message: ConversationMessage) => void;
  onSteer: (message: ConversationMessage, runId: string) => void;
}

export function ConversationMessageQueue({ messages, members, steerableRuns, disabled, discussing, onUpdate, onResume, onSteer }: Props) {
  const t = useTranslation();
  const [editing, setEditing] = useState<{ message: ConversationMessage; body: string }>();
  const [menuId, setMenuId] = useState<string>();
  const activeEditing = editing && messages.some(message => message.id === editing.message.id) ? editing : undefined;

  return <section className="conversation-queue" aria-label="消息队列">
    <header>消息队列 <span>{messages.length}</span></header>
    <ol>
      {messages.map(message => {
        const input = message.queuedInput;
        if (!input) return null;
        const recipientIds = input.chatDispatch?.kind === "assigned" ? [input.chatDispatch.agentId] : input.participantAgentIds;
        const names = recipientIds.map(id => members.find(member => member.id === id)?.name ?? "已移除成员").join("、");
        const runId = message.status === "queued" && input.participantAgentIds.length === 1 && !message.attachments?.length
          ? steerableRuns?.[recipientIds[0]] : undefined;
        const uncertain = message.status === "unconfirmed";
        const label = message.body || message.attachments?.map(file => file.name).join("、") || "附件消息";
        return <li key={message.id} className="conversation-queue-item">
          <div className="conversation-queue-summary">
            <div className="conversation-queue-text">
              <strong title={label}>{label}</strong>
              <small title={names}>发给 {names}{message.status === "needs_decision" ? " · 待确认发送"
                : message.status === "paused" ? " · 编辑暂停中" : uncertain ? " · 引导结果未确认" : ""}</small>
            </div>
            <div className="conversation-queue-actions">
              {message.status === "needs_decision"
                ? <button type="button" disabled={disabled || discussing} onClick={() => onResume(message)}>{t("继续发送")}</button>
                : <button type="button" disabled={disabled || !runId || uncertain}
                    title={runId ? "将消息交给该成员正在处理的这一轮" : "当前仅支持向正在回复的 Codex 成员引导纯文本"}
                    onClick={() => { if (runId) onSteer(message, runId); }}><ArrowBendDownRight size={15} aria-hidden="true" />{t("引导")}</button>}
              <button type="button" aria-label="移除排队消息" title="移除排队消息" disabled={disabled}
                onClick={() => void onUpdate(message)}><Trash size={15} aria-hidden="true" /></button>
              <button type="button" aria-label="排队消息操作" aria-expanded={menuId === message.id} disabled={disabled || uncertain}
                onClick={() => setMenuId(menuId === message.id ? undefined : message.id)}><DotsThree size={17} aria-hidden="true" /></button>
            </div>
          </div>
          {menuId === message.id && <div className="conversation-queue-edit-actions">
            <button type="button" disabled={disabled || uncertain || !!activeEditing} onClick={async () => {
              const held = await onUpdate(message, undefined, true);
              if (held) setEditing({ message: held, body: held.body });
              setMenuId(undefined);
            }}>{t("编辑消息")}</button>
          </div>}
          {activeEditing?.message.id === message.id && <form className="conversation-queue-edit" onSubmit={async event => {
            event.preventDefault();
            if (await onUpdate(activeEditing.message, activeEditing.body, false)) setEditing(undefined);
          }}>
            <textarea aria-label="编辑排队消息" autoFocus value={activeEditing.body}
              onChange={event => setEditing({ ...activeEditing, body: event.target.value })} />
            <div className="conversation-queue-edit-actions">
              <button type="button" disabled={disabled} onClick={async () => {
                if (message.updatedAt !== activeEditing.message.updatedAt || await onUpdate(message, undefined, false)) setEditing(undefined);
              }}>{t("取消")}</button>
              <button type="submit" disabled={disabled || (!activeEditing.body.trim() && !message.attachments?.length)}>{t("保存")}</button>
            </div>
          </form>}
        </li>;
      })}
    </ol>
  </section>;
}
