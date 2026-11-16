import type { AgentRunStatus, MultiAgentSnapshot } from "../types";
import { TASK_STATUS } from "../collaborationConversation";
import { UnverifiedRunNotice } from "./UnverifiedRunNotice";
import "../styles/collaboration-task-details.css";

const RUN_STATUS: Record<AgentRunStatus, string> = {
  queued: "已排队", starting: "启动中", running: "执行中", waiting_input: "等待输入",
  waiting_approval: "等待确认", succeeded: "已完成", failed: "失败", timed_out: "已超时",
  cancelled: "已取消", interrupted: "已中断",
};

/** Shared project task evidence; keep every run available when leaving the conversation drawer. */
export function CollaborationTaskDetails({ snapshot, taskId, onChanged }: {
  snapshot: MultiAgentSnapshot; taskId: string; onChanged: () => void | Promise<void>;
}) {
  const task = snapshot.tasks.find(item => item.id === taskId && item.projectId === snapshot.project.id);
  if (!task) return <p role="alert">任务已不可用，请关闭详情后重新查看。</p>;
  const runs = snapshot.runs.filter(run => run.taskId === task.id && run.projectId === snapshot.project.id).sort((a,b) => a.attempt-b.attempt || a.createdAt-b.createdAt);
  return <section className="collab-task-evidence" aria-label="任务详情与执行记录">
    <p className="collab-task-description">{task.description || "尚无任务说明"}</p>
    <dl><dt>负责人</dt><dd>{snapshot.agents.find(agent => agent.id === task.assigneeAgentId)?.name ?? "未分配"}</dd>
      <dt>状态</dt><dd>{TASK_STATUS[task.status] ?? task.status}</dd>
      <dt>工作区</dt><dd>{({worktree:"独立工作树",read_only:"只读任务",shared_write:"共享目录"} as Record<string,string>)[task.workspaceMode] ?? task.workspaceMode}</dd>
      <dt>验收要求</dt><dd>{task.acceptanceCriteria || "尚未指定"}</dd>
      <dt>验收命令</dt><dd><code>{task.verificationCommand ?? "未指定命令"}</code></dd></dl>
    <h4>执行记录</h4>{!runs.length && <p>暂无执行记录。</p>}
    {runs.map(run => <section className="collab-task-run" key={run.id} aria-label={`执行尝试 ${run.attempt}`}>
      <h4>尝试 {run.attempt} · {RUN_STATUS[run.status] ?? run.status}</h4>
      <small>执行编号：{run.id}</small>{run.error && <p role="alert">{run.error}</p>}
      {run.outputSummary && <p className="collab-task-description">{run.outputSummary}</p>}
      <details><summary>执行日志</summary><pre>{snapshot.runLogs.filter(log => log.runId === run.id && log.taskId === task.id).sort((a,b) => a.seq-b.seq).map(log => `[${new Date(log.createdAt).toLocaleTimeString("zh-CN")} ${log.stream}] ${log.content}`).join("\n") || "暂无日志记录"}</pre></details>
      <UnverifiedRunNotice run={run} taskTitle={task.title} onConfirmed={onChanged}/>
    </section>)}
  </section>;
}
