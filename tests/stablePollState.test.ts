import assert from "node:assert/strict";
import test from "node:test";
import { keepIfUnchanged, sameValue } from "../src/stablePollState.ts";

test("equal polled payloads keep the previous reference so React skips the render", () => {
  const current = [{ id: "a", at: 1, tags: ["x"] }];
  assert.equal(keepIfUnchanged(current, [{ id: "a", at: 1, tags: ["x"] }]), current);
  const usages = { s1: { inputTokens: 1, outputTokens: 2 }, s2: { inputTokens: 3, outputTokens: 4 } };
  assert.equal(keepIfUnchanged(usages, { s1: { inputTokens: 1, outputTokens: 2 }, s2: { inputTokens: 3, outputTokens: 4 } }), usages);
});

test("object key order is irrelevant because Rust HashMap serialisation order changes per call", () => {
  const current = { s1: { inputTokens: 1, outputTokens: 2 }, s2: { inputTokens: 3, outputTokens: 4 } };
  const reordered = { s2: { outputTokens: 4, inputTokens: 3 }, s1: { outputTokens: 2, inputTokens: 1 } };
  assert.equal(keepIfUnchanged(current, reordered), current);
});

test("any real change is adopted instead of being swallowed", () => {
  const current: Record<string, { tokens: number }> = { s1: { tokens: 1 } };
  const changed = { s1: { tokens: 2 } };
  assert.equal(keepIfUnchanged(current, changed), changed, "changed value");
  const added = { s1: { tokens: 1 }, s2: { tokens: 0 } };
  assert.equal(keepIfUnchanged(current, added), added, "added key");
  const removed = {};
  assert.equal(keepIfUnchanged(current, removed), removed, "removed key");
  const list = [{ id: "a" }, { id: "b" }];
  const swapped = [{ id: "b" }, { id: "a" }];
  assert.equal(keepIfUnchanged(list, swapped), swapped, "array order is significant");
  const longer = [{ id: "a" }, { id: "b" }, { id: "c" }];
  assert.equal(keepIfUnchanged(list, longer), longer, "array length");
  const grown: unknown[] = [{}];
  assert.equal(keepIfUnchanged<unknown[]>([], grown), grown, "empty to non-empty");
});

test("sameValue separates primitives, null, arrays and objects", () => {
  assert.equal(sameValue(null, null), true);
  assert.equal(sameValue(null, {}), false);
  assert.equal(sameValue({}, []), false, "an object is not an array");
  assert.equal(sameValue([], {}), false);
  assert.equal(sameValue(1, "1"), false);
  assert.equal(sameValue(NaN, NaN), true);
  assert.equal(sameValue({ a: undefined }, {}), false, "missing key differs from undefined value");
  assert.equal(sameValue({ a: 1 }, { b: 1 }), false, "same size, different keys");
});
