import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AgentRuntimeKind, SessionRecord } from "../types";
import { SWITCH_RUNTIME_LABELS as LABELS, switchDeliveryText, switchRuntime, switchTargets, type SessionSwitchRecord } from "../sessionSwitch";
import { useRuntimeAvailability } from "../runtimeAvailability";
import { Modal } from "./ui/Modal";
interface Props { source: SessionRecord; record?: SessionSwitchRecord; sourcePosition?: number; preview?: boolean; initialTarget?: AgentRuntimeKind; onRecord: (record: SessionSwitchRecord) => void; onClose: () => void; }
interface OpenHandoffInfo { id: string; fromAgent: string; createdAtMs: number; }
export function SessionSwitchDialog({ source, record, sourcePosition = 0, preview = false, initialTarget, onRecord, onClose }: Props) {
  const { isRuntimeEnabled } = useRuntimeAvailability();
  const targets = switchTargets(source);
  const [target, setTarget] = useState<AgentRuntimeKind>(record?.targetRuntime ?? initialTarget ?? (targets.includes("codex") ? "codex" : targets[0]));
  const [draft, setDraft] = useState<SessionSwitchRecord | null>(record ?? null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [official, setOfficial] = useState(record?.historyMode === "official" || record?.state === "importing");
  useEffect(() => {
    if (record?.codexDelivery) setDraft(current => current?.id === record.id ? record : current);
  }, [record]);
  // ai-memory 共享记忆交接棒：只读展示（offer-not-claim），接棒由目标智能体
  // 自己的 SessionStart 钩子完成，这里不代替消费。
  const [batons, setBatons] = useState<OpenHandoffInfo[] | null>(preview ? [{ id: "preview-baton-1", fromAgent: "claude-code", createdAtMs: Date.now() - 5 * 60_000 }] : null);
  const [batonError, setBatonError] = useState<string | null>(null);
  useEffect(() => {
    if (preview) return;
    let cancelled = false;
    setBatons(null);
    setBatonError(null);
    void invoke<Record<string, OpenHandoffInfo[]>>("ai_memory_open_handoffs", { cwds: [source.cwd] })
      .then((result) => {
        if (cancelled) return;
        if (!Array.isArray(result[source.cwd])) throw new Error("未返回当前项目的交接记录");
        setBatons(result[source.cwd]);
      })
      .catch((reason) => { if (!cancelled) setBatonError(`共享记忆交接记录检查失败：${String(reason)}`); });
    return () => { cancelled = true; };
  }, [preview, source.cwd]);
  const prepare = async () => {
    setBusy(true); setError(null);
    try {
      const value = preview ? { id: "preview-switch-" + source.id + "-" + target, source, sourcePosition, sourcePty: source.ptyId, targetId: null, threadId: null, targetRuntime: target, preserveSource: true, state: "preview", historyMode: "handoff", context: "预览演示：保持工作目录、需求及开发进度；真实交接将收集原始记录和 Git 证据。", error: null } as SessionSwitchRecord
        : await invoke<SessionSwitchRecord>("session_switch_preview", { sourceId: source.id, sourcePosition, targetRuntime: target });
      setDraft(value); onRecord(value);
    } catch (reason) { setError(String(reason)); } finally { setBusy(false); }
  };
  const run = async (restore: boolean) => {
    if (!draft || busy) return;
    setBusy(true); setError(null);
    try {
      const value = preview ? { ...draft, state: restore ? "cancelled" : "queued", targetId: restore ? draft.targetId : "preview-" + target + "-" + Date.now(), historyMode: official ? "official" : "handoff", error: null } as SessionSwitchRecord
        : await invoke<SessionSwitchRecord>(restore ? "session_switch_restore" : "session_switch_confirm", { id: draft.id, allowHandoff: !official });
      setDraft(value); onRecord(value);
    } catch (reason) { setError(String(reason)); } finally { setBusy(false); }
  };
  return <Modal className="editor session-switch-dialog" ariaLabel="将当前工作交接给其他 Agent" onClose={() => { if (!busy) onClose(); }}>
    <h2>将当前工作交接给其他 Agent</h2>
    <p><strong>{LABELS[switchRuntime(source)!]} → {LABELS[target]}</strong></p>
    <label>接手 Agent <select aria-label="接手 Agent" value={target} disabled={busy || !!draft} onChange={event => setTarget(event.target.value as AgentRuntimeKind)}>{targets.map(kind => <option key={kind} value={kind} disabled={!isRuntimeEnabled(kind)}>{LABELS[kind]}{!isRuntimeEnabled(kind) ? "（未启用）" : ""}</option>)}</select></label>
    <p>{draft && !draft.preserveSource ? "此记录沿用原有接力方式：来源停止并归档，接手者使用原显示位置。来源历史仍可回看，也可撤销接力。" : "先准备并预览交接资料，确认后才停止当前 Agent。来源标签和历史保留为只读，接手 Agent 在当前工作目录的新标签继续需求与进度。安装或配置不可用时会说明原因。"}</p>
    <p><strong>工作目录</strong><br /><code>{source.cwd}</code></p>
    {batonError && <p role="alert" className="session-error">{batonError}</p>}
    {!preview && !batonError && batons === null && <p role="status">正在检查共享记忆交接记录…</p>}
    {batons?.length === 0 && <p role="status">当前项目没有待接的共享记忆交接记录。</p>}
    {batons !== null && batons.length > 0 && <p role="status" className="session-switch-batons">📥 ai-memory：该目录有 {batons.length} 个待接交接棒（来自 {batons.map(baton => baton.fromAgent).join("、")}）。已装钩子的接手 Agent 启动时会自动收到；此处仅提示，不会代为消费。</p>}
    <p>目标按原生能力启用保守设置，不继承来源的 YOLO、队列或待审批请求。交接资料可能截断，接手者需核实计划与验证结果。</p>
    {(target === "pi" || target === "pi_chat") && <p>Pi 会确认 bash、powershell、write、edit 内置工具；这不提供系统级沙箱，自定义扩展工具遵循其原生权限。</p>}
    {target === "zcode_cli" && <p>ZCode CLI 接力通过其原生 app-server 执行，并显示结构化消息；输入由原生协议确认。</p>}
    {preview && <p role="status">界面演示：不会启动 Agent 或读取真实会话。</p>}
    {(error || draft?.error) && <p className="session-error" role="alert">{error ?? draft?.error}</p>}
    {draft && <details><summary>预览交接资料</summary><pre className="session-switch-context">{draft.context}</pre></details>}
    {switchRuntime(source) === "claude_code" && target === "codex" && !["queued", "cancelled"].includes(draft?.state ?? "") && <label><input type="checkbox" checked={official} disabled={busy} onChange={e => setOfficial(e.target.checked)} />使用 Codex 官方实验历史导入（失败后可改用工作交接包）</label>}
    {draft?.state === "queued" && <p role="status">{switchDeliveryText(draft)}</p>}
    {draft?.state === "cancelled" && <p role="status">本次接力已取消。恢复来源仅用于撤销；若要接回目标的新进度，请从目标发起新的接力。</p>}
    <div className="editor-actions">
      <button className="btn secondary" disabled={busy} onClick={onClose}>关闭</button>
      {draft && draft.state !== "cancelled" && <button className="btn secondary" disabled={busy} onClick={() => void run(true)}>{draft.state === "preview" ? "取消交接" : "撤销接力并恢复来源"}</button>}
      {!draft && <button className="btn primary" disabled={busy || !isRuntimeEnabled(target)} onClick={() => void prepare()}>{busy ? "准备中…" : "准备交接资料"}</button>}
      {draft?.state === "queued" && draft.codexDelivery === "pending" && <button className="btn primary" disabled={busy || !isRuntimeEnabled(target)} onClick={() => void run(false)}>继续接手会话</button>}
      {draft && !["queued", "cancelled"].includes(draft.state) && <button className="btn primary" disabled={busy || !isRuntimeEnabled(target)} onClick={() => void run(false)}>{busy ? "正在交接…" : draft.state === "preview" ? "确认交接并继续" : "重试接力"}</button>}
    </div>
  </Modal>;
}
