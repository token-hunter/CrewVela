import { useEffect, useRef, useState } from "react";
import { Target, X } from "@phosphor-icons/react";
import type { HarnessSessionSnapshot } from "../types";
import { formatContextTokens } from "../codexContextUsage";
import "../styles/codex-goal.css";

export const codexGoalStatus = { active: "执行中", paused: "已暂停", blocked: "受阻", complete: "已完成", limited: "已达用量或预算限制" };

export function CodexGoalPanel({ snapshot, open, onClose, onEdit, onControl }: {
  snapshot: HarnessSessionSnapshot | null;
  open: boolean;
  onClose: () => void;
  onEdit: () => void;
  onControl: (action: "set" | "pause" | "resume" | "clear", objective?: string) => Promise<void>;
}) {
  const [objective, setObjective] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");
  const input = useRef<HTMLTextAreaElement>(null);
  const dialog = useRef<HTMLDivElement>(null);
  const goal = snapshot?.codexGoal;
  const pending = snapshot?.codexGoalAction;
  const available = !!snapshot?.connected && !!snapshot?.codexGoalSupported;
  useEffect(() => { if (open) { setObjective(goal?.objective ?? ""); setError(""); input.current?.focus(); } }, [open]);
  useEffect(() => {
    if (!open) return;
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); onClose(); }
      if (event.key === "Tab") {
        const focusable = dialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled), textarea:not(:disabled)');
        if (!focusable?.length) return;
        const first = focusable[0], last = focusable[focusable.length - 1];
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
      }
    };
    document.addEventListener("keydown", escape);
    return () => document.removeEventListener("keydown", escape);
  }, [open, onClose]);
  const control = async (action: "set" | "pause" | "resume" | "clear") => {
    setSubmitting(true);
    setError("");
    try { await onControl(action, action === "set" ? objective.trim() : undefined); if (action === "set") onClose(); }
    catch (reason) { setError(String(reason)); }
    finally { setSubmitting(false); }
  };
  return <>
    {goal && <div className="codex-goal-bar" aria-label="当前 Codex 目标">
      <Target size={17} /><div><strong title={goal.objective}>{goal.objective}</strong><small>{pending ? "正在更新目标…" : codexGoalStatus[goal.status]} · 已用 {formatContextTokens(goal.tokensUsed)} Token{goal.tokenBudget != null ? ` / ${formatContextTokens(goal.tokenBudget)}` : ""} · {Math.floor(goal.timeUsedSeconds / 60)} 分 {goal.timeUsedSeconds % 60} 秒</small></div>
      <button type="button" onClick={() => goal.status === "complete" ? onEdit() : void control(goal.status === "active" ? "pause" : "resume")} disabled={!available || submitting || pending === "pause" || pending === "clear" || (goal.status !== "active" && (!!pending || !!snapshot?.busy))}>{goal.status === "active" ? "暂停" : goal.status === "complete" ? "设置新目标" : "继续"}</button>
      <button type="button" onClick={() => void control("clear")} disabled={!available || submitting || pending === "clear"}>清除</button>
    </div>}
    {open && <div className="codex-goal-overlay" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <div className="codex-goal-dialog harness-goal-dialog" ref={dialog} role="dialog" aria-modal="true" aria-labelledby="codex-goal-dialog-title">
        <header><h2 id="codex-goal-dialog-title"><Target size={22} />设置目标</h2><button type="button" aria-label="关闭目标设置" onClick={onClose}><X size={18} /></button></header>
        <p>设置要持续追求的目标。Codex 会持续工作，直到目标完成、受阻或达到用量限制；你可以随时暂停或清除。</p>
        <label htmlFor="codex-goal-objective">目标内容</label>
        <textarea ref={input} id="codex-goal-objective" value={objective} maxLength={4000} onChange={(event) => setObjective(event.target.value)} placeholder="例如：修复当前项目的测试失败，验证通过后完成目标。" rows={5} />
        {!available && <p role="status">{snapshot?.connected ? "当前适配器未公开目标控制能力，请升级 Codex ACP 适配器。" : "恢复 Codex 会话后才能设置目标。"}</p>}
        {error && <p role="alert">{error}</p>}
        <footer><span>{objective.length} / 4000</span><button type="button" onClick={onClose}>取消</button><button type="button" className="codex-goal-confirm" disabled={!available || !objective.trim() || !!snapshot?.busy || goal?.status === "active" || !!pending || submitting} onClick={() => void control("set")}>{submitting ? "提交中…" : "设置并开始"}</button></footer>
      </div>
    </div>}
  </>;
}
