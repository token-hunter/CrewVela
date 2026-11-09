import assert from "node:assert/strict";
import test from "node:test";
import { autoApprovalTaskIds, canAutoContinueProject, mergeFailedCollaborationMessages } from "../src/collaborationAutomation.ts";

test("Auto continuation respects a pause or cancellation made during approval", () => {
  for (const status of ["paused", "cancelled"]) {
    assert.equal(canAutoContinueProject({ project: { id: "project", status } }, "project"), false);
  }
  assert.equal(canAutoContinueProject({ project: { id: "other", status: "active" } }, "project"), false);
  assert.equal(canAutoContinueProject({ project: { id: "project", status: "active" } }, "project"), true);
});

test("Auto only continues successful deliveries without independent review or permission requests", () => {
  const snapshot = { tasks: [
    { id: "ok", status: "in_review", reviewerAgentId: null },
    { id: "review", status: "in_review", reviewerAgentId: "reviewer" },
    { id: "permission", status: "in_review" },
    { id: "failed", status: "failed" },
    { id: "stale", status: "in_review" },
  ], runs: [
    ...["ok", "review", "permission", "failed", "stale"].map((taskId) => ({ taskId, status: "succeeded", attempt: 1 })),
    { taskId: "stale", status: "failed", attempt: 2 },
  ], approvals: [
    { taskId: "ok", status: "pending", approvalType: "task_review" },
    { taskId: "permission", status: "pending", approvalType: "tool_permission" },
  ] };
  assert.deepEqual(autoApprovalTaskIds(snapshot), ["ok"]);
});

test("Polling retains failed messages and does not duplicate already persisted user messages", () => {
  const user = { role: "user", content: "打个招呼", surface: "multi_agent" };
  const error = { role: "assistant", content: "定向协作失败", surface: "multi_agent" };
  assert.deepEqual(mergeFailedCollaborationMessages([], [user, error]), [user, error]);
  assert.deepEqual(mergeFailedCollaborationMessages([user], [user, error]), [user, error]);
  assert.deepEqual(mergeFailedCollaborationMessages([user], [user, error, user, error]), [user, error, user, error]);
});
