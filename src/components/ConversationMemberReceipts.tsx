import type { AgentProfile, ConversationDeliveryReceipt, ConversationMemberReceipt } from "../types";
import { useTranslation } from "../useInterfaceLanguage";
import { ConversationDeliveryStatus } from "./ConversationDeliveryStatus";

export function ConversationMemberReceipts({ receipts, agents, onOpenRun }: {
  receipts: ConversationMemberReceipt[];
  agents: AgentProfile[];
  onOpenRun?: (runId: string) => void;
}) {
  const t = useTranslation();
  const directed: ConversationDeliveryReceipt[] = [];
  for (const receipt of receipts) {
    if (receipt.purpose !== "status" && receipt.agentId && receipt.status !== "inbox") {
      directed.push({
        messageId: receipt.messageId, agentId: receipt.agentId, runId: receipt.runId,
        purpose: receipt.purpose, status: receipt.status,
      });
    }
  }
  const memberName = (id: string | null) => id ? agents.find(agent => agent.id === id)?.name ?? t("历史成员") : t("全体成员");
  return <>
    <ConversationDeliveryStatus receipts={directed} agents={agents} label="成员协作进度" onOpenRun={onOpenRun} />
    <details className="conversation-member-receipts">
      <summary>{t("查看活动")}</summary>
      <ul>
        {receipts.map(receipt => <li key={receipt.messageId}>
          <strong>{memberName(receipt.senderId)} → {memberName(receipt.agentId)}</strong>
          <pre>{JSON.stringify({
            messageId: receipt.messageId,
            stage: receipt.purpose === "status" ? "inbox" : "queued",
            delivery: receipt.status,
            ...receipt.arguments,
            ...(receipt.runId && { runId: receipt.runId }),
          }, null, 2)}</pre>
          {receipt.error && <p>{receipt.error}</p>}
        </li>)}
      </ul>
    </details>
  </>;
}
