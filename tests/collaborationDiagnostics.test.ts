import assert from "node:assert/strict";
import test from "node:test";
import { activityLabel, diagnosticChecks, handoffRecoveryHistory, scopeHealth, sessionCheckState, taskMatchesFilter, taskPriority } from "../src/collaborationDiagnostics.ts";
import type { Health } from "../src/components/CollaborationSupportPanel.tsx";
import type { MultiAgentSnapshot } from "../src/types.ts";

const session = (extra = {}): Health["sessions"][number] => ({ id: "one", title: "Codex", runtime: "codex", cwd: "F:/test", ptyId: 1, status: "running", projectId: "p1", processRunning: true, ready: true, reason: "", error: null, waitingReason: null, interactiveHandoff: true, identityEnvironment: "", bridge: { configured: false, error: null }, ...extra });
test("completed means finished turn when live PTY is ready; busy and stopped are not errors", () => {
  assert.equal(sessionCheckState(session({ status: "completed" })), "正常");
  assert.equal(sessionCheckState(session({ ready: false })), "忙碌");
  assert.equal(sessionCheckState(session({ status: "stopped", ready: false })), "已停止");
  assert.equal(sessionCheckState(session({ archivedAt: 1, status: "error" })), "已停止");
  assert.equal(sessionCheckState(session({ ready: false, status: "waiting", waitingReason: "permission required" })), "等待授权");
  assert.equal(sessionCheckState(session({ ready: false, status: "waiting" })), "未验证");
  assert.equal(sessionCheckState(session({ commandHost: true })), "未验证");
  assert.equal(sessionCheckState(session({ ptyId: null, runtime: "zcode" })), "未验证");
});
test("scope uses exact project relationship and enabled runtimes, never cwd", () => {
  const health: Health = { cliPath: "exe", cliPathExists: true, replyEvidence: "", runtimes: [{ runtime: "codex", label: "Codex", supported: true, installed: true, skills: [{ path: "skill", matches: true, error: null }], searchedRoots: [] }], sessions: [session(), session({ id: "other", projectId: "p2" }), session({ id: "linked", projectId: null }), session({ id: "disabled", runtime: "pi" })], messages: [{ id: "m", toSessionId: "other", body: "", stage: "queued" }], handoffs: [] };
  const snapshot = { project: { id: "p1" }, agents: [{ runtimeKind: "codex", enabled: true }], agentSessions: [{ appSessionId: "linked" }] } as MultiAgentSnapshot;
  assert.deepEqual(scopeHealth(health, snapshot, false, ["codex"]).sessions.map((row) => row.id), ["one", "linked"]);
  assert.equal(scopeHealth(health, snapshot, false, ["codex"]).messages.length, 0);
  assert.equal(scopeHealth(health, null, false, ["codex"]).sessions.length, 0);
  assert.equal(scopeHealth(health, null, true, ["codex"]).sessions.length, 3);
  const mixed = { ...health, sessions: [session(), session({ id: "busy", ready: false }), session({ id: "old", status: "stopped" })] };
  const checks = diagnosticChecks(mixed);
  assert.equal(checks.length, 5);
  assert.equal(checks.filter((row) => row.state === "异常").length, 0);
  assert.equal(checks.filter((row) => row.state === "忙碌").length, 1);
  health.runtimes[0].skills.push({ path: "other-project-skill", matches: false, error: null, projectIds: ["p2"], globalRoot: false });
  health.runtimes[0].skills.push({ path: "unscoped-project-skill", matches: false, error: null, projectIds: [], globalRoot: false });
  assert.equal(scopeHealth(health, snapshot, false, ["codex"]).runtimes[0].skills.length, 1);
  assert.equal(scopeHealth(health, snapshot, true, ["codex"]).runtimes[0].skills.length, 3);
  health.runtimes[0].skills.push({ path: "linked-project-skill", matches: true, error: null, projectIds: [], sessionIds: ["linked"], globalRoot: false });
  assert.equal(scopeHealth(health, snapshot, false, ["codex"]).runtimes[0].skills.length, 2);
});
test("overview prioritizes actionable tasks and preserves readable events", () => {
  assert.ok(taskPriority("failed") < taskPriority("done"));
  assert.ok(taskPriority("in_review") < taskPriority("running"));
  assert.equal(activityLabel("run.status_changed"), "执行状态更新");
  assert.equal(activityLabel("成员已提交交付"), "成员已提交交付");
  assert.equal(taskMatchesFilter("waiting_approval", "in_review"), true);
  assert.equal(taskMatchesFilter("running", "attention"), false);
});
test("independent interrupted handoffs remain recoverable in project diagnostic scope", () => {
  const draft = { sourceId: "source", targetId: "target", context: "handoff", state: "source_stopped", error: "delivery failed", consequence: "source stopped" };
  const health: Health = { cliPath: "exe", cliPathExists: true, replyEvidence: "", runtimes: [], sessions: [], messages: [], handoffs: [
    { ...draft, id: "independent", mode: "session", projectId: null },
    { ...draft, id: "task-current", mode: "task", projectId: "p1" },
    { ...draft, id: "task-other", mode: "task", projectId: "p2" },
  ] };
  assert.deepEqual(handoffRecoveryHistory(health, "p1").map((item) => item.id), ["independent", "task-current"]);
  assert.deepEqual(handoffRecoveryHistory(health).map((item) => item.id), ["independent"]);
});
