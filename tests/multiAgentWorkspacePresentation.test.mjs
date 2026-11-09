import test from "node:test";
import assert from "node:assert/strict";
import { workspaceParticipation } from "../src/multiAgentWorkspacePresentation.ts";

test("历史会话和失败任务不进入当前参与及待处理摘要", () => {
  const result = workspaceParticipation({ agentSessions: [{ agentId: "old", appSessionId: "old-session", status: "running" }], tasks: [{ assigneeAgentId: "old", status: "failed" }], runs: [{ agentId: "old", status: "failed" }] }, [{ id: "old-session", ptyId: null, status: "stopped" }]);
  assert.equal(result.participating.size, 0);
  assert.equal(result.executing.size, 0);
  assert.equal(result.pending.size, 0);
});
test("真实空闲终端只计参与，活动运行及审查分别计数", () => {
  const result = workspaceParticipation({ agentSessions: [{ agentId: "idle", appSessionId: "live" }], tasks: [{ assigneeAgentId: "worker", reviewerAgentId: "reviewer", status: "in_review" }], runs: [{ agentId: "runner", status: "running" }, { agentId: "input", status: "waiting_input" }] }, [{ id: "live", ptyId: 2, status: "running" }]);
  assert.deepEqual([...result.participating].sort(), ["idle", "input", "reviewer", "runner", "worker"]);
  assert.deepEqual([...result.executing], ["runner"]);
  assert.deepEqual([...result.pending].sort(), ["input", "reviewer", "worker"]);
});
