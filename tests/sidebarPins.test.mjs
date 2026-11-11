import assert from "node:assert/strict";
import test from "node:test";
import { loadSidebarPins, pinnedFirst, toggleSidebarPin, PINNED_SESSIONS_STORAGE_KEY, PINNED_PROJECTS_STORAGE_KEY } from "../src/sidebarPins.ts";

test("多个置顶项稳定排序，不改写源列表，取消后回归原顺序", () => {
  const items = [{ id: "a" }, { id: "b" }, { id: "c" }, { id: "d" }];
  const pins = new Set(["d", "b", "deleted"]);
  assert.deepEqual(pinnedFirst(items, pins, item => item.id).map(item => item.id), ["b", "d", "a", "c"]);
  assert.deepEqual(items.map(item => item.id), ["a", "b", "c", "d"]);
  const next = toggleSidebarPin(toggleSidebarPin(pins, "b"), "d");
  assert.deepEqual(pinnedFirst(items, next, item => item.id), items);
  assert.equal(pins.has("b"), true);
});

test("项目与会话分别保存；重新读取保留置顶状态并忽略非法条目", () => {
  const values = new Map([
    [PINNED_SESSIONS_STORAGE_KEY, '["same",null,4,"same"]'],
    [PINNED_PROJECTS_STORAGE_KEY, '["project-b"]'],
  ]);
  const storage = { getItem: key => values.get(key) ?? null };
  assert.deepEqual([...loadSidebarPins(PINNED_SESSIONS_STORAGE_KEY, storage)], ["same"]);
  assert.deepEqual([...loadSidebarPins(PINNED_PROJECTS_STORAGE_KEY, storage)], ["project-b"]);
  assert.deepEqual([...loadSidebarPins("missing", storage)], []);
  assert.deepEqual([...loadSidebarPins("invalid-shape", { getItem: () => '{}' })], []);
  assert.throws(() => loadSidebarPins("bad-json", { getItem: () => '[' }));
  assert.throws(() => loadSidebarPins("unavailable", { getItem() { throw new Error("denied"); } }), /denied/);
});

test("筛选后只排序已有项，接力会话沿用逻辑身份的置顶状态", () => {
  const pins = new Set(["relay-root", "hidden"]);
  const filtered = [{ id: "regular", root: "regular" }, { id: "replacement", root: "relay-root" }];
  assert.deepEqual(pinnedFirst(filtered, pins, item => item.root).map(item => item.id), ["replacement", "regular"]);
  assert.deepEqual(pinnedFirst([], pins, item => item.id), []);
});
