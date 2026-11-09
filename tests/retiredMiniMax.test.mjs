import assert from "node:assert/strict";
import test from "node:test";
import { TERMINAL_RUNTIMES, COLLABORATION_RUNTIMES, isRetiredRuntime } from "../src/runtimeTaxonomy.ts";
import { switchTargets, sessionSwitchReason } from "../src/sessionSwitch.ts";
import { decodeMultiAgentDraft } from "../src/multiAgentDraftState.ts";

test("retired MiniMax CLI remains identifiable but is never a terminal, collaboration or relay target", () => {
  assert.equal(isRetiredRuntime("minimax_code"), true);
  assert.equal(TERMINAL_RUNTIMES.includes("minimax_code"), false);
  assert.equal(COLLABORATION_RUNTIMES.includes("minimax_code"), false);
  const legacy = { kind: "terminal", agentRuntimeKind: "minimax_code" };
  assert.deepEqual(switchTargets(legacy), []);
  assert.ok(sessionSwitchReason(legacy));
  assert.equal(isRetiredRuntime("kimi_code"), false);
  assert.equal(isRetiredRuntime("codex"), false);
});

test("persisted MiniMax draft plans cannot be restored into actionable execution", () => {
  const plan = {
    intent: "plan_project", reply: "legacy", projectName: "legacy", goal: "legacy",
    rootPath: "C:/legacy", targetTaskId: null, targetAgentId: null,
    agents: [{ key: "legacy", name: "legacy", runtimeKind: "minimax_code", role: "worker", description: "", model: null, permissionProfile: "default" }],
    tasks: [], requestedRuntimes: ["minimax_code"], requiresConfirmation: true,
  };
  const draft = decodeMultiAgentDraft(JSON.stringify({ projectPath: "C:/legacy", coordinatorProjectId: null, coordinatorDraft: "preserve my text", coordinatorMessages: [], mentionBindings: [], pendingPlan: plan, pendingPlanContext: { projectId: null, projectPath: "C:/legacy", requestedRuntimes: ["minimax_code"] } }));
  assert.equal(draft.pendingPlan, null);
  assert.equal(draft.pendingPlanContext, null);
  assert.equal(draft.coordinatorDraft, "preserve my text");
});
