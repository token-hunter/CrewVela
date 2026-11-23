import { ArrowSquareOut, Check, Copy, Pause, Play, X } from "@phosphor-icons/react";
import { useState } from "react";
import { AgentAvatar } from "./AgentAvatar";
import { TaskRunRecords } from "./TaskRunRecords";
import { UnverifiedRunNotice } from "./UnverifiedRunNotice";
import { TASK_STATUS } from "../collaborationConversation";
import { orderedTaskRuns, TASK_RUN_STATUS } from "../collaborationTasks";
import { taskMember } from "../taskMember";
import type {
  AgentRun,
  ConversationExecutionPlan,
  MultiAgentSnapshot,
  MultiAgentTask,
} from "../types";

const isTaskRunning = (task: MultiAgentTask) =>
  ["claimed", "running"].includes(task.status);

function taskStatusLabel(task: MultiAgentTask, pendingDependency: boolean): string {
  if (["draft", "ready"].includes(task.status)) return "待执行";
  if (task.status === "blocked") return pendingDependency ? "等待依赖" : "等待处理";
  return TASK_STATUS[task.status] ?? task.status;
}

function overallTone(
  tasks: MultiAgentTask[],
  paused: boolean,
): "fail" | "running" | "done" | "warn" | "idle" {
  if (tasks.some((item) => item.status === "failed" || item.status === "changes_requested")) {
    return "fail";
  }
  if (tasks.some((item) => isTaskRunning(item))) return "running";
  if (paused && tasks.some((item) => !["done", "cancelled", "failed"].includes(item.status))) {
    return "warn";
  }
  if (tasks.some((item) => item.status === "waiting_approval")) return "warn";
  if (tasks.length > 0 && tasks.every((item) => item.status === "done")) return "done";
  return "idle";
}

interface Props {
  snapshot: MultiAgentSnapshot | null;
  tasks: MultiAgentTask[];
  plans: ConversationExecutionPlan[];
  retainedPlans: ConversationExecutionPlan[];
  selectedTaskId: string | null;
  blocked: boolean;
  onSelectTask: (id: string) => void;
  onClose: () => void;
  onExecution: (action: "pause" | "resume") => void;
  onOpenTasksView: () => void;
  onFullRecords?: () => void;
  onOpenRun?: (run: AgentRun, task: MultiAgentTask) => void;
  onRefresh: () => void;
}

