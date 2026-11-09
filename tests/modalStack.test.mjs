import test from "node:test";
import assert from "node:assert/strict";
import {
  hasOpenModal,
  isTopModal,
  modalStackSize,
  modalStackSnapshot,
  pushModal,
  removeModal,
} from "../src/components/ui/modalStack.ts";

test("empty stack starts closed", () => {
  assert.equal(hasOpenModal(), false);
  assert.equal(modalStackSize(), 0);
  assert.deepEqual(modalStackSnapshot(), []);
});

test("push assigns increasing ids and top is the last pushed", () => {
  const a = pushModal();
  const b = pushModal();
  const c = pushModal();
  assert.deepEqual(modalStackSnapshot(), [a, b, c]);
  assert.equal(isTopModal(a), false);
  assert.equal(isTopModal(b), false);
  assert.equal(isTopModal(c), true);
  assert.equal(hasOpenModal(), true);
  removeModal(a);
  removeModal(b);
  removeModal(c);
});

test("remove pops in mount order so the previous modal becomes top", () => {
  const settings = pushModal();
  const editor = pushModal();
  assert.equal(isTopModal(editor), true);
  // Closing the nested editor must hand Escape back to the settings dialog.
  removeModal(editor);
  assert.equal(isTopModal(editor), false);
  assert.equal(isTopModal(settings), true);
  assert.equal(modalStackSize(), 1);
  removeModal(settings);
  assert.equal(hasOpenModal(), false);
});

test("remove is idempotent for unknown or already-removed ids", () => {
  const a = pushModal();
  removeModal(a);
  removeModal(a);
  removeModal(999999);
  assert.equal(modalStackSize(), 0);
});

test("removing a non-top modal keeps relative order of the rest", () => {
  const a = pushModal();
  const b = pushModal();
  const c = pushModal();
  removeModal(b);
  assert.deepEqual(modalStackSnapshot(), [a, c]);
  assert.equal(isTopModal(c), true);
  removeModal(a);
  removeModal(c);
});
