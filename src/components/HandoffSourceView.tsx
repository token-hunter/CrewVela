import type { SessionRecord } from "../types";
import { SWITCH_RUNTIME_LABELS, switchDeliveryText, type SessionSwitchRecord } from "../sessionSwitch";
import { TranscriptViewer } from "./TranscriptViewer";
import "../styles/handoff-source.css";

export function HandoffSourceView({ session, record, target, preview, onSelectTarget, onDetails }: {
  session: SessionRecord;
  record: SessionSwitchRecord;
  target?: SessionRecord;
  preview?: boolean;
  onSelectTarget: () => void;
  onDetails: () => void;
}) {
  const label = record.targetRuntime ? SWITCH_RUNTIME_LABELS[record.targetRuntime] ?? "接手 Agent" : "接手 Agent";
  const waiting = record.codexDelivery === "pending" || record.codexDelivery === "awaiting_receipt";
  return <section className="handoff-source-view" aria-label="已交接来源会话">
    <header className="handoff-source-banner">
      <div><strong>{record.state === "queued" ? waiting ? `等待 ${label} 接收` : `已交接给 ${label}` : "交接待处理"}</strong><p>{record.state === "queued" ? record.codexDelivery ? switchDeliveryText(record) : "来源 Agent 已停止。" : "交接尚未完成，请在详情中处理。"}历史保持只读，查看不会重新启动执行。</p></div>
      <div className="handoff-source-actions">
        {target && <button className="btn primary" onClick={onSelectTarget}>查看接手会话</button>}
        <button className="btn secondary" onClick={onDetails}>交接详情与撤销</button>
      </div>
    </header>
    <TranscriptViewer session={session} onClose={onDetails} preview={preview} relayHistory embedded />
  </section>;
}
