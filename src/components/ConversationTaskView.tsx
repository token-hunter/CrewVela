import { useLayoutEffect, useRef, useState } from "react";
import { ArrowBendUpLeft, CheckCircle, Circle, ClockCounterClockwise, FileText, LinkSimple, Pause, Play, PlayCircle, Target, User, WarningCircle } from "@phosphor-icons/react";
import { AgentAvatar } from "./AgentAvatar";
import { UnverifiedRunNotice } from "./UnverifiedRunNotice";
import { TaskRunRecords } from "./TaskRunRecords";
import { CONVERSATION_STATUS, TASK_STATUS } from "../collaborationConversation";
import { orderedTaskRuns } from "../collaborationTasks";
import { taskMember } from "../taskMember";
import type { AgentRun, ConversationExecutionPlan, ConversationRound, MultiAgentSnapshot, MultiAgentTask } from "../types";

const running = (task: MultiAgentTask) => ["claimed", "running"].includes(task.status);
const statusLabel = (task: MultiAgentTask, pendingDependency: boolean) => ["draft", "ready"].includes(task.status) ? "待执行" : task.status === "blocked" ? pendingDependency ? "等待依赖" : "等待处理" : TASK_STATUS[task.status] ?? task.status;

export function ConversationTaskView({ snapshot, tasks, plans, discussion, selectedId, initialRecordTaskId, blocked, onSelect, onExecution, onChat, onFullRecords, onOpenRun, onRefresh }: {
  snapshot: MultiAgentSnapshot | null;
  tasks: MultiAgentTask[];
  plans: ConversationExecutionPlan[];
  discussion?: ConversationRound;
  selectedId: string | null;
  initialRecordTaskId: string | null;
  blocked: boolean;
  onSelect: (id: string) => void;
  onExecution: (action: "pause" | "resume") => void;
  onChat: (messageId?: string) => void;
  onFullRecords?: () => void;
  onOpenRun?: (run: AgentRun, task: MultiAgentTask) => void;
  onRefresh: () => void;
}) {
  const [recordTask, setRecordTask] = useState(initialRecordTaskId);
  const recordsSection = useRef<HTMLElement>(null);
  useLayoutEffect(() => {
    const section = recordsSection.current;
    if (section) {
      section.focus({ preventScroll: true });
      section.scrollIntoView({ block: "start" });
    }
  }, []);
  const orderedTasks = [...tasks].sort((left, right) => Number(running(right)) - Number(running(left))
    || right.updatedAt - left.updatedAt || right.createdAt - left.createdAt);
  const task = tasks.find(item => item.id === selectedId) ?? orderedTasks.find(running);
  const agents = snapshot?.agents ?? [];
  const label = (item: MultiAgentTask) => statusLabel(item, (snapshot?.dependencies ?? []).some(dependency => dependency.taskId === item.id && snapshot?.tasks.find(other => other.id === dependency.dependsOnTaskId)?.status !== "done"));
  const planForTask = (id: string) => plans.find(plan => plan.taskIds.includes(id));
  const plan = task && planForTask(task.id);
  const assignee = taskMember(agents, task?.assigneeAgentId, plan);
  const runs = orderedTaskRuns(snapshot, task?.id);
  const dependencies = (snapshot?.dependencies ?? []).filter(item => item.taskId === task?.id).map(item => snapshot?.tasks.find(other => other.id === item.dependsOnTaskId)?.title ?? "历史前置任务");
  const paused = snapshot?.project.status === "paused";
  const schedulingControl = <button type="button" className="team-execution-control"
    disabled={blocked || !tasks.some(item => !["done", "cancelled", "failed"].includes(item.status))}
    onClick={() => onExecution(paused ? "resume" : "pause")}>
    {paused ? <Play size={18} /> : <Pause size={18} />}{paused ? "继续项目调度" : "暂停项目调度"}
  </button>;
  return <section className="conversation-task-view" aria-label="工程任务">
    {discussion && <div className="team-task-discussion">
      <span>团队聊天 · {CONVERSATION_STATUS[discussion.status]} <time dateTime={new Date(discussion.updatedAt).toISOString()}>{new Date(discussion.updatedAt).toLocaleString("zh-CN", { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false })}</time></span>
      <button type="button" className="conversation-text-button" onClick={() => onChat(discussion.userMessageId)}>查看本轮聊天</button>
    </div>}
    {paused && <p className="team-execution-paused" role="status">项目调度已暂停。继续调度会执行已就绪的任务。</p>}
    {!snapshot ? <p role="status">正在读取任务…</p> : !tasks.length ? <div className="team-tasks-empty"><p>暂无工程任务</p><button type="button" className="conversation-text-button" onClick={() => onChat()}>返回团队聊天</button></div> : <>
      <div className="team-task-list" aria-label="项目任务">{orderedTasks.map(item => {
        const member = taskMember(agents, item.assigneeAgentId, planForTask(item.id));
        const Icon = item.status === "done" ? CheckCircle : item.status === "failed" ? WarningCircle : running(item) ? PlayCircle : item.status === "blocked" ? LinkSimple : Circle;
        return <button type="button" className={`team-task-row ${item.status}`} key={item.id} aria-pressed={item.id === task?.id} onClick={() => onSelect(item.id)}>
          <span className="team-task-title"><Icon size={20} /><strong>{item.title}</strong></span>
          <span className="team-task-agent">{member.runtimeKind && <AgentAvatar avatarId={member.avatarId} runtimeKind={member.runtimeKind} decorative />}<span>{member.name}</span></span>
          <span className={`team-task-meter ${item.status}`} role="progressbar" aria-label={`${item.title}执行状态`} aria-valuetext={label(item)} {...(item.status === "done" ? { "aria-valuenow": 100 } : {})}><span /></span>
          <span className={`team-task-status ${item.status}`}>{label(item)}</span>
        </button>;
      })}</div>
      {task ? <article className="team-task-detail" aria-label={`任务详情 ${task.title}`}>
        <header><h2>{task.title}</h2><span className={`team-task-status ${task.status}`}>{label(task)}</span></header>
        <dl className="team-task-facts"><div><dt><User size={17} />负责人</dt><dd>{assignee.runtimeKind && <AgentAvatar avatarId={assignee.avatarId} runtimeKind={assignee.runtimeKind} decorative />}{assignee.name}</dd></div>
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
        {recordTask === task.id && <section ref={recordsSection} tabIndex={-1} className="team-task-records" aria-label="任务执行记录"><h3><ClockCounterClockwise size={17} />执行记录</h3>
          <TaskRunRecords key={task.id} runs={runs} onOpenRun={plan?.managed && onOpenRun ? run => onOpenRun(run, task) : undefined} />
          {onFullRecords && <button type="button" className="conversation-text-button" onClick={onFullRecords}>查看完整项目记录</button>}
        </section>}
        <footer className="team-task-actions">{schedulingControl}
          <button type="button" className="conversation-text-button team-record-button" aria-expanded={recordTask === task.id} onClick={() => setRecordTask(value => value === task.id ? null : task.id)}><FileText size={18} />查看记录</button>
          <button type="button" className="conversation-text-button team-related-chat" onClick={() => onChat(plan?.messageId)}><ArrowBendUpLeft size={17} />{plan ? "回到相关聊天" : "返回团队聊天"}</button>
        </footer>
      </article> : <footer className="team-task-actions"><span>选择任务查看详情</span>{schedulingControl}</footer>}
    </>}
  </section>;
}
