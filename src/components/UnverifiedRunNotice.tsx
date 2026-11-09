import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AgentRun } from "../types";

interface Props {
  run: AgentRun;
  taskTitle?: string;
  onConfirmed: () => void | Promise<void>;
}

/** A failed stop is not proof of process termination. Only explicit user verification unlocks retry. */
export function UnverifiedRunNotice({ run, taskTitle, onConfirmed }: Props) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const inFlight = useRef(false);
  if (run.sessionLocator?.stopUnverified !== true) return null;
  const confirmStopped = async () => {
    if (inFlight.current || !window.confirm("请先在终端或任务管理器中确认原执行进程已停止。只有确认后才能解除冻结；若进程仍在运行，重试可能重复执行任务。确认原进程已停止并允许重试？")) return;
    inFlight.current = true;
    setBusy(true);
    setError("");
    try {
      await invoke("ma_confirm_run_stopped", { runId: run.id, confirmedStopped: true });
      await onConfirmed();
    } catch (reason) {
      setError(`解除冻结失败：${String(reason)}`);
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  };
  return <section className="multi-agent-stop-unverified" style={{ minWidth: 0, overflowWrap: "anywhere" }} aria-label="执行进程状态待确认">
    <strong>{taskTitle ?? "协作任务"}：原执行进程状态未确认</strong>
    <p role="alert">{run.error || "无法确认原进程已停止，任务已冻结以避免重复执行。"}</p>
    <button type="button" className="btn secondary" style={{ maxWidth: "100%", whiteSpace: "normal" }} disabled={busy} onClick={() => void confirmStopped()}>{busy ? "确认中…" : "确认原进程已停止并允许重试"}</button>
    {error && <p role="alert">{error}</p>}
  </section>;
}
