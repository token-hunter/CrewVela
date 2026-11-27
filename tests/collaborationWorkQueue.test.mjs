import assert from "node:assert/strict";
import test from "node:test";
import { buildCollaborationWorkQueue } from "../src/collaborationWorkQueue.ts";

const project = () => ({ project: { id: "p" }, tasks: [], approvals: [], runs: [], dependencies: [] });
const chat = () => ({
  conversation: { id: "current", projectId: "p", title: "当前话题" }, messages: [], rounds: [], executionPlans: [],
});
const historical = (overrides = {}) => ({
  projectId: "p", conversationId: "saved", topicTitle: "文档修订", kind: "plan", id: "plan-saved",
  status: "pending", messageId: "m-saved", request: "更新安装说明", summary: "安装说明分工", updatedAt: 1, ...overrides,
});

test("仅采纳同项目记录，分别保留同一任务的每个待审批请求并使用原任务和话题", () => {
  const snapshot = project();
  const convo = chat();
  convo.workItems = [historical(), historical({ kind: "origin", id: "t", status: "waiting_approval" }), historical({ id: "foreign-plan", projectId: "other" })];
  snapshot.tasks = [{ projectId: "p", id: "t", title: "更新安装器", status: "waiting_approval", updatedAt: 1 }];
  const approval = { projectId: "p", taskId: "t", runId: "run", approvalType: "runtime_permission", status: "pending", createdAt: 2 };
  snapshot.approvals = [{ ...approval, id: "a1", summary: "读取签名" }, { ...approval, id: "a2", summary: "执行检查" },
    { ...approval, id: "other", projectId: "other", summary: "其他项目" }];
  const items = buildCollaborationWorkQueue(snapshot, convo);
  assert.deepEqual(items.map(item => item.id), ["approval:a1", "approval:a2", "plan:plan-saved"]);
  assert.equal(items[0].origin.conversationId, "saved");
  assert.equal(items[0].taskId, "t");
  snapshot.approvals[0].status = "approved";
  assert.deepEqual(buildCollaborationWorkQueue(snapshot, convo).map(item => item.id), ["approval:a2", "plan:plan-saved"]);
  assert.deepEqual(buildCollaborationWorkQueue(project(), { ...convo, conversation: { projectId: "wrong" } }), []);
});

test("补充输入定位实际成员回合，已完成回合和取消的排队消息不出现", () => {
  const snapshot = project();
  const convo = chat();
  const run = { projectId: "p", id: "r", taskId: "cli-message-conversation-t", agentId: "reviewer", createdAt: 4, status: "waiting_input" };
  snapshot.runs = [run];
  convo.rounds = [{ projectId: "p", conversationId: "current", id: "turn", userMessageId: "u", status: "needs_decision", createdAt: 1, updatedAt: 4 }];
  const request = { projectId: "p", conversationId: "current", roundId: "turn", id: "u", senderType: "user", body: "请核对这份说明", updatedAt: 1 };
  convo.messages = [request, { ...request, id: "reply", runId: run.id, taskId: run.taskId, senderType: "agent", body: "请补充版本号" },
    { ...request, id: "cancelled", status: "cancelled", queuedInput: {}, kind: "queued_message" }];
  const items = buildCollaborationWorkQueue(snapshot, convo);
  assert.equal(items.length, 1);
  assert.equal(items[0].id, "input:r");
  assert.equal(items[0].origin.messageId, "reply");
  assert.equal(items[0].origin.request, request.body);
  run.status = "succeeded";
  convo.rounds[0].status = "completed";
  assert.deepEqual(buildCollaborationWorkQueue(snapshot, convo), []);
});

test("等待原因只引用真实未完成任务依赖，历史中已取代的分工不进入队列", () => {
  const snapshot = project();
  const convo = chat();
  convo.workItems = [historical({ status: "superseded" })];
  snapshot.tasks = [
    { projectId: "p", id: "delivery", title: "交付", status: "blocked", updatedAt: 1 },
    { projectId: "p", id: "build", title: "构建检查", status: "running", updatedAt: 1 },
    { projectId: "p", id: "tests", title: "测试", status: "done", updatedAt: 1 },
    { projectId: "other", id: "secret", title: "另一项目的任务", status: "running", updatedAt: 1 },
  ];
  snapshot.dependencies = ["build", "tests", "secret"].map(dependsOnTaskId => ({ taskId: "delivery", dependsOnTaskId }));
  const items = buildCollaborationWorkQueue(snapshot, convo);
  assert.deepEqual(items.find(item => item.id === "task:delivery").dependencies, ["构建检查"]);
  assert.equal(items.every(item => item.group === "waiting"), true);
  snapshot.tasks[1].status = "done";
  assert.deepEqual(buildCollaborationWorkQueue(snapshot, convo)[0].dependencies, []);
});