export function GrokExecutionDrawer({
  snapshot,
  tasks,
  plans,
  retainedPlans,
  selectedTaskId,
  blocked,
  onSelectTask,
  onClose,
  onExecution,
  onOpenTasksView,
  onFullRecords,
  onOpenRun,
  onRefresh,
}: Props) {
  const [copiedCmd, setCopiedCmd] = useState(false);

  const orderedTasks = [...tasks].sort(
    (left, right) =>
      Number(isTaskRunning(right)) - Number(isTaskRunning(left)) ||
      right.updatedAt - left.updatedAt ||
      right.createdAt - left.createdAt,
  );
  const selectedTask =
    tasks.find((item) => item.id === selectedTaskId) ??
    orderedTasks.find(isTaskRunning) ??
    orderedTasks[0] ??
    null;

  const agents = snapshot?.agents ?? [];
  const paused = snapshot?.project.status === "paused";
  const tone = overallTone(tasks, paused);
  const planForTask = (id: string) =>
    plans.find((plan) => plan.taskIds.includes(id)) ??
    retainedPlans.find((plan) => plan.taskIds.includes(id));

  const label = (item: MultiAgentTask) =>
    taskStatusLabel(
      item,
      (snapshot?.dependencies ?? []).some(
        (dependency) =>
          dependency.taskId === item.id &&
          snapshot?.tasks.find((other) => other.id === dependency.dependsOnTaskId)?.status !==
            "done",
      ),
    );

  const selectedPlan = selectedTask ? planForTask(selectedTask.id) : undefined;
  const selectedRuns = orderedTaskRuns(snapshot, selectedTask?.id);
  const selectedAssignee = selectedTask
    ? taskMember(agents, selectedTask.assigneeAgentId, selectedPlan)
    : null;
  const selectedDependencies = selectedTask
    ? (snapshot?.dependencies ?? [])
        .filter((item) => item.taskId === selectedTask.id)
        .map(
          (item) =>
            snapshot?.tasks.find((other) => other.id === item.dependsOnTaskId)?.title ??
            "历史前置任务",
        )
    : [];

  const canControlScheduling =
    !blocked && tasks.some((item) => !["done", "cancelled", "failed"].includes(item.status));

  const copyVerificationCommand = (cmd: string) => {
    void navigator.clipboard?.writeText(cmd).then(() => {
      setCopiedCmd(true);
      window.setTimeout(() => setCopiedCmd(false), 1600);
    });
  };

  return (
    <aside className="grok-execution-drawer" aria-label="执行画布">
      <header className="grok-drawer-head">
        <h2 className="grok-drawer-title">
          <span className={`grok-status-dot is-${tone}`} aria-hidden="true" />
          <span>执行画布 · {tasks.length} 项任务</span>
        </h2>
        <button
          type="button"
          className="grok-icon-btn"
          aria-label="收起执行画布"
          title="收起执行画布"
          onClick={onClose}
        >
          <X size={15} aria-hidden="true" />
        </button>
      </header>

      <div className="grok-drawer-body">
        {paused && (
          <p className="grok-drawer-notice" role="status">
            项目调度已暂停，继续调度将执行已就绪任务。
          </p>
        )}

        {!snapshot ? (
          <p className="grok-drawer-empty" role="status">
            正在读取任务与执行日志…
          </p>
        ) : !orderedTasks.length ? (
          <div className="grok-drawer-empty">
            <strong>暂无工程任务</strong>
            <p>发送「生成分工」或「全自动托管」后，成员任务进度与实时运行日志将在此处同步呈现。</p>
          </div>
        ) : (
          <>
            <div className="grok-drawer-task-list" role="list" aria-label="项目任务列表">
              {orderedTasks.map((item) => {
                const member = taskMember(agents, item.assigneeAgentId, planForTask(item.id));
                const runs = orderedTaskRuns(snapshot, item.id);
                const latestRun = runs[runs.length - 1];
                const isSelected = item.id === selectedTask?.id;
                return (
                  <button
                    key={item.id}
                    type="button"
                    className={`grok-drawer-task-pill st-${item.status}${isSelected ? " is-selected" : ""}`}
                    aria-pressed={isSelected}
                    onClick={() => onSelectTask(item.id)}
                  >
                    <span className="grok-drawer-task-top">
                      <span className="grok-drawer-task-agent">
                        {member.runtimeKind && (
                          <AgentAvatar
                            avatarId={member.avatarId}
                            runtimeKind={member.runtimeKind}
                            decorative
                          />
                        )}
                        <strong>{member.name}</strong>
                      </span>
                      <span className={`grok-chip st-${item.status}`}>{label(item)}</span>
                    </span>
                    <span className="grok-drawer-task-name">{item.title}</span>
                    <span className="grok-drawer-task-meta">
                      {latestRun
                        ? `第 ${latestRun.attempt} 次 · ${TASK_RUN_STATUS[latestRun.status]}`
                        : "暂无运行记录"}
                    </span>
                  </button>
                );
              })}
            </div>

            {selectedTask && (
              <section className="grok-drawer-detail" aria-label={`执行日志 ${selectedTask.title}`}>
                <div className="grok-drawer-section-label">
                  <span>执行日志 · {selectedAssignee?.name ?? "历史成员"}</span>
                </div>

                {selectedDependencies.length > 0 && (
                  <p className="grok-drawer-meta-line">
                    依赖：{selectedDependencies.join("、")}
                  </p>
                )}

                {selectedTask.verificationCommand && (
                  <div className="grok-cmd-box">
                    <code>{selectedTask.verificationCommand}</code>
                    <button
                      type="button"
                      className="grok-icon-btn sm"
                      aria-label="复制验收命令"
                      title={copiedCmd ? "已复制" : "复制验收命令"}
                      onClick={() => copyVerificationCommand(selectedTask.verificationCommand!)}
                    >
                      {copiedCmd ? <Check size={13} /> : <Copy size={13} />}
                    </button>
                  </div>
                )}

                {selectedRuns
                  .filter((run) => run.sessionLocator?.stopUnverified === true)
                  .map((run) => (
                    <UnverifiedRunNotice
                      key={run.id}
                      run={run}
                      taskTitle={selectedTask.title}
                      onConfirmed={onRefresh}
                    />
                  ))}

                <div className="grok-drawer-logs">
                  <TaskRunRecords
                    key={selectedTask.id}
                    runs={selectedRuns}
                    onOpenRun={
                      selectedPlan?.managed && onOpenRun
                        ? (run) => onOpenRun(run, selectedTask)
                        : undefined
                    }
                  />
                </div>
              </section>
            )}
          </>
        )}
      </div>

      <footer className="grok-drawer-foot">
        <button
          type="button"
          className="grok-pill-btn primary"
          disabled={!canControlScheduling}
          onClick={() => onExecution(paused ? "resume" : "pause")}
        >
          {paused ? <Play size={13} weight="fill" /> : <Pause size={13} weight="bold" />}
          <span>{paused ? "继续调度" : "暂停调度"}</span>
        </button>
        <div className="grok-drawer-foot-links">
          <button type="button" className="grok-pill-btn" onClick={onOpenTasksView}>
            完整视图
          </button>
          {onFullRecords && (
            <button
              type="button"
              className="grok-icon-btn"
              title={`查看项目任务（${tasks.length}）`}
              aria-label={`查看任务 · ${tasks.length}`}
              onClick={onFullRecords}
            >
              <ArrowSquareOut size={14} />
            </button>
          )}
        </div>
      </footer>
    </aside>
  );
}
