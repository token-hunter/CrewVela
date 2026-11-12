import assert from "node:assert/strict";
import test from "node:test";
import {
  nextTrustPollDelay,
  sameTrustStatus,
  TRUST_POLL_ACTIVE_MS,
  TRUST_POLL_IDLE_MS,
} from "../src/controlTrustPolling.ts";
import type { ControlWorkspaceTrustStatus } from "../src/types.ts";

const idle: ControlWorkspaceTrustStatus = { batchCount: 0, awaitingCount: 0, startingCount: 0, missingCount: 0 };

test("trust status polling stays fast only while sessions are starting or awaiting trust", () => {
  assert.equal(nextTrustPollDelay(null), TRUST_POLL_IDLE_MS, "unknown status must not busy-poll");
  assert.equal(nextTrustPollDelay(idle), TRUST_POLL_IDLE_MS);
  assert.equal(nextTrustPollDelay({ ...idle, batchCount: 3, missingCount: 1 }), TRUST_POLL_IDLE_MS, "finished batches are idle");
  assert.equal(nextTrustPollDelay({ ...idle, startingCount: 1 }), TRUST_POLL_ACTIVE_MS);
  assert.equal(nextTrustPollDelay({ ...idle, awaitingCount: 2 }), TRUST_POLL_ACTIVE_MS);
  assert.ok(TRUST_POLL_ACTIVE_MS < TRUST_POLL_IDLE_MS);
});

test("equal trust statuses are recognised so unchanged polls do not re-render the sidebar", () => {
  assert.equal(sameTrustStatus(idle, { ...idle }), true);
  for (const key of ["batchCount", "awaitingCount", "startingCount", "missingCount"] as const) {
    assert.equal(sameTrustStatus(idle, { ...idle, [key]: 1 }), false, key);
  }
});
