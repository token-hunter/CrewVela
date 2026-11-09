import assert from "node:assert/strict";
import test from "node:test";
import { getPetWorkPhase } from "../src/components/sessionPetState.ts";

test("running sessions progress from typing to busy to overheated", () => {
  assert.equal(getPetWorkPhase("running", 0), "typing");
  assert.equal(getPetWorkPhase("running", 179_999), "typing");
  assert.equal(getPetWorkPhase("running", 180_000), "busy");
  assert.equal(getPetWorkPhase("running", 479_999), "busy");
  assert.equal(getPetWorkPhase("running", 480_000), "overheated");
});

test("non-running sessions always use the idle phase", () => {
  assert.equal(getPetWorkPhase("pending", 900_000), "idle");
  assert.equal(getPetWorkPhase("waiting", 900_000), "idle");
  assert.equal(getPetWorkPhase("completed", 900_000), "idle");
  assert.equal(getPetWorkPhase("error", 900_000), "idle");
  assert.equal(getPetWorkPhase("stopped", 900_000), "idle");
});

test("negative elapsed time is treated as a fresh running session", () => {
  assert.equal(getPetWorkPhase("running", -1), "typing");
});
