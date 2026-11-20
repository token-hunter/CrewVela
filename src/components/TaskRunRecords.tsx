import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { TASK_RUN_STATUS } from "../collaborationTasks";
import type { AgentRun, RunLogEntry } from "../types";

const logLimit = 200;

function RunLogs({ projectId, runId, live }: { projectId: string; runId: string; live: boolean }) {
  const [logs, setLogs] = useState<RunLogEntry[] | null>(null);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);

  useEffect(() => {
    let disposed = false;
    let timer: number | undefined;
    async function read() {
      try {
        const entries = await invoke<RunLogEntry[]>("ma_run_log_history", { projectId, runId, limit: logLimit });
        if (!disposed) { setLogs(entries); setError(""); }
      } catch (reason) {
        if (!disposed) setError(String(reason));
      } finally {
        if (!disposed && live) timer = window.setTimeout(read, 1500);
      }
    }
    void read();
    return () => { disposed = true; window.clearTimeout(timer); };
  }, [projectId, runId, live, retry]);

  const modes = logs?.filter(log => log.stream === "launch_mode") ?? [];
  const mode = modes[modes.length - 1];
  const output = logs?.filter(log => log.seq !== mode?.seq) ?? [];
  return <>
    {error && <p role="alert">读取运行日志失败：{error} <button type="button" className="conversation-text-button" onClick={() => setRetry(value => value + 1)}>重新读取</button></p>}
    {!error && logs === null && <p role="status">正在读取运行日志…</p>}
    {!error && logs?.length === 0 && <p>{live ? "本次运行尚未产生可回看的日志。" : "本次运行没有可回看的日志。"}</p>}
    {mode && <p className="team-run-launch-mode">{mode.content}</p>}
    {(logs?.length ?? 0) >= logLimit && <p>显示最近 {logLimit} 条日志</p>}
    {output.length > 0 && <pre>{output.map(log => log.content).join("\n")}</pre>}
  </>;
}

export function TaskRunRecords({ runs, onOpenRun }: { runs: AgentRun[]; onOpenRun?: (run: AgentRun) => void }) {
  const [selected, setSelected] = useState("");
  const run = runs.find(entry => entry.id === selected) ?? runs[runs.length - 1];
  if (!run) return <p>暂无运行记录。</p>;
  const { sessionId, threadId, sessionPath } = run.sessionLocator ?? {};
  const hasOriginal = [sessionId, threadId, sessionPath].some(value => typeof value === "string" && value.length > 0);
  return <>
    {runs.length > 1 && <label className="team-run-picker">执行尝试
      <select value={selected} onChange={event => setSelected(event.target.value)}>
        <option value="">最新一次</option>
        {[...runs].reverse().map(item => <option value={item.id} key={item.id}>第 {item.attempt} 次 · {TASK_RUN_STATUS[item.status]}</option>)}
      </select>
    </label>}
    <p>执行编号：{run.id}</p>
    {onOpenRun && hasOriginal && <button type="button" className="conversation-text-button" onClick={() => onOpenRun(run)}>查看原会话记录</button>}
    {run.outputSummary && <p>{run.outputSummary}</p>}
    {run.error && <p role="alert">{run.error}</p>}
    <RunLogs key={run.id} projectId={run.projectId} runId={run.id}
      live={["queued", "starting", "running", "waiting_input", "waiting_approval"].includes(run.status)} />
  </>;
}
