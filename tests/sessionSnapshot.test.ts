import assert from "node:assert/strict";
import test from "node:test";
import { sameSessionSnapshot } from "../src/sessionSnapshot.ts";
import type { SessionRecord } from "../src/types.ts";

const session = (id: string, updatedAt: number) =>
  ({ id, updatedAt }) as SessionRecord;

test("会话轮询只在内容或顺序变化时更新 React 状态", () => {
  const current = [session("a", 1), session("b", 2)];
  assert.equal(sameSessionSnapshot(current, [session("a", 1), session("b", 2)]), true);
  assert.equal(sameSessionSnapshot(current, [session("a", 2), session("b", 2)]), false);
  assert.equal(sameSessionSnapshot(current, [session("b", 2), session("a", 1)]), false);
});
