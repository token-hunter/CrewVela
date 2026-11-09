import test from "node:test";
import assert from "node:assert/strict";
import { buildTeamMessages, deliveryStageLabel, normalizeRightSidebarTab, summarizeTeamSend } from "../src/teamMessages.ts";

function snapshot() {
  return { project: { id: "p" }, agents: [{ id: "a", name: "Claude" }, { id: "b", name: "Pi" }],
    agentSessions: [{ agentId: "a", appSessionId: "s-a" }, { agentId: "b", appSessionId: "s-b" }], tasks: [], runs: [], messages: [] };
}
function message(id, body = "hi", targetAgentId = "a") {
  return { id, projectId: "p", senderType: "agent", senderId: "b", targetAgentId, body, createdAt: 100 };
}
function evidence(id, stage = "queued", toSessionId = "s-a") {
  return { id, at: 101, fromSessionId: "s-b", toSessionId, body: "hi", kind: "chat", stage };
}

test("共同 ID 和目标匹配时历史与实时证据合并，排队不会标为回复", () => {
  const data = snapshot(); data.messages = [message("one")];
  const items = buildTeamMessages(data, [evidence("one")]);
  assert.equal(items.length, 1);
  assert.equal(items[0].stage, "queued");
  assert.equal(deliveryStageLabel(items[0].stage), "排队中");
  assert.equal(deliveryStageLabel("submit_sent"), "已提交输入");
  assert.equal(deliveryStageLabel("bridge_received"), "运行时已接收");
});

test("相同正文、不同消息 ID 或不同目标不得猜测合并", () => {
  const data = snapshot(); data.messages = [message("old"), message("new")];
  const items = buildTeamMessages(data, [evidence("new"), evidence("external")]);
  assert.equal(items.length, 3);
  assert.equal(items.find(item => item.id === "message:old").stage, "unknown");
  const wrongTarget = buildTeamMessages(data, [evidence("new", "submit_sent", "s-b")]);
  assert.equal(wrongTarget.find(item => item.id === "message:new").stage, "unknown");
  assert.equal(wrongTarget.length, 3);
});

test("其他项目和初始 Prompt 不混入，旧项目历史重启后仍有未知状态", () => {
  const data = snapshot(); data.messages = [message("history"), { ...message("other"), projectId: "other" }];
  const items = buildTeamMessages(data, [{ ...evidence("external"), fromSessionId: "outside", toSessionId: "outside" },
    { ...evidence("initial"), kind: "initial_prompt" }]);
  assert.equal(items.length, 1);
  assert.equal(items[0].stage, "unknown");
});

test("失败原因和执行摘要保留来源，终端输出不被当作团队回复", () => {
  const data = snapshot(); data.messages = [message("m")];
  data.runLogs = [{ content: "terminal output" }];
  data.runs = [{ id: "run", projectId: "p", taskId: "task", agentId: "a", attempt: 2,
    status: "failed", outputSummary: "summary", error: "测试未通过", createdAt: 200 }];
  const items = buildTeamMessages(data, [{ ...evidence("m", "text_written"), error: "回车失败" }]);
  assert.equal(items.length, 2);
  assert.equal(items[0].error, "回车失败");
  assert.match(items[1].source, /任务执行摘要.*Agent 报告/);
  assert.equal(items[1].body, "测试未通过");
});

test("旧群聊页签和损坏存储恢复到协作入口", () => {
  assert.equal(normalizeRightSidebarTab("group_chat"), "multi_agent");
  assert.equal(normalizeRightSidebarTab(null), "multi_agent");
  assert.equal(normalizeRightSidebarTab("files"), "files");
  assert.equal(normalizeRightSidebarTab("git"), "git");
});

test("全失败保留原文，部分成功清空且只列出失败对象", () => {
  const fail = { status: "rejected", reason: "会话已停止" };
  const ok = { status: "fulfilled", value: "Claude" };
  assert.equal(summarizeTeamSend([fail], ["one"]).clearDraft, false);
  assert.equal(summarizeTeamSend([fail, fail], ["one", "two"]).clearDraft, false);
  const partial = summarizeTeamSend([ok, fail], ["one", "two"]);
  assert.equal(partial.clearDraft, true);
  assert.deepEqual(partial.failures, ["two：会话已停止"]);
  assert.equal(summarizeTeamSend([ok, ok], ["one", "two"]).failures.length, 0);
});
