import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ApprovalRecord, InterventionRecord, InterventionResolutionReceipt } from "../types";
import "../styles/conversation-approval-requests.css";

type Props = {
  approvals: ApprovalRecord[];
  disabled: boolean;
  onChanged: () => void;
  onOpen?: () => void;
  onStop?: () => void;
};

export function ConversationApprovalRequests(props: Props) {
  return <>{props.approvals.map(approval => <ConversationApproval key={approval.id} {...props} approval={approval} />)}</>;
}

function ConversationApproval({ approval, disabled, onChanged, onOpen, onStop }: Props & { approval: ApprovalRecord }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const pending = useRef(false);
  const mounted = useRef(false);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const approve = async () => {
    if (pending.current || disabled) return;
    pending.current = true; setBusy(true); setError("");
    try {
      const items = await invoke<InterventionRecord[]>("ma_list_interventions");
      if (!mounted.current) return;
      const item = items.find(item => item.projectId === approval.projectId && item.runId === approval.runId && item.taskId === approval.taskId && item.kind === "approval_required" && item.status === "open" && item.metadata?.approvalId === approval.id);
      if (!item) throw new Error("此授权请求已变化，请重新核对当前会话");
      await invoke<InterventionResolutionReceipt>("ma_resolve_intervention", { input: { interventionId: item.id, action: "approve" } });
      if (mounted.current) onChanged();
    } catch (cause) {
      if (mounted.current) { setError(`批准失败：${cause instanceof Error ? cause.message : String(cause)}`); onChanged(); }
    } finally {
      pending.current = false;
      if (mounted.current) setBusy(false);
    }
  };
  const [title, ...details] = approval.summary.split("\n");
  return <section className="conversation-approval" aria-label="工具授权请求">
    <p>工具需要确认</p>
    <strong>{title || "Agent 请求执行工具"}</strong>
    {details.length > 0 && <details><summary>查看请求内容</summary><pre>{details.join("\n")}</pre></details>}
    {error && <p className="conversation-approval-error" role="alert">{error}</p>}
    <div className="conversation-approval-actions">
      <button type="button" disabled={disabled || busy} onClick={() => void approve()}>{busy ? "正在批准…" : "允许本次操作"}</button>
      {onOpen && <button type="button" disabled={busy} onClick={onOpen}>查看原会话</button>}
      {onStop && <button type="button" disabled={disabled || busy} onClick={onStop}>停止本轮</button>}
    </div>
  </section>;
}
