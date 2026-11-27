import { ArrowSquareOut, Clock, ListChecks, UserCircle, X } from "@phosphor-icons/react";
import { createPortal } from "react-dom";
import type { ReactNode } from "react";
import type { AgentProfile } from "../types";
import type { CollaborationWorkItem } from "../collaborationWorkQueue";
import { useTranslation } from "../useInterfaceLanguage";
import { RUNTIME_LABELS } from "../runtimeLogos";
import { AgentAvatar } from "./AgentAvatar";
import { Modal } from "./ui/Modal";
import { ConversationApprovalRequests } from "./ConversationApprovalRequests";

export function CollaborationWorkRail({ id, items, agents, currentId, compact, disabled, onOpen, onClose, children }: {
  id: string; items: CollaborationWorkItem[]; agents: AgentProfile[]; currentId: string; compact: boolean;
  disabled: boolean; onOpen: (item: CollaborationWorkItem) => void; onClose: () => void; children?: ReactNode;
}) {
  const t = useTranslation();
  const panel = <aside id={id} className="collab-work-rail" aria-label={t("任务队列")}>
    <header className="collab-work-heading"><h2><ListChecks size={19} aria-hidden="true" />{t("任务队列")}</h2>
      <button type="button" className="conversation-icon" aria-label={t("收起任务队列")} onClick={onClose}><X size={16} /></button></header>
    <div className="collab-work-scroll">
      {(["attention", "waiting"] as const).map(group => {
        const rows = items.filter(item => item.group === group);
        const Icon = group === "attention" ? UserCircle : Clock;
        const heading = group === "attention" ? t("待我处理") : t("等待中");
        return <section key={group} className="collab-work-group" data-group={group} aria-label={heading}>
          <h3><Icon size={17} aria-hidden="true" />{heading}<span>{rows.length}</span></h3>
          {rows.length === 0 ? <p className="collab-work-empty">{group === "attention" ? t("当前没有需要你处理的事项") : t("当前没有等待中的事项")}</p>
            : <ul>{rows.map(item => {
              const agent = agents.find(candidate => candidate.id === item.agentId && candidate.projectId === item.projectId);
              const title = item.title || t(item.label);
              const action = item.approval ? t("查看审批请求") : item.taskId ? t("打开原任务") : t("查看原请求");
              return <li key={item.id}><button type="button" className="collab-work-card" data-work-id={item.id}
                disabled={disabled} onClick={() => onOpen(item)} aria-label={t("{action}，{title}", { action, title })}>
                <span className="collab-work-card-head">{agent && <AgentAvatar avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative />}
                  {agent && <span className="collab-work-agent" title={RUNTIME_LABELS[agent.runtimeKind]}>{agent.name}</span>}
                  <span className="collab-work-state">{t(item.label)}</span></span>
                <strong>{title}</strong>
                {!!item.dependencies?.length && <span className="collab-work-reason">{t("等待完成 · {tasks}", { tasks: item.dependencies.join(t("、")) })}</span>}
                <span className="collab-work-topic">{item.origin
                  ? t("{scope} · {topic}", { scope: t(item.origin.conversationId === currentId ? "当前话题" : "其他话题"), topic: item.origin.topicTitle || t("未命名话题") })
                  : t("项目任务")}</span>
                <span className="collab-work-link">{action}<ArrowSquareOut size={13} aria-hidden="true" /></span>
              </button></li>;
            })}</ul>}
        </section>;
      })}
      {children}
    </div>
  </aside>;
  return compact ? createPortal(<Modal className="collab-work-drawer" overlayClassName="dialog-overlay collab-work-overlay"
    ariaLabel={t("任务队列")} onClose={onClose} closeOnOverlayClick>{panel}</Modal>, document.body) : panel;
}

export function WorkSourceDialog({ item, disabled, onChanged, onClose }: {
  item: CollaborationWorkItem; disabled: boolean; onChanged: () => void; onClose: () => void;
}) {
  const t = useTranslation();
  return createPortal(<Modal className="collab-work-source" ariaLabel={t("原始请求")} onClose={onClose} closeOnOverlayClick>
    <header><h2>{item.approval ? t("审批请求") : t("原始请求")}</h2>
      <button type="button" className="conversation-icon" aria-label={t("关闭")} onClick={onClose}><X size={17}/></button></header>
    {item.origin?.topicTitle && <p className="collab-work-topic">{item.origin.topicTitle}</p>}
    {item.approval && (item.approval.taskId
      ? <ConversationApprovalRequests approvals={[item.approval]} disabled={disabled} onChanged={onChanged} />
      : <pre>{item.approval.summary}</pre>)}
    {item.origin && <><h3>{t("原请求内容")}</h3><pre>{item.origin.request || t("未记录文本正文")}</pre>
      {item.origin.summary && <p>{item.origin.summary}</p>}</>}
  </Modal>, document.body);
}
