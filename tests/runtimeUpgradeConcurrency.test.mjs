import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript";

// 执行组件真实的升级处理函数；仅替换 IPC，避免测试升级本机软件。
const source = readFileSync(new URL("../src/components/CollaborationSettingsPanel.tsx", import.meta.url), "utf8");
const handlers = source.slice(source.indexOf("  const upgradeOneRuntime ="), source.indexOf("  const suggestedSkillSets ="));
function harness() {
  const calls = [];
  const pendingUpgrades = { current: new Set() };
  const runtimeCheckInFlight = { current: false };
  let active = new Set(), results = {}, rows = ["codex", "pi", "opencode"].map(kind => ({
    kind, version: "1.0.0", latestVersion: "2.0.0", updateAvailable: true, canAutoUpdate: true,
  }));
  const runtimeVersion = value => value ?? "—";
  const invoke = (_, { kind }) => new Promise((resolve, reject) => calls.push({ kind, resolve, reject }));
  const setUpgradingRuntimes = value => { active = value; };
  const setUpgradeResults = update => { results = update(results); };
  const setRuntimes = update => { rows = update(rows); };
  const busyRuntimeKinds = new Set();
  const RUNTIME_LABELS = {};
  const window = { confirm: () => true };
  const runtimes = rows;
  const availability = { isRuntimeEnabled: () => true };
  const js = ts.transpile(handlers + "\nreturn { upgradeRuntime, upgradeAllRuntimes };", { target: ts.ScriptTarget.ES2022 });
  const actions = eval("(function(){" + js + "})")();
  return { ...actions, calls, pendingUpgrades, runtimeCheckInFlight, active: () => active, results: () => results };
}

test("不同 Runtime 同时执行，同项重复点击只发起一次", async () => {
  const h = harness();
  const a = h.upgradeRuntime("codex"), b = h.upgradeRuntime("pi");
  await h.upgradeRuntime("codex");
  assert.deepEqual(h.calls.map(call => call.kind), ["codex", "pi"]);
  assert.equal(h.active().size, 2);
  h.calls[0].reject(new Error("下载失败"));
  await a;
  assert.deepEqual([...h.active()], ["pi"]);
  h.calls[1].resolve({ runtime: { kind: "pi", version: "2.0.0", error: null } });
  await b;
  assert.equal(h.active().size, 0);
  assert.equal(h.results().codex.error, true);
  assert.equal(h.results().pi.error, false);
});

test("版本检查期间拒绝升级，检查完成后恢复且不误记升级状态", async () => {
  const h = harness();
  h.runtimeCheckInFlight.current = true;
  await h.upgradeRuntime("codex");
  await h.upgradeAllRuntimes();
  assert.equal(h.calls.length, 0);
  assert.equal(h.active().size, 0);
  assert.equal(h.pendingUpgrades.current.size, 0);
  h.runtimeCheckInFlight.current = false;
  const pending = h.upgradeRuntime("codex");
  assert.deepEqual(h.calls.map(call => call.kind), ["codex"]);
  h.calls[0].resolve({ runtime: { kind: "codex", version: "2.0.0", error: null } });
  await pending;
  assert.equal(h.results().codex.error, false);
});

test("批量并行启动，跳过正在升级项，失败不阻断其他项", async () => {
  const h = harness();
  const single = h.upgradeRuntime("codex");
  const batch = h.upgradeAllRuntimes();
  assert.deepEqual(h.calls.map(call => call.kind), ["codex", "pi", "opencode"]);
  h.calls[1].reject(new Error("网络错误"));
  h.calls[2].resolve({ runtime: { kind: "opencode", version: "2.0.0", error: null } });
  await batch;
  assert.deepEqual([...h.active()], ["codex"]);
  h.calls[0].resolve({ runtime: { kind: "codex", version: "2.0.0", error: null } });
  await single;
  assert.equal(h.results().pi.error, true);
  assert.equal(h.results().opencode.error, false);
});
