import assert from "node:assert/strict";
import test from "node:test";
import { limitGroupedEntries } from "../src/gitSidebarLimit.ts";

test("Git 侧栏在分组之间共享同一个渲染上限", () => {
  const limited = limitGroupedEntries(
    [Array.from({ length: 400 }, (_, i) => i), Array.from({ length: 300 }, (_, i) => i + 400)],
    500,
  );
  assert.deepEqual(limited.map((group) => group.length), [400, 100]);
  assert.equal(limited.flat().length, 500);
});
