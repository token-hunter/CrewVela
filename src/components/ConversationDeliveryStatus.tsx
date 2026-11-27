import { useTranslation } from "../useInterfaceLanguage";
import type { AgentProfile, ConversationDeliveryReceipt } from "../types";
import "../styles/conversation-delivery-status.css";

const labels: Record<ConversationDeliveryReceipt["status"], string> = {
  queued: "已排队",
  awaiting_receipt: "等待回执",
  received: "已接收",
  running: "执行中",
  waiting_approval: "需要审批",
  needs_decision: "等待你补充",
  completed: "已回复",
  handled: "已处理",
  cancelled: "已取消",
  failed: "失败",
  unconfirmed: "结果未确认",
  not_dispatched: "未派发",
};

export function ConversationDeliveryStatus({ receipts, agents, onOpenRun, label = "成员投递回执" }: {
  receipts: ConversationDeliveryReceipt[];
  agents: AgentProfile[];
  onOpenRun?: (runId: string) => void;
  label?: string;
}) {
  const t = useTranslation();
  if (!receipts.length) return null;
  const render = (receipt: ConversationDeliveryReceipt) => {
    const name = agents.find(agent => agent.id === receipt.agentId)?.name ?? t("历史成员");
    return <><span>{name}</span>{receipt.purpose && <span>{t(receipt.purpose === "question" ? "定向提问" : "任务交接")}</span>}<span className="conversation-delivery-state">{t(labels[receipt.status])}</span></>;
  };
  return <ul className="conversation-delivery-status" aria-label={t(label)}>
    {receipts.map(receipt => {
      const runId = receipt.runId;
      const action = receipt.purpose && (["failed", "unconfirmed", "waiting_approval", "needs_decision"].includes(receipt.status)
        && runId && onOpenRun ? "查看运行记录"
        : receipt.status === "not_dispatched" ? "成员不可用，请重新指定" : undefined);
      return <li key={receipt.messageId ?? receipt.agentId} data-state={receipt.status}>
        {runId && onOpenRun
          ? <button type="button" onClick={() => onOpenRun(runId)}>{render(receipt)}{action && <span className="conversation-delivery-action">{t(action)}</span>}</button>
          : render(receipt)}
        {!(runId && onOpenRun) && action && <span className="conversation-delivery-action">{t(action)}</span>}
      </li>;
    })}
  </ul>;
}
