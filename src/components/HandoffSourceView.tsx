import type { SessionRecord } from "../types";
import { SWITCH_RUNTIME_LABELS, switchDeliveryText, switchRuntime, type SessionSwitchRecord } from "../sessionSwitch";
import { TranscriptViewer } from "./TranscriptViewer";
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowRight, FileText } from "@phosphor-icons/react";
import "../styles/handoff-source.css";

export function HandoffSourceView({ session, record, target, preview, onSelectTarget, onDetails, onRecord }: {
  session: SessionRecord;
  record: SessionSwitchRecord;
  target?: SessionRecord;
  preview?: boolean;
  onSelectTarget: () => void;
  onDetails: () => void;
  onRecord?: (record: SessionSwitchRecord) => void;
}) {
  const label = record.targetRuntime ? SWITCH_RUNTIME_LABELS[record.targetRuntime] ?? "接手 Agent" : "接手 Agent";
  const waiting = record.codexDelivery === "pending" || record.codexDelivery === "awaiting_receipt";
  const [returnDraft, setReturnDraft] = useState<SessionSwitchRecord | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [expanded, setExpanded] = useState<number | null>(null);
  const runtime = switchRuntime(session);
  const canReturn = Boolean(target && onRecord && !waiting && !record.returnOf && record.state === "queued"
    && ((runtime === "claude_chat" && record.targetRuntime === "codex_chat") ||
      (session.agentRuntimeKind === "claude_code" && record.targetRuntime === "codex")));
  const returnAction = async (mode: "preview" | "confirm" | "cancel") => {
    if (busy || (mode !== "preview" && !returnDraft)) return;
    setBusy(true);
    setError("");
    try {
      const result = preview
        ? { ...(returnDraft ?? record), id: "preview-return", returnOf: record.id,
            source: target ?? record.source, targetId: session.id, targetRuntime: runtime ?? undefined,
            state: mode === "preview" ? "preview" : mode === "confirm" ? "queued" : "cancelled",
            sourceHistory: [{ role: "assistant", text: "零值用例还需补测", at: null }, { role: "user", text: "输出保留三位小数", at: null }] }
        : await invoke<SessionSwitchRecord>(
            mode === "preview" ? "session_switch_return_preview" : mode === "confirm" ? "session_switch_confirm" : "session_switch_restore",
            { id: mode === "preview" ? record.id : returnDraft!.id, ...(mode === "confirm" ? { allowHandoff: true } : {}) },
          );
      setReturnDraft(result.state === "cancelled" ? null : result);
      onRecord?.(result);
      if (result.error) setError(result.error);
    } catch (failure) { setError(String(failure)); }
    finally { setBusy(false); }
  };
  return <section className="handoff-source-view" aria-label="已交接来源会话">
    {canReturn && <section className="handoff-source-banner handoff-return" aria-label="回到原 Claude Code 会话">
      <header>
        <ArrowRight size={22} weight="bold" aria-hidden />
        <div><strong>回到原 Claude Code 会话</strong>
          {returnDraft?.sourceHistory?.length ? <p>仅补发 Codex 期间的 {returnDraft.sourceHistory.length} 条更新</p> : null}
        </div>
        {!returnDraft ? <button type="button" className="btn secondary" disabled={busy} onClick={() => void returnAction("preview")}>查看新增内容</button>
          : <div className="handoff-source-actions">
              {returnDraft.state !== "queued" && <button type="button" className="btn secondary" disabled={busy} onClick={() => void returnAction("cancel")}>取消</button>}
              <button type="button" className="btn primary" disabled={busy || ["queued", "sending"].includes(returnDraft.state)}
                onClick={() => void returnAction("confirm")}>{busy ? "处理中…" : "恢复会话并补发"}</button>
            </div>}
      </header>
      {returnDraft?.sourceHistory?.length ? <ul>{returnDraft.sourceHistory.map((entry, index) => <li key={index}>
        <div><FileText size={16} /><span>{entry.text.trim().split("\n").find(line => line.trim())?.slice(0, 120)}</span>
          <button type="button" className="handoff-source-link" aria-expanded={expanded === index}
            onClick={() => setExpanded(expanded === index ? null : index)}>查看原文</button>
        </div>
        {expanded === index && <pre>{entry.text}</pre>}
      </li>)}</ul> : null}
      {error && <p role="alert">{error}</p>}
    </section>}
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
