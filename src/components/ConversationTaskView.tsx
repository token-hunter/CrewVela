import { useState } from "react";
import { ArrowBendUpLeft, CheckCircle, Circle, ClockCounterClockwise, FileText, LinkSimple, Pause, Play, PlayCircle, Target, User, WarningCircle } from "@phosphor-icons/react";
import { AgentAvatar } from "./AgentAvatar";
import { UnverifiedRunNotice } from "./UnverifiedRunNotice";
import { TASK_STATUS } from "../collaborationConversation";
import type { ConversationExecutionPlan, MultiAgentSnapshot, MultiAgentTask } from "../types";

const running = (task: MultiAgentTask) => ["claimed", "running"].includes(task.status);
const statusLabel = (task: MultiAgentTask, pendingDependency: boolean) => ["draft", "ready"].includes(task.status) ? "待执行" : task.status === "blocked" ? pendingDependency ? "等待依赖" : "等待处理" : TASK_STATUS[task.status] ?? task.status;

export function ConversationTaskView({ snapshot, tasks, plans, selectedId, blocked, onSelect, onExecution, onChat, onFullRecords, onRefresh }: {
  snapshot: MultiAgentSnapshot | null;
  tasks: MultiAgentTask[];
  plans: ConversationExecutionPlan[];
  selectedId: string | null;
  blocked: boolean;
  onSelect: (id: string) => void;
  onExecution: (action: "pause" | "resume") => void;
  onChat: (messageId?: string) => void;
  onFullRecords?: () => void;
  onRefresh: () => void;
}) {
  const [recordTask, setRecordTask] = useState<string | null>(null);
  const task = tasks.find(item => item.id === selectedId) ?? tasks.find(running) ?? tasks[0];
  const agents = snapshot?.agents ?? [];
  const label = (item: MultiAgentTask) => statusLabel(item, (snapshot?.dependencies ?? []).some(dependency => dependency.taskId === item.id && snapshot?.tasks.find(other => other.id === dependency.dependsOnTaskId)?.status !== "done"));
  const assignee = agents.find(agent => agent.id === task?.assigneeAgentId);
  const plan = plans.find(item => item.taskIds.includes(task?.id ?? ""));
  const runs = (snapshot?.runs ?? []).filter(run => run.taskId === task?.id).sort((a, b) => a.createdAt - b.createdAt);
  const latest = runs[runs.length - 1];
  const dependencies = (snapshot?.dependencies ?? []).filter(item => item.taskId === task?.id).map(item => snapshot?.tasks.find(other => other.id === item.dependsOnTaskId)?.title ?? "历史前置任务");
  const logs = (snapshot?.runLogs ?? []).filter(log => log.taskId === task?.id && log.runId === latest?.id);
  const paused = snapshot?.project.status === "paused";
  const resumable = paused && tasks.some(item => !["done", "cancelled"].includes(item.status));
  return <section className="conversation-task-view" aria-label="工程任务">
    {paused && <p className="team-execution-paused" role="status">调度已暂停；已有运行的停止状态以执行记录为准。</p>}
    {!snapshot ? <p role="status">正在读取任务…</p> : !task ? <div className="team-tasks-empty"><p>暂无工程任务</p><button type="button" className="conversation-text-button" onClick={() => onChat()}>返回团队聊天</button></div> : <>
      <div className="team-task-list" aria-label="项目任务">{tasks.map(item => {
        const member = agents.find(agent => agent.id === item.assigneeAgentId);
        const Icon = item.status === "done" ? CheckCircle : item.status === "failed" ? WarningCircle : running(item) ? PlayCircle : item.status === "blocked" ? LinkSimple : Circle;
        return <button type="button" className={`team-task-row ${item.status}`} key={item.id} aria-pressed={item.id === task.id} onClick={() => onSelect(item.id)}>
          <span className="team-task-title"><Icon size={20} /><strong>{item.title}</strong></span>
          <span className="team-task-agent">{member && <AgentAvatar avatarId={member.avatarId} runtimeKind={member.runtimeKind} decorative />}<span>{member?.name ?? "未分配"}</span></span>
          <span className={`team-task-meter ${item.status}`} role="progressbar" aria-label={`${item.title}执行状态`} aria-valuetext={label(item)} {...(item.status === "done" ? { "aria-valuenow": 100 } : {})}><span /></span>
          <span className={`team-task-status ${item.status}`}>{label(item)}</span>
        </button>;
      })}</div>
      <article className="team-task-detail" aria-label={`任务详情 ${task.title}`}>
        <header><h2>{task.title}</h2><span className={`team-task-status ${task.status}`}>{label(task)}</span></header>
        <dl className="team-task-facts"><div><dt><User size={17} />负责人</dt><dd>{assignee && <AgentAvatar avatarId={assignee.avatarId} runtimeKind={assignee.runtimeKind} decorative />}{assignee?.name ?? "未分配"}</dd></div>
          <div><dt><Target size={17} />范围</dt><dd>{task.description || "未填写任务范围"}</dd></div>
          <div><dt><LinkSimple size={17} />依赖</dt><dd>{dependencies.join("、") || "无前置任务"}</dd></div>
          <div><dt><CheckCircle size={17} />验收</dt><dd>{task.acceptanceCriteria || "未填写验收要求"}</dd></div>
        </dl>
        <section className="team-task-activity" aria-label="执行动态"><h3>执行动态</h3>
          {plan?.status === "applied" && <p><CheckCircle size={18} weight="fill" />已确认分工</p>}
          {runs.map(run => <p key={run.id}><Circle size={18} /><span>{agents.find(member => member.id === run.agentId)?.name ?? "历史成员"} · {run.error || run.outputSummary || (run.startedAt ? "已开始执行任务" : "执行已登记")}{run.status !== "running" && <small>{run.status === "succeeded" ? "本次运行已结束，任务验收以任务状态为准" : run.status === "failed" ? "运行失败" : run.status === "cancelled" ? "运行已取消" : run.status}</small>}</span></p>)}
          {!runs.length && <p><Circle size={18} />{task.status === "done" ? "任务已验收完成" : task.status === "blocked" ? label(task) : "等待调度执行"}</p>}
        </section>
        {runs.filter(run => run.sessionLocator?.stopUnverified === true).map(run => <UnverifiedRunNotice key={run.id} run={run} taskTitle={task.title} onConfirmed={onRefresh} />)}
        {recordTask === task.id && <section className="team-task-records" aria-label="任务执行记录"><h3><ClockCounterClockwise size={17} />执行记录</h3>
          {latest ? <><p>执行编号：{latest.id}</p>{latest.outputSummary && <p>{latest.outputSummary}</p>}{latest.error && <p role="alert">{latest.error}</p>}{logs.length ? <pre>{logs.map(log => log.content).join("\n")}</pre> : <p>暂无可见运行日志。</p>}</> : <p>暂无运行记录。</p>}
          {onFullRecords && <button type="button" className="conversation-text-button" onClick={onFullRecords}>查看完整项目记录</button>}
        </section>}
        <footer className="team-task-actions"><button type="button" className="team-execution-control" title={paused ? "继续当前项目的工程调度" : "暂停当前项目的工程执行"} disabled={blocked || (paused ? !resumable : !tasks.some(item => !["done", "cancelled"].includes(item.status)))} onClick={() => onExecution(paused ? "resume" : "pause")}>{paused ? <Play size={18} /> : <Pause size={18} />}{paused ? "继续执行" : "暂停执行"}</button>
          <button type="button" className="conversation-text-button team-record-button" aria-expanded={recordTask === task.id} onClick={() => setRecordTask(value => value === task.id ? null : task.id)}><FileText size={18} />查看记录</button>
          <button type="button" className="conversation-text-button team-related-chat" onClick={() => onChat(plan?.messageId)}><ArrowBendUpLeft size={17} />{plan ? "回到相关聊天" : "返回团队聊天"}</button>
        </footer>
      </article>
    </>}
  </section>;
}
