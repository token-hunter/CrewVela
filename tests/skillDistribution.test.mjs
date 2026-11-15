import assert from "node:assert/strict";
import test from "node:test";
import { distributionAssets, distributionSelection, scopedDistribution } from "../src/skillDistribution.ts";

const assets = [{ id: "a", name: "Browser Control", description: "浏览器", trustState: "trusted" }, { id: "b", name: "code-review", description: "审核", trustState: "unknown" }, { id: "c", name: "research", description: "资料", trustState: "blocked" }];
test("Runtime 分发搜索及状态过滤保留未知与异常状态，不改动原始列表", () => {
  const deployments = [{ skillId: "a", runtime: "codex", status: "synced" }, { skillId: "b", runtime: "codex", status: "future-failure" }];
  assert.deepEqual(distributionAssets(assets, deployments, " BROWSER ", "all", ["codex"]), [assets[0]]);
  assert.deepEqual(distributionAssets(assets, deployments, "审核", "attention", ["codex"]), [assets[1]]);
  assert.deepEqual(distributionAssets(assets, deployments, "", "undeployed", ["codex"]), [assets[2]]);
  assert.deepEqual(distributionAssets(assets, deployments, "", "untrusted", ["codex"]), assets.slice(1));
  assert.deepEqual(distributionAssets(assets, deployments, "", "synced", ["codex"]), [assets[0]]);
  assert.deepEqual(assets.map(a => a.id), ["a", "b", "c"]);
});
test("批量选择只生成可见 Skill 与选中 Runtime 的组合，去重且忽略隐藏或过期 Skill", () => {
  const selection = new Set(["a", "b", "deleted"]);
  assert.deepEqual(distributionSelection(selection, ["claude_code", "codex", "codex"], [assets[0]]), ["a::claude_code", "a::codex"]);
  assert.deepEqual(distributionSelection(selection, ["codex"], assets), ["a::codex", "b::codex"]);
  assert.deepEqual(distributionSelection(selection, [], assets), []);
  assert.deepEqual(distributionSelection(new Set(), ["pi"], assets), []);
  assert.equal(selection.size, 3);
});
test("多目标筛选要求全部同步，保留部分未部署、冲突和未知状态", () => {
  const deployments = [{ skillId: "a", runtime: "claude_code", status: "synced" }, { skillId: "a", runtime: "codex", status: "conflict" }, { skillId: "b", runtime: "claude_code", status: "synced" }, { skillId: "c", runtime: "claude_code", status: "synced" }, { skillId: "c", runtime: "codex", status: "synced" }];
  const runtimes = ["claude_code", "codex"];
  assert.deepEqual(distributionAssets(assets, deployments, "", "synced", runtimes), [assets[2]]);
  assert.deepEqual(distributionAssets(assets, deployments, "", "undeployed", runtimes), [assets[1]]);
  assert.deepEqual(distributionAssets(assets, deployments, "", "attention", runtimes), [assets[0]]);
  assert.deepEqual(distributionAssets(assets, deployments, "", "synced", []), []);
});
test("任务分发精确匹配项目、任务及 Agent 坐标，不显示其它任务的部署", () => {
  const base = { scope: "task", projectPath: "/fixture/worktree", taskId: "task-1", agentId: null };
  const items = [base, { ...base, taskId: "task-2" }, { ...base, agentId: "agent-1" }, { ...base, projectPath: "/fixture/other" }];
  assert.deepEqual(scopedDistribution(items, "task", "/fixture/worktree", " task-1 ", ""), [base]);
  assert.deepEqual(scopedDistribution(items, "task", "/fixture/worktree", "", ""), []);
  assert.deepEqual(scopedDistribution(items, "task", "/fixture/worktree", "task-1", "agent-1"), [items[2]]);
});
