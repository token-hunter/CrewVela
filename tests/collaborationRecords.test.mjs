import test from "node:test";
import assert from "node:assert/strict";
import { buildCollaborationRecords } from "../src/collaborationRecords.ts";

const snapshot = () => ({
  project: { id: "A" }, agents: [{ id: "agent", name: "成员甲", role: "worker" }],
  agentSessions: [{ agentId: "agent", appSessionId: "session-a" }],
  tasks: [{ id: "task", title: "验收任务", assigneeAgentId: "agent" }], runs: [],
  messages: [{ id: "message", createdAt: 20, senderId: "agent", targetAgentId: null, body: "完整正文\n第二行" }],
  events: [{ seq: 1, createdAt: 30, eventType: "message.sent", payload: {}, entityType: "project", entityId: "A", actorId: "agent" }],
  runtimeEvents: [{ seq: 2, createdAt: 10, eventType: "failed", taskId: "task", agentId: "agent" }, { seq: 3, createdAt: 11, eventType: "output", taskId: "task", agentId: "agent" }],
  artifacts: [{ id: "artifact", taskId: "task", createdAt: 5, summary: "交付内容", contentHash: "hash" }],
  memories: [{ id: "memory", updatedAt: 1, namespace: "project", key: "decision", value: "项目决策", sourceTaskId: "task" }],
});
test("迁移保留所有原记录类型、完整正文、时间排序与成员身份", () => {
  const input = snapshot(); const before = structuredClone(input);
  const records = buildCollaborationRecords(input, [{ id: "delivery", at: 15, fromSessionId: "session-a", toSessionId: "other", body: "投递正文", kind: "direct" }]);
  assert.deepEqual(new Set(records.map(item => item.kind)), new Set(["消息", "事件", "运行", "交付", "经验", "直通"]));
  assert.deepEqual(records.map(item => item.at), [30,20,15,10,5,1]);
  assert.equal(records.find(item => item.id === "message-message").body, "完整正文\n第二行");
  assert.equal(records.find(item => item.id === "delivery-delivery").title, "成员甲 → other");
  assert.deepEqual(input, before);
});
test("只显示收发至少一方关联当前项目的投递，隐藏运行输出与心跳噪声", () => {
  const records = buildCollaborationRecords(snapshot(), [{ id: "foreign", at: 15, fromSessionId: "B-1", toSessionId: "B-2", body: "其他项目内容", kind: "direct" }, { id: "inbound", at: 16, fromSessionId: null, fromLabel: "用户", toSessionId: "session-a", body: "收到", kind: "broadcast" }]);
  assert.equal(records.some(item => item.id === "delivery-foreign"), false);
  assert.equal(records.find(item => item.id === "delivery-inbound").kind, "直通·广播");
  assert.equal(records.some(item => item.id === "runtime-event-3"), false);
});

test("统一入口保留原全部活动弹窗中的事件载荷", () => {
  const input = snapshot(); input.events[0].payload = { error: "原始失败原因", taskId: "task" };
  assert.deepEqual(JSON.parse(buildCollaborationRecords(input, []).find(item => item.id === "event-1").details), input.events[0].payload);
});
