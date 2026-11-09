import test from "node:test";
import assert from "node:assert/strict";

import { groupTasksByStage,
  buildTaskProgressSummary,
  buildTaskStatusPresentations,
  countActiveTasks,
  shouldExpandTaskProgress,
} from "../src/multiAgentTaskPresentation.ts";

function task(id, title, status, assigneeAgentId, updatedAt) {
  return {
    id,
    projectId: "project-1",
    parentId: null,
    stage: 0,
    title,
    description: "",
    acceptanceCriteria: "",
    verificationCommand: null,
    assigneeAgentId,
    reviewerAgentId: null,
    status,
    workspaceMode: "worktree",
    priority: 0,
    createdAt: updatedAt,
    updatedAt,
  };
}

const snapshot = {
  agents: [
    {
      id: "agent-claude",
      name: "Claude 前端",
    },
    {
      id: "agent-codex",
      name: "Codex 审查",
    },
  ],
  tasks: [
    task("task-running", "实现页面", "running", "agent-claude", 30),
    task("task-blocked", "代码审查", "blocked", "agent-codex", 20),
    task("task-dependency", "接口实现", "running", "agent-claude", 10),
  ],
  dependencies: [
    {
      taskId: "task-blocked",
      dependsOnTaskId: "task-dependency",
    },
  ],
  runs: [],
};

test("waiting tasks explain which dependency blocks them", () => {
  const rows = buildTaskStatusPresentations(snapshot);

  assert.equal(rows[0].taskId, "task-blocked");
  assert.equal(rows[0].label, "等待依赖");
  assert.equal(rows[0].detail, "等待：接口实现");
});

test("active task count only includes sessions that need live attention", () => {
  assert.equal(countActiveTasks(snapshot), 2);
});

test("completed task list collapses into one progress summary", () => {
  const completedSnapshot = {
    ...snapshot,
    tasks: [
      task("task-1", "实现", "done", "agent-claude", 10),
      task("task-2", "审核", "done", "agent-codex", 20),
    ],
  };

  assert.equal(buildTaskProgressSummary(completedSnapshot), "2/2 · 已完成");
  assert.equal(shouldExpandTaskProgress(completedSnapshot), false);
});

test("failed or approval tasks expand automatically and show attention count", () => {
  const attentionSnapshot = {
    ...snapshot,
    tasks: [
      task("task-1", "实现", "done", "agent-claude", 10),
      task("task-2", "审核", "failed", "agent-codex", 20),
      task("task-3", "验收", "in_review", "agent-codex", 30),
    ],
  };

  assert.equal(buildTaskProgressSummary(attentionSnapshot), "1/3 · 2 项需处理");
  assert.equal(shouldExpandTaskProgress(attentionSnapshot), true);
});

test("groupTasksByStage 按阶段分组并识别卡点", () => {
  const task = (id, stage, status) => ({
    id, stage, status, title: id, assigneeAgentId: null, reviewerAgentId: null,
    updatedAt: 0, description: "", acceptanceCriteria: "",
  });
  const snapshot = {
    project: {}, agents: [], runs: [], wakeups: [], events: [], messages: [],
    artifacts: [], approvals: [], dependencies: [], agentSessions: [], runLogs: [],
    instructions: [], runtimeEvents: [], memories: [], reviewDecisions: [],
    tasks: [
      task("research", 1, "done"),
      task("prd", 2, "in_review"),
      task("architecture", 3, "blocked"),
      task("frontend", 5, "ready"),
      task("backend", 5, "running"),
    ],
  };
  const groups = groupTasksByStage(snapshot);
  assert.equal(groups.length, 4);
  assert.deepEqual(groups.map((group) => group.stage), [1, 2, 3, 5]);
  assert.equal(groups[0].done, 1);
  assert.ok(groups[0].allDone);
  assert.ok(groups[1].awaitingGate, "in_review 阶段应标记人工确认");
  assert.equal(groups[3].tasks.length, 2);
  assert.equal(groups[3].needsAttention, 0);
  assert.equal(groups[3].done, 0);
  assert.deepEqual(groupTasksByStage(null), []);
});
