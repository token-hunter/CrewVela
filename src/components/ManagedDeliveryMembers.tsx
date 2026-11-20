import { useId } from "react";
import { CaretRight, User } from "@phosphor-icons/react";
import { AgentAvatar } from "./AgentAvatar";
import { orderedTaskRuns, TASK_RUN_STATUS } from "../collaborationTasks";
import { taskMember } from "../taskMember";
import type { AgentRun, ConversationExecutionPlan, MultiAgentSnapshot, MultiAgentTask } from "../types";

function acceptance(task: MultiAgentTask | undefined, plan: ConversationExecutionPlan): string {
  if (!task) {
    if (plan.status === "pending") return plan.managed?.status === "planning" ? "待派发" : "未派发";
    return "暂无验收记录";
  }
  switch (task.status) {
    case "done": return "验收通过";
    case "failed":
    case "changes_requested": return "未通过验收";
    case "cancelled": return "已取消";
    default: return "待验收";
  }
}

function attemptLabel(run: AgentRun, runs: AgentRun[]): string {
  const round = run.attempt === 1 ? "首轮"
    : run.attempt === 2 && runs.length === 2 && runs[0].attempt === 1 ? "重试" : `第 ${run.attempt} 次`;
  return round + (run.status === "succeeded" ? "成功" : TASK_RUN_STATUS[run.status]);
}

export function ManagedDeliveryMembers({ plan, snapshot, onRecords }: {
  plan: ConversationExecutionPlan;
  snapshot: MultiAgentSnapshot | null;
  onRecords: (taskId: string) => void;
}) {
  const headingId = useId();
  const project = snapshot?.project.id === plan.projectId ? snapshot : null;
  const rowCount = Math.max(plan.tasks.length, plan.taskIds.length);
  if (!rowCount) return null;

  const agents = project?.agents ?? [];

  return <div className="team-delivery-members" role="group" aria-labelledby={headingId}>
    <h4 id={headingId}>成员交付明细</h4>
    <div className="team-plan-rows">{Array.from({ length: rowCount }, (_, index) => {
      // The host stores taskIds in the same order as the original plan, even when snapshot order changes.
      const planned = plan.tasks[index];
      const id = plan.taskIds[index];
      const task = project?.tasks.find(item => item.id === id && item.projectId === plan.projectId);
      const runs = orderedTaskRuns(project, id);
      const assigneeId = task ? task.assigneeAgentId : planned?.assigneeAgentId;
      const assignee = taskMember(agents, assigneeId, plan);
      const title = task?.title ?? planned?.title ?? "历史任务";
      const attempts = runs.map(run => {
        const label = attemptLabel(run, runs);
        return run.agentId === assigneeId ? label : `${label}（${taskMember(agents, run.agentId, plan).name}）`;
      }).join(" → ");
      const statusId = `${headingId}-status-${index}`;
      const key = id || planned.key;
      const content = <>
        <span className="team-task-agent">{assignee.runtimeKind
          ? <AgentAvatar avatarId={assignee.avatarId} runtimeKind={assignee.runtimeKind} decorative />
          : <User size={20} aria-hidden="true" />}<strong>{assignee.name}</strong></span>
        <span className="team-delivery-task">{title}</span>
        <span className="team-delivery-result" id={statusId}>
          {(attempts || task) && <small>{attempts || "暂无运行记录"}</small>}
          <span className={`team-task-status ${task?.status ?? ""}`}>{acceptance(task, plan)}</span>
        </span>
        {task && <CaretRight className="team-delivery-link" size={16} aria-hidden="true" />}
      </>;
      return task
        ? <button type="button" className="team-delivery-row" key={key}
          aria-label={`查看 ${assignee.name} 的执行记录：${title}`} aria-describedby={statusId}
          onClick={() => onRecords(task.id)}>{content}</button>
        : <div className="team-delivery-row" key={key}>{content}</div>;
    })}</div>
  </div>;
}
