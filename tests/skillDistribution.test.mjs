import assert from "node:assert/strict";
import test from "node:test";
import { distributionAssets, distributionSelection, scopedDistribution } from "../src/skillDistribution.ts";

const assets = [{ id: "a", name: "Browser Control", description: "浏览器", trustState: "trusted" }, { id: "b", name: "code-review", description: "审核", trustState: "unknown" }, { id: "c", name: "research", description: "资料", trustState: "blocked" }];
test("Runtime 分发搜索及状态过滤保留未知与异常状态，不改动原始列表", () => {
  const deployments = [{ skillId: "a", status: "synced" }, { skillId: "b", status: "future-failure" }];
  assert.deepEqual(distributionAssets(assets, deployments, " BROWSER ", "all"), [assets[0]]);
  assert.deepEqual(distributionAssets(assets, deployments, "审核", "attention"), [assets[1]]);
  assert.deepEqual(distributionAssets(assets, deployments, "", "undeployed"), [assets[2]]);
  assert.deepEqual(distributionAssets(assets, deployments, "", "untrusted"), assets.slice(1));
  assert.deepEqual(distributionAssets(assets, deployments, "", "synced"), [assets[0]]);
  assert.deepEqual(assets.map(a => a.id), ["a", "b", "c"]);
});
test("批量选择严格限制当前 Runtime 和当前可见 Skill，忽略过期与隐藏选择", () => {
  const selection = new Set(["a::claude_code", "a::codex", "b::claude_code", "deleted::claude_code"]);
  assert.deepEqual(distributionSelection(selection, "claude_code", [assets[0]]), ["a::claude_code"]);
  assert.deepEqual(distributionSelection(selection, "codex", assets), ["a::codex"]);
  assert.deepEqual(distributionSelection(selection, "pi", assets), []);
  assert.equal(selection.size, 4);
});
test("任务分发精确匹配项目、任务及 Agent 坐标，不显示其它任务的部署", () => {
  const base = { scope: "task", projectPath: "/fixture/worktree", taskId: "task-1", agentId: null };
  const items = [base, { ...base, taskId: "task-2" }, { ...base, agentId: "agent-1" }, { ...base, projectPath: "/fixture/other" }];
  assert.deepEqual(scopedDistribution(items, "task", "/fixture/worktree", " task-1 ", ""), [base]);
  assert.deepEqual(scopedDistribution(items, "task", "/fixture/worktree", "", ""), []);
  assert.deepEqual(scopedDistribution(items, "task", "/fixture/worktree", "task-1", "agent-1"), [items[2]]);
});
